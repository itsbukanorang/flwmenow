#!/usr/bin/env python3

import argparse
import base64
import json
import os
import shutil
import signal
import subprocess
import time
from importlib.metadata import version
from typing import Any

from fi.alk.evals import evaluate_task_evidence_auto


ALK_PACKAGE = "agent-learning-kit"
ALK_VERSION = "0.3.4"
DEFAULT_THRESHOLD = 0.70
DEFAULT_INTERVAL = 5.0
DEFAULT_BATCH_SIZE = 10

_running = True


def stop_handler(*_: Any) -> None:
    global _running
    _running = False


def psql(sql: str) -> str:
    binary = shutil.which("psql")

    if not binary:
        raise RuntimeError("psql not found")

    database = os.environ.get(
        "FLWME_DB_NAME",
        "flwmenow",
    )

    proc = subprocess.run(
        [
            binary,
            "-X",
            "-A",
            "-t",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            database,
            "-c",
            sql,
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    return proc.stdout.strip()


def candidate_ids(
    *,
    evidence_id: int | None,
    limit: int,
) -> list[int]:
    if evidence_id is not None:
        where_id = f"AND le.id = {int(evidence_id)}"
    else:
        where_id = ""

    rows = psql(
        f"""
        SELECT le.id
        FROM learning_evidence le
        WHERE le.learning_status = 'observed'
          AND le.verification_status = 'pending'
          {where_id}
        ORDER BY le.created_at ASC
        LIMIT {int(limit)};
        """
    )

    if not rows:
        return []

    return [
        int(line.strip())
        for line in rows.splitlines()
        if line.strip()
    ]

def load_evidence(
    evidence_id: int,
) -> dict[str, Any]:
    raw = psql(
        f"""
        SELECT json_build_object(
            'id', le.id,
            'schema_version',
                le.schema_version,
            'task_id',
                le.task_id,
            'run_id',
                le.run_id,
            'agent',
                le.agent,
            'task_description',
                le.objective,
            'result',
                le.output_json,
            'evidence',
                le.evidence_json,
            'environment_state',
                json_build_object(
                    'company_id',
                        le.company_id,
                    'user_id',
                        le.user_id,
                    'verification_status',
                        le.verification_status,
                    'learning_status',
                        le.learning_status,
                    'source_event',
                        le.source_event
                ),
            'metadata',
                json_build_object(
                    'flwme_evidence_id',
                        le.id,
                    'company_id',
                        le.company_id,
                    'user_id',
                        le.user_id,
                    'task_id',
                        le.task_id,
                    'run_id',
                        le.run_id,
                    'agent',
                        le.agent
                )
        )::text
        FROM learning_evidence le
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'observed'
          AND le.verification_status = 'pending';
        """
    )

    if not raw:
        raise RuntimeError(
            f"evidence {evidence_id} unavailable"
        )

    return json.loads(raw)


def evaluate(
    evidence: dict[str, Any],
    threshold: float,
) -> dict[str, Any]:
    installed = version(ALK_PACKAGE)

    if installed != ALK_VERSION:
        raise RuntimeError(
            "ALK version drift: "
            f"expected={ALK_VERSION} "
            f"actual={installed}"
        )

    evidence_id = int(evidence["id"])

    result = evaluate_task_evidence_auto(
        evidence,
        threshold=threshold,
        name=f"flwme-evidence-{evidence_id}",
        source_path=".",
        metadata={
            "runtime": "flwme",
            "phase": "ALK-01D",
            "automatic": True,
            "flwme_evidence_id": evidence_id,
        },
    )

    if (
        result.get("schema_version")
        != "agent-learning.artifact-evaluation.v1"
    ):
        raise RuntimeError(
            "unexpected ALK evaluation schema"
        )

    if result.get("status") not in {
        "passed",
        "failed",
    }:
        raise RuntimeError(
            "unexpected ALK evaluation status"
        )

    summary = result.get("summary") or {}

    score = summary.get("score")
    eval_threshold = summary.get("threshold")

    if not isinstance(
        score,
        (int, float),
    ):
        raise RuntimeError(
            "evaluation score missing"
        )

    if not isinstance(
        eval_threshold,
        (int, float),
    ):
        raise RuntimeError(
            "evaluation threshold missing"
        )

    return result


def persist(
    evidence_id: int,
    result: dict[str, Any],
) -> str:
    payload = json.dumps(
        result,
        separators=(",", ":"),
        sort_keys=True,
        default=str,
    ).encode("utf-8")

    encoded = base64.b64encode(
        payload
    ).decode("ascii")

    output = psql(
        f"""
        BEGIN;

        WITH payload AS (
            SELECT
                convert_from(
                    decode(
                        '{encoded}',
                        'base64'
                    ),
                    'UTF8'
                )::jsonb AS value
        )
        INSERT INTO learning_evaluations (
            evidence_id,
            evaluator,
            evaluator_version,
            schema_version,
            status,
            score,
            threshold,
            findings_json,
            metrics_json,
            result_json
        )
        SELECT
            le.id,
            'future-agi-agent-learning-kit',
            '{ALK_VERSION}',
            payload.value
                ->> 'schema_version',
            payload.value
                ->> 'status',
            (
                payload.value
                -> 'summary'
                ->> 'score'
            )::double precision,
            (
                payload.value
                -> 'summary'
                ->> 'threshold'
            )::double precision,
            COALESCE(
                payload.value
                    -> 'findings',
                '[]'::jsonb
            ),
            COALESCE(
                payload.value
                    -> 'summary'
                    -> 'metric_averages',
                '{{}}'::jsonb
            ),
            payload.value
        FROM learning_evidence le
        CROSS JOIN payload
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'observed'
          AND le.verification_status = 'pending'
        ON CONFLICT (evidence_id)
        DO NOTHING;

        UPDATE learning_evidence le
        SET
            learning_status = 'evaluated',
            updated_at = now()
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'observed'
          AND le.verification_status = 'pending'
          AND EXISTS (
              SELECT 1
              FROM learning_evaluations ev
              WHERE ev.evidence_id = le.id
          );

        COMMIT;

        SELECT
            le.id || '|' ||
            le.learning_status || '|' ||
            le.verification_status
        FROM learning_evidence le
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'evaluated';
        """
    )

    expected_id = str(int(evidence_id))

    for line in reversed(
        output.splitlines()
    ):
        value = line.strip()
        parts = value.split("|")

        if (
            len(parts) == 3
            and parts[0] == expected_id
            and parts[1] == "evaluated"
        ):
            return value

    return ""

def process_one(
    evidence_id: int,
    *,
    threshold: float,
    dry_run: bool,
) -> None:
    print(
        f"AUTO_EVAL_START "
        f"evidence_id={evidence_id}",
        flush=True,
    )

    evidence = load_evidence(
        evidence_id
    )

    result = evaluate(
        evidence,
        threshold,
    )

    summary = result["summary"]

    print(
        "AUTO_EVAL_RESULT "
        f"evidence_id={evidence_id} "
        f"status={result['status']} "
        f"score={summary['score']} "
        f"threshold={summary['threshold']} "
        f"findings={summary['finding_count']}",
        flush=True,
    )

    if dry_run:
        print(
            "AUTO_EVAL_DRY_RUN "
            f"evidence_id={evidence_id}",
            flush=True,
        )
        return

    state = persist(
        evidence_id,
        result,
    )

    if not state:
        raise RuntimeError(
            "evaluation persistence "
            f"did not transition evidence "
            f"{evidence_id}"
        )

    print(
        "AUTO_EVAL_PERSISTED "
        f"state={state}",
        flush=True,
    )


def run_batch(
    *,
    evidence_id: int | None,
    threshold: float,
    limit: int,
    dry_run: bool,
) -> int:
    ids = candidate_ids(
        evidence_id=evidence_id,
        limit=limit,
    )

    if not ids:
        print(
            "AUTO_EVAL_IDLE "
            "observed_evidence=0",
            flush=True,
        )
        return 0

    failures = 0

    for item in ids:
        try:
            process_one(
                item,
                threshold=threshold,
                dry_run=dry_run,
            )
        except Exception as exc:
            failures += 1

            print(
                "AUTO_EVAL_ERROR "
                f"evidence_id={item} "
                f"error={type(exc).__name__}:"
                f"{exc}",
                flush=True,
            )

    return failures


def main() -> int:
    parser = argparse.ArgumentParser()

    mode = parser.add_mutually_exclusive_group(
        required=True
    )

    mode.add_argument(
        "--once",
        action="store_true",
    )

    mode.add_argument(
        "--loop",
        action="store_true",
    )

    parser.add_argument(
        "--dry-run",
        action="store_true",
    )

    parser.add_argument(
        "--evidence-id",
        type=int,
    )

    parser.add_argument(
        "--threshold",
        type=float,
        default=DEFAULT_THRESHOLD,
    )

    parser.add_argument(
        "--interval",
        type=float,
        default=DEFAULT_INTERVAL,
    )

    parser.add_argument(
        "--batch-size",
        type=int,
        default=DEFAULT_BATCH_SIZE,
    )

    args = parser.parse_args()

    if not (
        0.0 <= args.threshold <= 1.0
    ):
        raise ValueError(
            "threshold must be 0..1"
        )

    if args.interval < 1.0:
        raise ValueError(
            "interval must be >= 1 second"
        )

    if args.batch_size < 1:
        raise ValueError(
            "batch-size must be >= 1"
        )

    if args.once:
        failures = run_batch(
            evidence_id=args.evidence_id,
            threshold=args.threshold,
            limit=args.batch_size,
            dry_run=args.dry_run,
        )

        return 1 if failures else 0

    signal.signal(
        signal.SIGTERM,
        stop_handler,
    )

    signal.signal(
        signal.SIGINT,
        stop_handler,
    )

    print(
        "AUTO_EVALUATOR_STARTED "
        f"alk_version={ALK_VERSION} "
        f"interval={args.interval}",
        flush=True,
    )

    while _running:
        run_batch(
            evidence_id=None,
            threshold=args.threshold,
            limit=args.batch_size,
            dry_run=args.dry_run,
        )

        if _running:
            time.sleep(args.interval)

    print(
        "AUTO_EVALUATOR_STOPPED",
        flush=True,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
