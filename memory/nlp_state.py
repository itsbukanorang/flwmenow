import asyncio
import json
import os
import re
from typing import Any, Callable, Awaitable

import httpx


STATE_COLLECTION = "flwme_company_memory_state_v1"

_DB_FACTORY = None
_EMBED = None
_QDRANT = None
_MODELS = None


def configure(
    *,
    db,
    embed,
    qdrant,
    models,
) -> None:
    global _DB_FACTORY
    global _EMBED
    global _QDRANT
    global _MODELS

    _DB_FACTORY = db
    _EMBED = embed
    _QDRANT = qdrant
    _MODELS = models


def _require_runtime() -> None:
    if (
        _DB_FACTORY is None
        or _EMBED is None
        or _QDRANT is None
        or _MODELS is None
    ):
        raise RuntimeError(
            "NLP memory runtime is not configured"
        )


async def ensure_state_collection() -> None:
    _require_runtime()

    exists = await _QDRANT.collection_exists(
        STATE_COLLECTION
    )

    if not exists:
        await _QDRANT.create_collection(
            collection_name=STATE_COLLECTION,
            vectors_config=_MODELS.VectorParams(
                size=384,
                distance=_MODELS.Distance.COSINE,
            ),
        )

    for field in (
        "company_id",
        "user_id",
    ):
        try:
            await _QDRANT.create_payload_index(
                collection_name=STATE_COLLECTION,
                field_name=field,
                field_schema=(
                    _MODELS.PayloadSchemaType.INTEGER
                ),
                wait=True,
            )
        except Exception:
            # Safe on restarts when the index already exists.
            pass


async def reset_jobs() -> None:
    connection = await _DB_FACTORY()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_ingest_jobs
            SET
                status = 'pending',
                updated_at = now()
            WHERE status = 'running'
            """
        )

        await connection.execute(
            """
            UPDATE memory_state_jobs
            SET
                status = 'pending',
                updated_at = now()
            WHERE status = 'running'
            """
        )


_SECRET_PATTERNS = [
    re.compile(
        r"(?i)\b("
        r"password|passwd|api[_ -]?key|"
        r"access[_ -]?token|refresh[_ -]?token|"
        r"secret|authorization|cookie"
        r")\b\s*[:=]\s*\S+"
    ),
    re.compile(
        r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{12,}"
    ),
]


def contains_sensitive_secret(
    content: str,
) -> bool:
    return any(
        pattern.search(content)
        for pattern in _SECRET_PATTERNS
    )


async def claim_ingest_job():
    connection = await _DB_FACTORY()

    async with connection:
        async with connection.transaction():
            cursor = await connection.execute(
                """
                SELECT
                    id,
                    company_id,
                    user_id,
                    source_message_id,
                    content,
                    attempts
                FROM memory_ingest_jobs
                WHERE status IN (
                    'pending',
                    'failed'
                )
                  AND attempts < 3
                ORDER BY id
                FOR UPDATE SKIP LOCKED
                LIMIT 1
                """
            )

            row = await cursor.fetchone()

            if not row:
                return None

            await connection.execute(
                """
                UPDATE memory_ingest_jobs
                SET
                    status = 'running',
                    attempts = attempts + 1,
                    last_error = NULL,
                    updated_at = now()
                WHERE id = %s
                """,
                (
                    row["id"],
                ),
            )

            return dict(row)


async def mark_ingest_done(
    job_id: int,
) -> None:
    connection = await _DB_FACTORY()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_ingest_jobs
            SET
                status = 'done',
                content = '',
                last_error = NULL,
                updated_at = now()
            WHERE id = %s
            """,
            (
                job_id,
            ),
        )


