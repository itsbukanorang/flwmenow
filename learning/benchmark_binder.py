#!/usr/bin/env python3

import argparse
import os
import re
import shutil
import signal
import subprocess
import time
from typing import Any


DEFAULT_INTERVAL = 2.0
DEFAULT_BATCH_SIZE = 20

BENCHMARK_RE = re.compile(
    r"Verification\s+benchmark\s*:\s*"
    r"([A-Z0-9][A-Z0-9_-]{7,127})",
    re.IGNORECASE,
)

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


def benchmark_key(
    objective: str,
) -> str | None:
    match = BENCHMARK_RE.search(
        objective or ""
    )

    if match is None:
        return None

    return match.group(1).upper()


def candidate_rows(
    limit: int,
) -> list[tuple[int, str]]:
    raw = psql(
        f"""
        SELECT
            le.id,
            encode(
                convert_to(
                    le.objective,
                    'UTF8'
                ),
                'hex'
            )
        FROM learning_evidence le
        LEFT JOIN learning_verification_oracles o
          ON o.company_id = le.company_id
         AND o.task_id = le.task_id
         AND o.active = TRUE
        WHERE
            le.learning_status = 'evaluated'
            AND le.verification_status = 'pending'
            AND o.id IS NULL
            AND le.objective ILIKE
                '%Verification benchmark:%'
        ORDER BY le.created_at ASC
        LIMIT {int(limit)};
        """
    )

    if not raw:
        return []

    import base64

    rows: list[tuple[int, str]] = []

    for line in raw.splitlines():
        left, right = line.split(
            "|",
            1,
        )

        objective = bytes.fromhex(
            right
        ).decode("utf-8")

        rows.append(
            (
                int(left),
                objective,
            )
        )

    return rows


def bind(
    evidence_id: int,
    key: str,
) -> str:
    if not re.fullmatch(
        r"[A-Z0-9][A-Z0-9_-]{7,127}",
        key,
    ):
        raise ValueError(
            "invalid benchmark key"
        )

    return psql(
        f"""
        INSERT INTO learning_verification_oracles (
            company_id,
            task_id,
            source_type,
            comparison,
            expected_json,
            tolerance,
            actual_path,
            active,
            benchmark_id
        )
        SELECT
            le.company_id,
            le.task_id,
            b.source_type,
            b.comparison,
            b.expected_json,
            b.tolerance,
            b.actual_path,
            TRUE,
            b.id
        FROM learning_evidence le
        JOIN learning_verification_benchmarks b
          ON b.company_id = le.company_id
         AND b.benchmark_key = '{key}'
         AND b.active = TRUE
        WHERE le.id = {int(evidence_id)}
          AND le.learning_status = 'evaluated'
          AND le.verification_status = 'pending'
        ON CONFLICT DO NOTHING
        RETURNING
            id || '|' ||
            task_id || '|' ||
            benchmark_id;
        """
    )


def run_batch(
    limit: int,
) -> int:
    rows = candidate_rows(
        limit
    )

    if not rows:
        print(
            "BENCHMARK_BIND_IDLE "
            "eligible=0",
            flush=True,
        )

        return 0

    failures = 0

    for evidence_id, objective in rows:
        key = benchmark_key(
            objective
        )

        if key is None:
            continue

        print(
            "BENCHMARK_BIND_START "
            f"evidence_id={evidence_id} "
            f"key={key}",
            flush=True,
        )

        try:
            state = bind(
                evidence_id,
                key,
            )

            if state:
                print(
                    "BENCHMARK_BIND_PERSISTED "
                    f"state={state} "
                    f"key={key}",
                    flush=True,
                )
            else:
                print(
                    "BENCHMARK_BIND_NOOP "
                    f"evidence_id={evidence_id} "
                    f"key={key}",
                    flush=True,
                )

        except Exception as exc:
            failures += 1

            print(
                "BENCHMARK_BIND_ERROR "
                f"evidence_id={evidence_id} "
                f"key={key} "
                f"error="
                f"{type(exc).__name__}:"
                f"{exc}",
                flush=True,
            )

    return failures


def selftest() -> None:
    cases = {
        (
            "Verification benchmark: "
            "ALK02C-CORRECT-001"
        ):
            "ALK02C-CORRECT-001",

        (
            "Please test.\n"
            "Verification benchmark: "
            "ALK02C-WRONG-001\n"
            "Delegate this."
        ):
            "ALK02C-WRONG-001",

        (
            "verification benchmark:"
            " alk02c-case-123"
        ):
            "ALK02C-CASE-123",

        "No benchmark here":
            None,
    }

    for text, expected in cases.items():
        actual = benchmark_key(
            text
        )

        assert actual == expected, (
            text,
            actual,
            expected,
        )

    print(
        "BENCHMARK_KEY_PARSER=PASS"
    )

    print(
        "BENCHMARK_BINDER_SELFTEST=PASS"
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
        return (
            1
            if run_batch(
                args.batch_size
            )
            else 0
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
        "BENCHMARK_BINDER_STARTED "
        f"interval={args.interval}",
        flush=True,
    )

    while _running:
        run_batch(
            args.batch_size
        )

        if _running:
            time.sleep(
                args.interval
            )

    print(
        "BENCHMARK_BINDER_STOPPED",
        flush=True,
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
