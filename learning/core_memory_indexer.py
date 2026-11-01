#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import signal
import time
from typing import Any

import psycopg
from psycopg.rows import dict_row
from qdrant_client import QdrantClient, models


DATABASE_URL = os.getenv(
    "DATABASE_URL",
    (
        "host=/var/run/postgresql "
        "dbname=flwmenow "
        "user=flwme-dev"
    ),
)

QDRANT_URL = os.getenv(
    "QDRANT_URL",
    "http://127.0.0.1:6333",
)

COLLECTION = "flwme_core_learning_v1"

EMBED_MODEL = (
    "sentence-transformers/"
    "paraphrase-multilingual-MiniLM-L12-v2"
)

EMBED_CACHE = os.getenv(
    "FASTEMBED_CACHE",
    "/home/flwme-dev/.cache/fastembed",
)

VECTOR_SIZE = 384

SANITIZATION_VERSION = (
    "flwme.core.sanitize.v1"
)

_running = True
_embedding_model = None
_qdrant = None


CLAIM_SQL = """
SELECT
    j.id AS job_id,
    cm.id AS core_memory_id,
    cm.behavior_key,
    cm.category,
    cm.agent_role,
    cm.behavior_text,
    cm.behavior_metrics,
    cm.quality_score,
    cm.ethics_status,
    cm.sanitization_version,
    cm.policy_version
FROM flwme_core_learning_jobs j
JOIN flwme_core_learning_memory cm
  ON cm.id = j.core_memory_id
WHERE j.status IN (
        'pending',
        'failed'
      )
  AND j.attempts < 3
  AND cm.index_status IN (
        'pending',
        'failed'
      )
  AND cm.ethics_status = 'approved'
  AND cm.sanitization_version =
      'flwme.core.sanitize.v1'
ORDER BY j.id
FOR UPDATE SKIP LOCKED
LIMIT 1
"""


FORBIDDEN_PAYLOAD_KEYS = {
    "company_id",
    "user_id",
    "source_candidate_id",
    "evidence_id",
    "task_id",
    "run_id",
    "objective",
    "output_json",
    "evidence_json",
    "source_message_id",
    "customer",
    "email",
}


def stop_handler(
    signum: int,
    frame: Any,
) -> None:
    del signum
    del frame

    global _running
    _running = False


def db() -> psycopg.Connection:
    return psycopg.connect(
        DATABASE_URL,
        row_factory=dict_row,
    )


def qdrant() -> QdrantClient:
    global _qdrant

    if _qdrant is None:
        _qdrant = QdrantClient(
            url=QDRANT_URL,
            timeout=10,
        )

    return _qdrant


def embedding_model() -> Any:
    global _embedding_model

    if _embedding_model is None:
        from fastembed import TextEmbedding

        _embedding_model = TextEmbedding(
            model_name=EMBED_MODEL,
            cache_dir=EMBED_CACHE,
        )

    return _embedding_model


def embed(text: str) -> list[float]:
    vector = next(
        embedding_model().embed(
            [text],
        )
    )

    return vector.tolist()


def ensure_collection() -> None:
    client = qdrant()

    if not client.collection_exists(
        COLLECTION,
    ):
        client.create_collection(
            collection_name=COLLECTION,
            vectors_config=models.VectorParams(
                size=VECTOR_SIZE,
                distance=models.Distance.COSINE,
            ),
        )

    for field in (
        "category",
        "agent_role",
    ):
        try:
            client.create_payload_index(
                collection_name=COLLECTION,
                field_name=field,
                field_schema=(
                    models.PayloadSchemaType.KEYWORD
                ),
                wait=True,
            )
        except Exception:
            # Index may already exist.
            pass


def build_payload(
    row: dict[str, Any],
) -> dict[str, Any]:

    payload = {
        "core_memory_id":
            int(row["core_memory_id"]),
        "behavior_key":
            str(row["behavior_key"]),
        "category":
            str(row["category"]),
        "agent_role":
            str(row["agent_role"]),
        "behavior_text":
            str(row["behavior_text"]),
        "behavior_metrics":
            dict(
                row.get(
                    "behavior_metrics",
                    {},
                )
            ),
        "quality_score":
            float(row["quality_score"]),
        "ethics_status":
            str(row["ethics_status"]),
        "sanitization_version":
            str(
                row["sanitization_version"]
            ),
        "policy_version":
            str(row["policy_version"]),
    }

    forbidden = (
        set(payload)
        & FORBIDDEN_PAYLOAD_KEYS
    )

    if forbidden:
        raise RuntimeError(
            "forbidden_core_payload_keys:"
            + ",".join(
                sorted(forbidden)
            )
        )

    if (
        payload["ethics_status"]
        != "approved"
    ):
        raise RuntimeError(
            "core_ethics_not_approved"
        )

    if (
        payload["sanitization_version"]
        != SANITIZATION_VERSION
    ):
        raise RuntimeError(
            "core_sanitization_version_invalid"
        )

    return payload