async def mark_ingest_failed(
    job_id: int,
    error: Exception,
) -> None:
    connection = await _DB_FACTORY()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_ingest_jobs
            SET
                status = 'failed',
                last_error = %s,
                updated_at = now()
            WHERE id = %s
            """,
            (
                str(error)[:1000],
                job_id,
            ),
        )


async def extraction_context(
    job: dict[str, Any],
) -> dict[str, Any]:
    connection = await _DB_FACTORY()

    async with connection:
        state_cursor = await connection.execute(
            """
            SELECT
                memory_key,
                value,
                kind,
                version
            FROM company_memory_state
            WHERE company_id = %s
              AND user_id = %s
            ORDER BY updated_at DESC
            LIMIT 40
            """,
            (
                job["company_id"],
                job["user_id"],
            ),
        )

        state_rows = await state_cursor.fetchall()

        chat_cursor = await connection.execute(
            """
            SELECT
                role,
                content
            FROM (
                SELECT
                    id,
                    role,
                    content
                FROM chat_messages
                WHERE company_id = %s
                  AND user_id = %s
                  AND id <= %s
                ORDER BY id DESC
                LIMIT 10
            ) recent
            ORDER BY id ASC
            """,
            (
                job["company_id"],
                job["user_id"],
                job["source_message_id"],
            ),
        )

        chat_rows = await chat_cursor.fetchall()

    return {
        "current_state": [
            {
                "key": row["memory_key"],
                "value": row["value"],
                "kind": row["kind"],
                "version": row["version"],
            }
            for row in state_rows
        ],
        "recent_conversation": [
            {
                "role": row["role"],
                "content": row["content"],
            }
            for row in chat_rows
        ],
    }


def parse_json_object(
    raw: str,
) -> dict[str, Any]:
    text = raw.strip()

    start = text.find("{")
    end = text.rfind("}")

    if start < 0 or end < start:
        raise ValueError(
            "memory extractor returned no JSON object"
        )

    return json.loads(
        text[
            start:
            end + 1
        ]
    )


def normalize_key(
    value: str,
) -> str:
    key = re.sub(
        r"[^a-z0-9]+",
        "_",
        value.strip().lower(),
    ).strip("_")

    return key[:128]


async def extract_memories(
    job: dict[str, Any],
) -> list[dict[str, Any]]:
    content = str(
        job["content"]
    ).strip()

    if not content:
        return []

    # Credentials / tokens / cookies must never enter
    # organizational memory or Qdrant.
    if contains_sensitive_secret(content):
        return []

    context = await extraction_context(job)

    api_key = os.getenv(
        "MODEL_API_KEY",
        "",
    ).strip()

    if not api_key:
        raise RuntimeError(
            "MODEL_API_KEY unavailable to memory extractor"
        )

    system = """
You are FLWME's asynchronous organizational memory extractor.

Your job is NOT to answer the user.

Read ONLY the current user message as the source of new durable knowledge.
Recent conversation and current_state are context only for resolving references,
slang, typos, implicit updates, and which existing semantic key the user means.

Understand natural language in any language, including Indonesian, Malay,
English, mixed language, abbreviations, informal wording, and slang.

Return JSON only with exactly this shape:

{
  "memories": [
    {
      "key": "stable_snake_case_semantic_key",
      "value": "current durable value",
      "kind": "fact|decision|preference|outcome",
      "confidence": 0.0
    }
  ]
}

Rules:

- If the current message is only a question, request, greeting, command to do work,
  or contains no durable company/user knowledge, return {"memories":[]}.
- Detect updates even when the user does NOT say remember, update, replace,
  obsolete, save, or any special keyword.
- A natural statement that changes an existing fact must reuse the SAME semantic
  key from current_state whenever it refers to the same concept.
- Use conversation context to understand ellipsis such as "ganti deh, pakai X aja".
- Extract the NEW value, not the old value it supersedes.
- Do not create separate keys for historical and current versions of the same fact.
- Do not extract model responses as facts.
- Never extract passwords, API keys, access tokens, cookies, authorization
  headers, private keys, or other credentials.
