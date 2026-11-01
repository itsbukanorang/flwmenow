#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import shutil
import signal
import subprocess
import time
from typing import Any

from promotion_gate import (
    POLICY_VERSION,
    decide,
    load_evidence,
)


_running = True

_ALLOWED_DECISIONS = {
    "approved",
    "blocked",
}

_ALLOWED_REASONS = {
    "alk_evaluation_not_passed",
    "alk_score_below_threshold",
    "truth_source_not_promotable",
    "unsupported_source_event",
    "promotion_policy_passed",
}


def stop_handler(
    *_: Any,
) -> None:
    global _running
    _running = False


def psql(
    sql: str,
) -> str:
    binary = shutil.which(
        "psql"
    )

    if not binary:
        raise RuntimeError(
            "psql not found"
        )

    database = os.environ.get(
        "FLWME_DB_NAME",
        "flwmenow",
    )

    result = subprocess.run(
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

    return result.stdout.strip()


def sql_text(
    value: str,
) -> str:
    return (
        "'"
        + value.replace(
            "'",
            "''",
        )
        + "'"
    )


def candidate_ids(
    batch_size: int,
) -> list[int]:
    if (
        batch_size < 1
        or batch_size > 100
    ):
        raise ValueError(
            "invalid batch size"
        )

    raw = psql(
        f"""
        SELECT le.id
        FROM learning_evidence le
        WHERE le.learning_status =
              'evaluated'
          AND le.verification_status =
              'verified'
        ORDER BY le.id
        LIMIT {int(batch_size)};
        """
    )

    if not raw:
        return []

    result: list[int] = []

    for line in raw.splitlines():
        line = line.strip()

        if not line:
            continue

        result.append(
            int(line)
        )

    return result


def build_persist_sql(
    record: dict[str, Any],
    decision: dict[str, Any],
) -> str:
    if decision.get("eligible") is not True:
        raise ValueError(
            "evidence is not candidate eligible"
        )

    status = str(
        decision.get(
            "decision"
        )
    )

    reason = str(
        decision.get(
            "reason_code"
        )
    )

    if status not in _ALLOWED_DECISIONS:
        raise ValueError(
            "invalid promotion decision"
        )

    if reason not in _ALLOWED_REASONS:
        raise ValueError(
            "invalid promotion reason"
        )

    evidence_id = int(
        record["evidence_id"]
    )

    company_id = int(
        record["company_id"]
    )

    evaluation_id = int(
        record["evaluation_id"]
    )

    verification_id = int(
        record["verification_id"]
    )

    policy_sql = sql_text(
        POLICY_VERSION
    )

    status_sql = sql_text(
        status
    )

    reason_sql = sql_text(
        reason
    )

    return f"""
INSERT INTO learning_promotion_candidates (
    evidence_id,
    company_id,
    evaluation_id,
    verification_id,
    policy_version,
    status,
    reason_code,
    evaluation_score,
    evaluation_threshold,
    oracle_source_type,
    decided_at
)
SELECT
    le.id,
    le.company_id,
    ev.id,
    v.id,
    {policy_sql},
    {status_sql},
    {reason_sql},
    ev.score,
    ev.threshold,
    o.source_type,
    now()
FROM learning_evidence le
JOIN learning_evaluations ev
  ON ev.id = {evaluation_id}
 AND ev.evidence_id = le.id
JOIN learning_verifications v
  ON v.id = {verification_id}
 AND v.evidence_id = le.id
JOIN learning_verification_oracles o
  ON o.id = v.oracle_id
WHERE le.id = {evidence_id}
  AND le.company_id = {company_id}
  AND le.learning_status = 'evaluated'
  AND le.verification_status = 'verified'
  AND v.verdict = 'verified'
ON CONFLICT (evidence_id)
DO NOTHING;

UPDATE learning_evidence le
SET learning_status = 'candidate'
WHERE le.id = {evidence_id}
  AND le.company_id = {company_id}
  AND le.learning_status = 'evaluated'
  AND le.verification_status = 'verified'
  AND EXISTS (
      SELECT 1
      FROM learning_promotion_candidates pc
      WHERE pc.evidence_id = le.id
        AND pc.company_id = le.company_id
        AND pc.status IN (
            'approved',
            'blocked'
        )
  );
""".strip()


def persist(
    record: dict[str, Any],
    decision: dict[str, Any],
) -> str:
    evidence_id = int(
        record["evidence_id"]
    )

    sql = build_persist_sql(
        record,
        decision,
    )

    return psql(
        f"""
        BEGIN;

        {sql}

        COMMIT;

        SELECT
            le.id || '|' ||
            le.learning_status || '|' ||
            le.verification_status || '|' ||
            pc.status || '|' ||
            pc.reason_code
        FROM learning_evidence le
        JOIN learning_promotion_candidates pc
          ON pc.evidence_id = le.id
        WHERE le.id = {evidence_id};
        """
    )


def process_one(
    evidence_id: int,
    *,
    dry_run: bool,
) -> bool:
    record = load_evidence(
        evidence_id
    )

    decision = decide(
        record
    )

    if decision.get(
        "eligible"
    ) is not True:
        print(
            "PROMOTION_SKIP "
            f"evidence_id={evidence_id} "
            f"reason={decision.get('reason_code')}",
            flush=True,
        )

        return False

    print(
        "PROMOTION_DECISION "
        f"evidence_id={evidence_id} "
        f"decision={decision['decision']} "
        f"reason={decision['reason_code']} "
        f"score={record['evaluation_score']} "
        f"threshold={record['evaluation_threshold']} "
        f"oracle={record['oracle_source_type']}",
        flush=True,
    )

    if dry_run:
        return True

    state = persist(
        record,
        decision,
    )

    if not state:
        raise RuntimeError(
            "promotion persistence returned "
            "empty state"
        )

    print(
        "PROMOTION_PERSISTED "
        f"state={state}",
        flush=True,
    )

    return True


def run_batch(
    *,
    batch_size: int,
    dry_run: bool,
) -> int:
    ids = candidate_ids(
        batch_size
    )

    if not ids:
        print(
            "PROMOTION_IDLE "
            "eligible_evidence=0",
            flush=True,
        )

        return 0

    failures = 0

    for evidence_id in ids:
        try:
            process_one(
                evidence_id,
                dry_run=dry_run,
            )
        except Exception as exc:
            failures += 1

            print(
                "PROMOTION_ERROR "
                f"evidence_id={evidence_id} "
                f"error={type(exc).__name__}",
                flush=True,
            )

    return failures


def selftest() -> None:
    fixture_record = {
        "evidence_id": 1001,
        "company_id": 1,
        "evaluation_id": 2001,
        "verification_id": 3001,
        "learning_status":
            "evaluated",
        "verification_status":
            "verified",
        "verification_verdict":
            "verified",
        "evaluation_status":
            "passed",
        "evaluation_score":
            0.95,
        "evaluation_threshold":
            0.70,
        "oracle_source_type":
            "fixture",
        "source_event":
            "executive_task_completion",
    }

    fixture_decision = decide(
        fixture_record
    )

    assert (
        fixture_decision["decision"]
        == "blocked"
    )

    sql = build_persist_sql(
        fixture_record,
        fixture_decision,
    )

    assert (
        "ON CONFLICT (evidence_id)"
        in sql
    )

    assert (
        "learning_status = 'candidate'"
        in sql
    )

    assert (
        "verification_status = 'verified'"
        in sql
    )

    assert (
        "v.verdict = 'verified'"
        in sql
    )

    assert (
        "company_memory"
        not in sql
    )

    assert (
        "memory_jobs"
        not in sql
    )

    assert (
        "qdrant"
        not in sql.lower()
    )

    trusted_record = {
        **fixture_record,
        "oracle_source_type":
            "database_verified",
    }

    trusted_decision = decide(
        trusted_record
    )

    assert (
        trusted_decision["decision"]
        == "approved"
    )

    rejected_record = {
        **trusted_record,
        "learning_status":
            "rejected",
        "verification_status":
            "rejected",
        "verification_verdict":
            "rejected",
    }

    rejected_decision = decide(
        rejected_record
    )

    assert (
        rejected_decision["eligible"]
        is False
    )

    print(
        "SELFTEST_FIXTURE="
        "BLOCKED"
    )

    print(
        "SELFTEST_DATABASE_VERIFIED="
        "APPROVED"
    )

    print(
        "SELFTEST_REJECTED="
        "NOT_CANDIDATE"
    )

    print(
        "SELFTEST_IDEMPOTENCY_GUARD="
        "ON_CONFLICT"
    )

    print(
        "SELFTEST_CORE_MEMORY_WRITE=0"
    )

    print(
        "ASYNC_PROMOTION_SELFTEST=PASS"
    )


def emit_persist_sql(
    evidence_id: int,
) -> int:
    record = load_evidence(
        evidence_id
    )

    decision = decide(
        record
    )

    if decision.get(
        "eligible"
    ) is not True:
        print(
            "EMIT_DENIED="
            f"{decision.get('reason_code')}"
        )

        return 3

    print(
        build_persist_sql(
            record,
            decision,
        )
    )

    return 0


def main() -> int:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--loop",
        action="store_true",
    )

    parser.add_argument(
        "--once",
        action="store_true",
    )

    parser.add_argument(
        "--dry-run",
        action="store_true",
    )

    parser.add_argument(
        "--selftest",
        action="store_true",
    )

    parser.add_argument(
        "--emit-persist-sql",
        type=int,
    )

    parser.add_argument(
        "--interval",
        type=float,
        default=5.0,
    )

    parser.add_argument(
        "--batch-size",
        type=int,
        default=10,
    )

    args = parser.parse_args()

    if args.selftest:
        selftest()
        return 0

    if args.emit_persist_sql:
        return emit_persist_sql(
            args.emit_persist_sql
        )

    if args.once:
        failures = run_batch(
            batch_size=args.batch_size,
            dry_run=args.dry_run,
        )

        return 1 if failures else 0

    if args.loop:
        if args.interval <= 0:
            raise ValueError(
                "interval must be positive"
            )

        signal.signal(
            signal.SIGTERM,
            stop_handler,
        )

        signal.signal(
            signal.SIGINT,
            stop_handler,
        )

        print(
            "PROMOTION_WORKER_STARTED "
            f"policy={POLICY_VERSION} "
            f"interval={args.interval}",
            flush=True,
        )

        while _running:
            run_batch(
                batch_size=args.batch_size,
                dry_run=args.dry_run,
            )

            if _running:
                time.sleep(
                    args.interval
                )

        return 0

    parser.error(
        "--selftest, --once, --loop, "
        "or --emit-persist-sql required"
    )


if __name__ == "__main__":
    raise SystemExit(main())