def embedding_text(
    payload: dict[str, Any],
) -> str:
    return (
        f"Role: {payload['agent_role']}\n"
        f"Category: {payload['category']}\n"
        f"Behavior: {payload['behavior_text']}"
    )


def reset_running_jobs() -> int:
    with db() as connection:
        with connection.cursor() as cursor:
            cursor.execute(
                """
                UPDATE flwme_core_learning_jobs
                SET
                    status = 'failed',
                    last_error =
                        'worker_restart_recovery',
                    updated_at = now()
                WHERE status = 'running'
                """
            )

            changed = cursor.rowcount

        connection.commit()

    return changed


def claim_job() -> dict[str, Any] | None:
    with db() as connection:
        with connection.transaction():
            with connection.cursor() as cursor:
                cursor.execute(
                    CLAIM_SQL
                )

                row = cursor.fetchone()

                if row is None:
                    return None

                cursor.execute(
                    """
                    UPDATE flwme_core_learning_jobs
                    SET
                        status = 'running',
                        attempts = attempts + 1,
                        last_error = NULL,
                        updated_at = now()
                    WHERE id = %s
                    """,
                    (
                        row["job_id"],
                    ),
                )

                return dict(row)


def mark_done(
    *,
    job_id: int,
    core_memory_id: int,
) -> None:
    with db() as connection:
        with connection.transaction():
            with connection.cursor() as cursor:
                cursor.execute(
                    """
                    UPDATE flwme_core_learning_memory
                    SET
                        index_status = 'indexed',
                        indexed_at = now(),
                        updated_at = now()
                    WHERE id = %s
                    """,
                    (
                        core_memory_id,
                    ),
                )

                cursor.execute(
                    """
                    UPDATE flwme_core_learning_jobs
                    SET
                        status = 'done',
                        last_error = NULL,
                        updated_at = now()
                    WHERE id = %s
                    """,
                    (
                        job_id,
                    ),
                )


def mark_failed(
    *,
    job_id: int,
    core_memory_id: int,
    error: Exception,
) -> None:
    message = (
        f"{type(error).__name__}: {error}"
    )[:500]

    with db() as connection:
        with connection.transaction():
            with connection.cursor() as cursor:
                cursor.execute(
                    """
                    UPDATE flwme_core_learning_memory
                    SET
                        index_status = 'failed',
                        updated_at = now()
                    WHERE id = %s
                    """,
                    (
                        core_memory_id,
                    ),
                )

                cursor.execute(
                    """
                    UPDATE flwme_core_learning_jobs
                    SET
                        status = 'failed',
                        last_error = %s,
                        updated_at = now()
                    WHERE id = %s
                    """,
                    (
                        message,
                        job_id,
                    ),
                )


def process_job(
    row: dict[str, Any],
) -> None:

    payload = build_payload(
        row
    )

    text = embedding_text(
        payload
    )

    vector = embed(
        text
    )

    qdrant().upsert(
        collection_name=COLLECTION,
        wait=True,
        points=[
            models.PointStruct(
                id=payload[
                    "core_memory_id"
                ],
                vector=vector,
                payload=payload,
            )
        ],
    )

    mark_done(
        job_id=int(
            row["job_id"]
        ),
        core_memory_id=int(
            row["core_memory_id"]
        ),
    )


def run_batch(
    batch_size: int,
) -> int:

    processed = 0

    for _ in range(
        max(1, batch_size)
    ):
        row = claim_job()

        if row is None:
            break

        try:
            process_job(
                row
            )
        except Exception as error:
            mark_failed(
                job_id=int(
                    row["job_id"]
                ),
                core_memory_id=int(
                    row["core_memory_id"]
                ),
                error=error,
            )

            print(
                "CORE_INDEX_FAILED "
                f"core_memory_id="
                f"{row['core_memory_id']} "
                f"error={type(error).__name__}",
                flush=True,
            )

            continue

        processed += 1

        print(
            "CORE_INDEXED "
            f"core_memory_id="
            f"{row['core_memory_id']} "
            f"behavior_key="
            f"{row['behavior_key']}",
            flush=True,
        )

    return processed