- Do not infer facts that the user did not state.
- Prefer a small number of high-confidence durable memories.
- confidence must be between 0 and 1.
""".strip()

    payload = {
        "current_user_message": content,
        "current_state": context[
            "current_state"
        ],
        "recent_conversation": context[
            "recent_conversation"
        ],
    }

    async with httpx.AsyncClient(
        timeout=25.0,
    ) as client:
        response = await client.post(
            (
                "https://api.meta.ai/"
                "v1/chat/completions"
            ),
            headers={
                "Authorization":
                    f"Bearer {api_key}",
                "Content-Type":
                    "application/json",
            },
            json={
                "model": "muse-spark-1.3",
                "messages": [
                    {
                        "role": "system",
                        "content": system,
                    },
                    {
                        "role": "user",
                        "content": json.dumps(
                            payload,
                            ensure_ascii=False,
                        ),
                    },
                ],
            },
        )

    response.raise_for_status()

    body = response.json()

    raw = (
        body.get("choices", [{}])[0]
        .get("message", {})
        .get("content", "")
    )

    parsed = parse_json_object(
        str(raw)
    )

    items = parsed.get(
        "memories",
        [],
    )

    if not isinstance(items, list):
        return []

    normalized = []

    for item in items:
        if not isinstance(item, dict):
            continue

        key = normalize_key(
            str(
                item.get(
                    "key",
                    "",
                )
            )
        )

        value = str(
            item.get(
                "value",
                "",
            )
        ).strip()

        kind = str(
            item.get(
                "kind",
                "fact",
            )
        ).strip().lower()

        if kind not in {
            "fact",
            "decision",
            "preference",
            "outcome",
        }:
            kind = "fact"

        try:
            confidence = float(
                item.get(
                    "confidence",
                    1.0,
                )
            )
        except (
            TypeError,
            ValueError,
        ):
            confidence = 1.0

        confidence = max(
            0.0,
            min(
                1.0,
                confidence,
            ),
        )

        if (
            len(key) < 2
            or not value
        ):
            continue

        normalized.append(
            {
                "key": key,
                "value": value[:10000],
                "kind": kind,
                "confidence": confidence,
            }
        )

    return normalized


async def apply_memory(
    job: dict[str, Any],
    item: dict[str, Any],
) -> None:
    connection = await _DB_FACTORY()

    async with connection:
        async with connection.transaction():
            cursor = await connection.execute(
                """
                INSERT INTO company_memory_state (
                    company_id,
                    user_id,
                    memory_key,
                    value,
                    kind,
                    source_message_id,
                    version,
                    confidence
                )
                VALUES (
                    %s,
                    %s,
                    %s,
                    %s,
                    %s,
                    %s,
                    1,
                    %s
                )
                ON CONFLICT (
                    company_id,
                    user_id,
                    memory_key
                )
                DO UPDATE SET
                    value = EXCLUDED.value,
                    kind = EXCLUDED.kind,
                    source_message_id =
                        EXCLUDED.source_message_id,
                    version =
                        company_memory_state.version + 1,
                    confidence =
                        EXCLUDED.confidence,
                    updated_at = now()
                WHERE
                    company_memory_state.source_message_id
                        IS NULL
                    OR
                    company_memory_state.source_message_id
                        < EXCLUDED.source_message_id
                RETURNING
                    id,
                    version,
                    source_message_id
                """,
                (
                    job["company_id"],
                    job["user_id"],
                    item["key"],
                    item["value"],
                    item["kind"],
                    job["source_message_id"],
                    item["confidence"],
                ),
            )

            state = await cursor.fetchone()

            applied = state is not None

            if not applied:
                current_cursor = (
                    await connection.execute(
                        """
                        SELECT
                            id,
                            version,
                            source_message_id
                        FROM company_memory_state
                        WHERE company_id = %s
                          AND user_id = %s
                          AND memory_key = %s
                        """,
                        (
                            job["company_id"],
                            job["user_id"],
                            item["key"],
                        ),
                    )
                )

                current = (
                    await current_cursor.fetchone()
                )

                if (
                    current
                    and current[
                        "source_message_id"
                    ]
                    == job[
                        "source_message_id"
                    ]
                ):
                    state = current
                    applied = True

            await connection.execute(
                """
                INSERT INTO
                    company_memory_state_history (
                        company_id,
                        user_id,
                        memory_key,
                        value,
                        kind,
                        source_message_id,
                        confidence,
                        applied
                    )
                VALUES (
                    %s,
                    %s,
                    %s,
                    %s,
                    %s,
                    %s,
                    %s,
                    %s
                )
                ON CONFLICT (
                    source_message_id,
                    memory_key
                )
                DO UPDATE SET
                    value = EXCLUDED.value,
                    kind = EXCLUDED.kind,
                    confidence =
                        EXCLUDED.confidence,
                    applied = EXCLUDED.applied
                """,
                (
                    job["company_id"],
                    job["user_id"],
                    item["key"],
                    item["value"],
                    item["kind"],
                    job["source_message_id"],
                    item["confidence"],
                    applied,
                ),
            )

            if applied and state:
                await connection.execute(
                    """
                    INSERT INTO memory_state_jobs (
                        state_id,
                        target_version
                    )
                    VALUES (
                        %s,
                        %s
                    )
                    ON CONFLICT (
                        state_id
                    )
                    DO UPDATE SET
                        target_version =
                            EXCLUDED.target_version,
                        status = 'pending',
                        attempts = 0,
                        last_error = NULL,
                        updated_at = now()
                    """,
                    (
                        state["id"],
                        state["version"],
                    ),
                )


async def process_ingest_job(
    job: dict[str, Any],
) -> None:
    memories = await extract_memories(
        job
    )

    for item in memories:
        await apply_memory(
            job,
            item,
        )

    await mark_ingest_done(
        job["id"]
    )


async def ingest_worker(
    worker_number: int,
) -> None:
    while True:
        try:
            job = await claim_ingest_job()

            if job is None:
                await asyncio.sleep(0.25)
                continue

            try:
                await process_ingest_job(job)

            except asyncio.CancelledError:
                raise

            except Exception as error:
                await mark_ingest_failed(
                    job["id"],
                    error,
                )

        except asyncio.CancelledError:
            raise

        except Exception:
            await asyncio.sleep(0.5)


async def claim_state_job():
    connection = await _DB_FACTORY()

    async with connection:
        async with connection.transaction():
            cursor = await connection.execute(
                """
                SELECT
                    j.id AS job_id,
                    j.state_id,
                    j.target_version,
                    s.company_id,
                    s.user_id,
                    s.memory_key,
                    s.value,
                    s.kind,
                    s.version,
                    s.updated_at
                FROM memory_state_jobs j
                JOIN company_memory_state s
                  ON s.id = j.state_id
                WHERE j.status IN (
                    'pending',
                    'failed'
                )
                  AND j.attempts < 3
                ORDER BY j.id
                FOR UPDATE OF j
                    SKIP LOCKED
                LIMIT 1
                """
            )

            row = await cursor.fetchone()

            if not row:
                return None

            await connection.execute(
                """
                UPDATE memory_state_jobs
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


