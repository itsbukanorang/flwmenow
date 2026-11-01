#!/usr/bin/env python3

import argparse
import base64
import json
import os
import shutil
import signal
import subprocess
import time
from typing import Any

from deterministic_verifier import (
    REQUEST_SCHEMA,
    RESULT_SCHEMA,
    VERIFIER_NAME,
    VERIFIER_VERSION,
    verify,
)


DEFAULT_INTERVAL = 5.0
DEFAULT_BATCH_SIZE = 10

_running = True


def stop_handler(*_: Any) -> None:
    global _running
    _running = False


def psql(sql: str) -> str:
    binary = shutil.which("psql")

    if not binary:
        raise RuntimeError(
            "psql not found"
        )

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


def extract_path(
    value: Any,
    path: str,
) -> Any:
    parts = [
        part.strip()
        for part in path.split(".")
        if part.strip()
    ]

    if not parts:
        raise ValueError(
            "actual_path is empty"
        )

    current = value

    for part in parts:
        if isinstance(
            current,
            dict,
        ):
            if part not in current:
                raise KeyError(
                    f"path segment {part!r} "
                    "not found"
                )

            current = current[part]

            continue

        if isinstance(
            current,
            list,
        ):
            try:
                index = int(part)
            except ValueError as exc:
                raise KeyError(
                    f"path segment {part!r} "
                    "is not a list index"
                ) from exc

            if (
                index < 0
                or index >= len(current)
            ):
                raise KeyError(
                    f"list index {index} "
                    "out of range"
                )

            current = current[index]

            continue

        raise KeyError(
            f"path segment {part!r} "
            "cannot traverse current value"
        )

    return current


def candidate_ids(
    *,
    evidence_id: int | None,
    limit: int,
) -> list[int]:
    if evidence_id is None:
        id_clause = ""
    else:
        id_clause = (
            f"AND le.id = "
            f"{int(evidence_id)}"
        )

    rows = psql(
        f"""
        SELECT le.id
        FROM learning_evidence le
        JOIN learning_evaluations ev
          ON ev.evidence_id = le.id
        JOIN learning_verification_oracles o
          ON o.company_id = le.company_id
         AND o.task_id = le.task_id
         AND o.active = TRUE
        WHERE le.learning_status = 'evaluated'
          AND le.verification_status = 'pending'
          {id_clause}
        ORDER BY le.created_at ASC
        LIMIT {int(limit)};
        """
    )

    if not rows:
        return []

    return [
        int(line)
        for line in rows.splitlines()
        if line.strip()
    ]


def load_case(
    evidence_id: int,
) -> dict[str, Any]:
    raw = psql(
        f"""
        SELECT json_build_object(
            'evidence_id',
                le.id,
            'company_id',
                le.company_id,
            'task_id',
                le.task_id,
            'output',
                le.output_json,
            'alk_quality_score',
                ev.score,
            'oracle_id',
                o.id,
            'oracle',
                json_build_object(
                    'source_type',
                        o.source_type,
                    'source_company_id',
                        o.company_id,
                    'comparison',
                        o.comparison,
                    'expected',
                        o.expected_json,
                    'tolerance',
                        o.tolerance,
                    'actual_path',
                        o.actual_path
                )
        )::text
        FROM learning_evidence le
        JOIN learning_evaluations ev
          ON ev.evidence_id = le.id
        JOIN learning_verification_oracles o
          ON o.company_id = le.company_id
         AND o.task_id = le.task_id
         AND o.active = TRUE
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'evaluated'
          AND le.verification_status = 'pending'
        LIMIT 1;
        """
    )

    if not raw:
        raise RuntimeError(
            f"verification case "
            f"{evidence_id} unavailable"
        )

    return json.loads(raw)


def build_request(
    case: dict[str, Any],
) -> dict[str, Any]:
    oracle = case["oracle"]

    actual = extract_path(
        case["output"],
        oracle["actual_path"],
    )

    request_oracle = {
        "source_type":
            oracle["source_type"],
        "source_company_id":
            case["company_id"],
        "comparison":
            oracle["comparison"],
        "expected":
            oracle["expected"],
    }

    if (
        oracle["comparison"]
        == "numeric_tolerance"
    ):
        request_oracle["tolerance"] = (
            oracle["tolerance"]
        )

    return {
        "schema_version":
            REQUEST_SCHEMA,
        "evidence_id":
            case["evidence_id"],
        "company_id":
            case["company_id"],
        "alk_quality_score":
            case["alk_quality_score"],
        "oracle":
            request_oracle,
        "actual":
            actual,
    }