def contract_payload() -> dict[str, Any]:
    return build_payload(
        {
            "core_memory_id": 42,
            "behavior_key":
                "cto:action_safety"
                "+reasoning_quality",
            "category": "safety",
            "agent_role": "CTO",
            "behavior_text":
                (
                    "Prefer safe reversible "
                    "actions and verify evidence "
                    "before committing to a "
                    "material operational decision."
                ),
            "behavior_metrics": {
                "action_safety": 0.98,
                "reasoning_quality": 0.96,
            },
            "quality_score": 0.97,
            "ethics_status": "approved",
            "sanitization_version":
                SANITIZATION_VERSION,
            "policy_version":
                "flwme-core-policy-v1",

            # These simulate data present in
            # an upstream record. build_payload
            # MUST ignore all of them.
            "company_id": 999,
            "user_id": 888,
            "objective":
                "PRIVATE OBJECTIVE",
            "output_json": {
                "secret":
                    "TENANT-SECRET-XYZ"
            },
            "customer":
                "PRIVATE CUSTOMER",
        }
    )


def selftest() -> None:
    payload = contract_payload()

    serialized = json.dumps(
        payload,
        sort_keys=True,
    )

    for forbidden in (
        "company_id",
        "user_id",
        "source_candidate_id",
        "evidence_id",
        "task_id",
        "run_id",
        "objective",
        "output_json",
        "evidence_json",
        "source_message_id",
        "customer",
        "PRIVATE OBJECTIVE",
        "TENANT-SECRET-XYZ",
        "PRIVATE CUSTOMER",
    ):
        assert (
            forbidden
            not in serialized
        )

    assert (
        payload[
            "ethics_status"
        ]
        == "approved"
    )

    assert (
        payload[
            "sanitization_version"
        ]
        == SANITIZATION_VERSION
    )

    assert (
        COLLECTION
        == "flwme_core_learning_v1"
    )

    lower_claim = (
        CLAIM_SQL.lower()
    )

    assert (
        "flwme_core_learning_jobs"
        in lower_claim
    )

    assert (
        "flwme_core_learning_memory"
        in lower_claim
    )

    for private_table in (
        "company_memory ",
        "company_memory_state",
        "memory_ingest_jobs",
        "memory_jobs ",
    ):
        assert (
            private_table
            not in lower_claim
        )

    print(
        "SELFTEST_CORE_PAYLOAD_ALLOWLIST=PASS"
    )
    print(
        "SELFTEST_TENANT_IDENTIFIERS_REMOVED=PASS"
    )
    print(
        "SELFTEST_RAW_TENANT_CONTENT_REMOVED=PASS"
    )
    print(
        "SELFTEST_PRIVATE_MEMORY_TABLES=0"
    )
    print(
        "SELFTEST_CORE_COLLECTION="
        + COLLECTION
    )
    print(
        "CORE_INDEXER_SELFTEST=PASS"
    )


def main() -> None:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--selftest",
        action="store_true",
    )

    parser.add_argument(
        "--emit-payload-contract",
        action="store_true",
    )

    parser.add_argument(
        "--once",
        action="store_true",
    )

    parser.add_argument(
        "--loop",
        action="store_true",
    )

    parser.add_argument(
        "--interval",
        type=float,
        default=2.0,
    )

    parser.add_argument(
        "--batch-size",
        type=int,
        default=10,
    )

    args = parser.parse_args()

    if args.selftest:
        selftest()
        return

    if args.emit_payload_contract:
        print(
            json.dumps(
                contract_payload(),
                indent=2,
                sort_keys=True,
            )
        )
        return

    if not (
        args.once
        or args.loop
    ):
        parser.error(
            "use --selftest, "
            "--emit-payload-contract, "
            "--once, or --loop"
        )

    signal.signal(
        signal.SIGTERM,
        stop_handler,
    )

    signal.signal(
        signal.SIGINT,
        stop_handler,
    )

    ensure_collection()

    recovered = reset_running_jobs()

    print(
        "CORE_INDEXER_STARTED "
        f"collection={COLLECTION} "
        f"recovered_jobs={recovered}",
        flush=True,
    )

    if args.once:
        processed = run_batch(
            args.batch_size
        )

        print(
            "CORE_INDEXER_ONCE_DONE "
            f"processed={processed}",
            flush=True,
        )

        return

    while _running:
        processed = run_batch(
            args.batch_size
        )

        if processed == 0:
            print(
                "CORE_INDEXER_IDLE",
                flush=True,
            )

        if _running:
            time.sleep(
                max(
                    0.5,
                    args.interval,
                )
            )


if __name__ == "__main__":
    main()