async def mark_state_failed(
    job: dict[str, Any],
    error: Exception,
) -> None:
    connection = await _DB_FACTORY()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_state_jobs
            SET
                status = 'failed',
                last_error = %s,
                updated_at = now()
            WHERE id = %s
              AND target_version = %s
            """,
            (
                str(error)[:1000],
                job["job_id"],
                job["target_version"],
            ),
        )


async def process_state_job(
    job: dict[str, Any],
) -> None:
    if (
        job["version"]
        != job["target_version"]
    ):
        connection = await _DB_FACTORY()

        async with connection:
            await connection.execute(
                """
                UPDATE memory_state_jobs
                SET
                    target_version = %s,
                    status = 'pending',
                    attempts = 0,
                    updated_at = now()
                WHERE id = %s
                """,
                (
                    job["version"],
                    job["job_id"],
                ),
            )

        return

    canonical = (
        f"{job['memory_key']}: "
        f"{job['value']}"
    )

    vector = await _EMBED(
        canonical
    )

    await _QDRANT.upsert(
        collection_name=STATE_COLLECTION,
        wait=True,
        points=[
            _MODELS.PointStruct(
                id=job["state_id"],
                vector=vector,
                payload={
                    "state_id":
                        job["state_id"],
                    "company_id":
                        job["company_id"],
                    "user_id":
                        job["user_id"],
                    "memory_key":
                        job["memory_key"],
                    "value":
                        job["value"],
                    "kind":
                        job["kind"],
                    "version":
                        job["version"],
                    "content":
                        canonical,
                },
            )
        ],
    )

    connection = await _DB_FACTORY()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_state_jobs
            SET
                status = 'done',
                last_error = NULL,
                updated_at = now()
            WHERE id = %s
              AND target_version = %s
            """,
            (
                job["job_id"],
                job["target_version"],
            ),
        )