def persist(
    *,
    evidence_id: int,
    oracle_id: int,
    result: dict[str, Any],
) -> str:
    payload = json.dumps(
        result,
        separators=(",", ":"),
        sort_keys=True,
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
        INSERT INTO learning_verifications (
            evidence_id,
            oracle_id,
            verifier,
            verifier_version,
            schema_version,
            verdict,
            reason_code,
            comparison,
            expected_json,
            actual_json,
            result_json
        )
        SELECT
            {int(evidence_id)},
            {int(oracle_id)},
            payload.value
                ->> 'verifier',
            payload.value
                ->> 'verifier_version',
            payload.value
                ->> 'schema_version',
            payload.value
                ->> 'verdict',
            payload.value
                ->> 'reason_code',
            payload.value
                ->> 'comparison',
            payload.value
                -> 'expected',
            payload.value
                -> 'actual',
            payload.value
        FROM payload
        ON CONFLICT (evidence_id)
        DO NOTHING;

        UPDATE learning_evidence le
        SET
            verification_status =
                CASE v.verdict
                    WHEN 'verified'
                        THEN 'verified'
                    ELSE 'rejected'
                END,

            learning_status =
                CASE v.verdict
                    WHEN 'rejected'
                        THEN 'rejected'
                    ELSE le.learning_status
                END,

            updated_at = now()

        FROM learning_verifications v

        WHERE v.evidence_id = le.id
          AND le.id = {int(evidence_id)}
          AND le.verification_status = 'pending';

        COMMIT;

        SELECT
            le.id || '|' ||
            le.learning_status || '|' ||
            le.verification_status
        FROM learning_evidence le
        WHERE le.id = {int(evidence_id)}
          AND le.verification_status
              IN ('verified', 'rejected');
        """
    )

    expected_id = str(
        int(evidence_id)
    )

    for line in reversed(
        output.splitlines()
    ):
        value = line.strip()
        parts = value.split("|")

        if (
            len(parts) == 3
            and parts[0] == expected_id
        ):
            return value

    return ""


def process_one(
    evidence_id: int,
    *,
    dry_run: bool,
) -> None:
    print(
        "AUTO_VERIFY_START "
        f"evidence_id={evidence_id}",
        flush=True,
    )

    case = load_case(
        evidence_id
    )

    request = build_request(
        case
    )

    result = verify(
        request
    )

    if (
        result.get(
            "schema_version"
        )
        != RESULT_SCHEMA
    ):
        raise RuntimeError(
            "unexpected verifier schema"
        )

    verdict = result["verdict"]

    print(
        "AUTO_VERIFY_RESULT "
        f"evidence_id={evidence_id} "
        f"verdict={verdict} "
        f"reason="
        f"{result['reason_code']} "
        f"alk_score="
        f"{request.get('alk_quality_score')}",
        flush=True,
    )

    if dry_run:
        print(
            "AUTO_VERIFY_DRY_RUN "
            f"evidence_id={evidence_id}",
            flush=True,
        )
        return

    state = persist(
        evidence_id=evidence_id,
        oracle_id=int(
            case["oracle_id"]
        ),
        result=result,
    )

    if not state:
        raise RuntimeError(
            "verification persistence "
            "did not transition evidence"
        )

    print(
        "AUTO_VERIFY_PERSISTED "
        f"state={state}",
        flush=True,
    )


def run_batch(
    *,
    evidence_id: int | None,
    limit: int,
    dry_run: bool,
) -> int:
    ids = candidate_ids(
        evidence_id=evidence_id,
        limit=limit,
    )

    if not ids:
        print(
            "AUTO_VERIFY_IDLE "
            "eligible_evidence=0",
            flush=True,
        )
        return 0

    failures = 0

    for item in ids:
        try:
            process_one(
                item,
                dry_run=dry_run,
            )
        except Exception as exc:
            failures += 1

            print(
                "AUTO_VERIFY_ERROR "
                f"evidence_id={item} "
                f"error="
                f"{type(exc).__name__}:"
                f"{exc}",
                flush=True,
            )

    return failures


def selftest() -> None:
    output = {
        "role": "CFO",
        "result": {
            "profit": 30,
            "approved": True,
        },
    }

    assert (
        extract_path(
            output,
            "result.profit",
        )
        == 30
    )

    assert (
        extract_path(
            output,
            "result.approved",
        )
        is True
    )

    request = {
        "schema_version":
            REQUEST_SCHEMA,
        "evidence_id": 1,
        "company_id": 1,
        "alk_quality_score": 0.9999,
        "oracle": {
            "source_type":
                "fixture",
            "source_company_id":
                1,
            "comparison":
                "numeric_exact",
            "expected":
                30,
        },
        "actual": 45,
    }

    verification = verify(
        request
    )

    assert (
        verification["verdict"]
        == "rejected"
    )

    assert (
        verification["reason_code"]
        == "wrong_numeric_answer"
    )

    print(
        "SELFTEST_ACTUAL_PATH=PASS"
    )

    print(
        "SELFTEST_HIGH_ALK_WRONG="
        "REJECTED"
    )

    print(
        "SELFTEST_ASYNC_VERIFIER="
        "PASS"
    )


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

    mode.add_argument(
        "--selftest",
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

    if args.selftest:
        selftest()
        return 0

    if args.interval < 1:
        raise ValueError(
            "interval must be >= 1"
        )

    if args.batch_size < 1:
        raise ValueError(
            "batch-size must be >= 1"
        )

    if args.once:
        failures = run_batch(
            evidence_id=args.evidence_id,
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
        "AUTO_VERIFIER_STARTED "
        f"verifier="
        f"{VERIFIER_NAME} "
        f"version="
        f"{VERIFIER_VERSION} "
        f"interval="
        f"{args.interval}",
        flush=True,
    )

    while _running:
        run_batch(
            evidence_id=None,
            limit=args.batch_size,
            dry_run=args.dry_run,
        )

        if _running:
            time.sleep(
                args.interval
            )

    print(
        "AUTO_VERIFIER_STOPPED",
        flush=True,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