async def state_writer() -> None:
    # One writer prevents an older version from racing
    # a newer version into the same Qdrant point.
    while True:
        try:
            job = await claim_state_job()

            if job is None:
                await asyncio.sleep(0.25)
                continue

            try:
                await process_state_job(job)

            except asyncio.CancelledError:
                raise

            except Exception as error:
                await mark_state_failed(
                    job,
                    error,
                )

        except asyncio.CancelledError:
            raise

        except Exception:
            await asyncio.sleep(0.5)


def start_workers() -> list[asyncio.Task]:
    tasks = [
        asyncio.create_task(
            ingest_worker(index),
            name=f"flwme-nlp-ingest-{index}",
        )
        for index in range(3)
    ]

    tasks.append(
        asyncio.create_task(
            state_writer(),
            name="flwme-state-writer",
        )
    )

    return tasks


async def recall(
    *,
    company_id: int,
    user_id: int,
    query: str,
    limit: int,
) -> dict[str, Any]:
    """
    Two-layer organizational recall.

    Layer 1:
      Qdrant identifies which semantic memory keys are relevant.

    Layer 2:
      PostgreSQL returns the canonical CURRENT value plus
      the durable historical timeline for those keys.

    We do NOT stuff every company memory into the prompt.
    Large memory remains searchable and only the relevant
    semantic timelines are materialized.
    """

    query_vector = await _EMBED(
        query
    )

    result = await _QDRANT.query_points(
        collection_name=STATE_COLLECTION,
        query=query_vector,
        query_filter=_MODELS.Filter(
            must=[
                _MODELS.FieldCondition(
                    key="company_id",
                    match=_MODELS.MatchValue(
                        value=company_id,
                    ),
                ),
                _MODELS.FieldCondition(
                    key="user_id",
                    match=_MODELS.MatchValue(
                        value=user_id,
                    ),
                ),
            ],
        ),
        limit=max(
            limit * 4,
            16,
        ),
        with_payload=True,
    )

    candidates: list[
        dict[str, Any]
    ] = []

    for point in result.points:
        payload = point.payload or {}

        memory_key = str(
            payload.get(
                "memory_key",
                "",
            )
        ).strip()

        current_value = str(
            payload.get(
                "value",
                "",
            )
        ).strip()

        if (
            not memory_key
            or not current_value
        ):
            continue

        candidates.append(
            {
                "state_id": int(
                    payload.get(
                        "state_id",
                        0,
                    )
                ),
                "memory_key":
                    memory_key,
                "current_value":
                    current_value,
                "kind": str(
                    payload.get(
                        "kind",
                        "fact",
                    )
                ),
                "score": float(
                    point.score
                ),
            }
        )

    # Qdrant may briefly lag PostgreSQL immediately after
    # asynchronous NLP state updates.
    #
    # Add very recent canonical state as read-after-write
    # candidates so fresh memory is immediately available.
    connection = await _DB_FACTORY()

    async with connection:
        cursor = await connection.execute(
            """
            SELECT
                id,
                memory_key,
                value,
                kind
            FROM company_memory_state
            WHERE company_id = %s
              AND user_id = %s
              AND updated_at >=
                    now() - INTERVAL '30 seconds'
            ORDER BY updated_at DESC
            LIMIT 8
            """,
            (
                company_id,
                user_id,
            ),
        )

        recent_states = (
            await cursor.fetchall()
        )

    by_key = {
        item["memory_key"]: item
        for item in candidates
    }

    for row in recent_states:
        key = row["memory_key"]

        if key in by_key:
            # PostgreSQL is canonical.
            by_key[key][
                "current_value"
            ] = row["value"]
            continue

        # Recent state not yet in Qdrant.
        by_key[key] = {
            "state_id":
                row["id"],
            "memory_key":
                key,
            "current_value":
                row["value"],
            "kind":
                row["kind"],
            # Lower than semantic candidates,
            # but available if needed.
            "score":
                -1.0,
        }

    candidates = list(
        by_key.values()
    )

    if not candidates:
        return {
            "memories": [],
        }

    # Keep only semantic keys near the strongest match.
    #
    # This prevents an unrelated key such as launch_codename
    # from contaminating a project_codename history query.
    semantic = [
        item
        for item in candidates
        if item["score"] >= 0.0
    ]

    if semantic:
        best_score = max(
            item["score"]
            for item in semantic
        )

        selected = [
            item
            for item in semantic
            if item["score"]
            >= best_score - 0.12
        ]

    else:
        selected = candidates

    selected.sort(
        key=lambda item:
            item["score"],
        reverse=True,
    )

    # Usually one semantic key is enough.
    # Allow a few when the user's question genuinely spans
    # several related organizational facts.
    selected = selected[
        : max(
            1,
            min(
                limit,
                4,
            ),
        )
    ]

    memories: list[
        dict[str, Any]
    ] = []

    connection = await _DB_FACTORY()

    async with connection:
        for item in selected:
            history_cursor = (
                await connection.execute(
                    """
                    SELECT
                        value,
                        kind,
                        source_message_id,
                        created_at
                    FROM company_memory_state_history
                    WHERE company_id = %s
                      AND user_id = %s
                      AND memory_key = %s
                    ORDER BY
                        source_message_id
                            ASC NULLS FIRST,
                        id ASC
                    LIMIT 500
                    """,
                    (
                        company_id,
                        user_id,
                        item[
                            "memory_key"
                        ],
                    ),
                )
            )

            history_rows = (
                await history_cursor.fetchall()
            )

            timeline: list[str] = []

            # Preserve history while avoiding duplicated
            # consecutive extraction of exactly the same state.
            for row in history_rows:
                value = str(
                    row["value"]
                ).strip()

                if not value:
                    continue

                if (
                    timeline
                    and timeline[-1]
                    == value
                ):
                    continue

                timeline.append(
                    value
                )

            current = item[
                "current_value"
            ]

            # Ensure the canonical current value is represented
            # at the end even if Qdrant/history indexing lags.
            if (
                not timeline
                or timeline[-1]
                != current
            ):
                timeline.append(
                    current
                )

            previous = (
                timeline[:-1]
                if timeline
                else []
            )

            if previous:
                previous_text = (
                    " -> ".join(
                        previous
                    )
                )
            else:
                previous_text = (
                    "(none recorded)"
                )

            content = (
                f"MEMORY KEY: "
                f"{item['memory_key']}\n"
                f"CURRENT: "
                f"{current}\n"
                f"PREVIOUS VALUES "
                f"(oldest -> newest): "
                f"{previous_text}\n"
                f"FULL TIMELINE "
                f"(oldest -> newest): "
                + " -> ".join(
                    timeline
                )
            )

            memories.append(
                {
                    "kind":
                        item["kind"],
                    "content":
                        content,
                    "score":
                        item["score"],
                }
            )

    return {
        "memories":
            memories[
                : limit
            ],
    }

