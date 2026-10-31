import asyncio
import os

import nlp_state
# FLWME_NLP_MEMORY_STATE_V1
from contextlib import asynccontextmanager
from typing import Any

from fastapi import (
    FastAPI,
    HTTPException,
)
from fastembed import TextEmbedding
from pydantic import (
    BaseModel,
    Field,
)
from psycopg import AsyncConnection
from psycopg.rows import dict_row
from qdrant_client import (
    AsyncQdrantClient,
    models,
)


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

COLLECTION = (
    "flwme_company_memory_v1"
)

EMBED_MODEL = (
    "sentence-transformers/"
    "paraphrase-multilingual-MiniLM-L12-v2"
)

EMBED_CACHE = os.getenv(
    "FASTEMBED_CACHE",
    "/home/flwme-dev/.cache/fastembed",
)

VECTOR_SIZE = 384

embedding_model = TextEmbedding(
    model_name=EMBED_MODEL,
    cache_dir=EMBED_CACHE,
)

embedding_limit = asyncio.Semaphore(2)

qdrant = AsyncQdrantClient(
    url=QDRANT_URL,
    timeout=10,
)


class RecallRequest(BaseModel):
    company_id: int = Field(gt=0)
    user_id: int = Field(gt=0)

    query: str = Field(
        min_length=1,
        max_length=16000,
    )

    limit: int = Field(
        default=6,
        ge=1,
        le=8,
    )


async def db() -> AsyncConnection:
    return await AsyncConnection.connect(
        DATABASE_URL,
        row_factory=dict_row,
    )


async def embed(
    text: str,
) -> list[float]:
    async with embedding_limit:
        vector = await asyncio.to_thread(
            lambda: next(
                embedding_model.embed(
                    [text],
                )
            )
        )

    return vector.tolist()


async def ensure_collection() -> None:
    exists = await qdrant.collection_exists(
        COLLECTION,
    )

    if not exists:
        await qdrant.create_collection(
            collection_name=COLLECTION,
            vectors_config=models.VectorParams(
                size=VECTOR_SIZE,
                distance=models.Distance.COSINE,
            ),
        )

    for field in (
        "company_id",
        "user_id",
    ):
        try:
            await qdrant.create_payload_index(
                collection_name=COLLECTION,
                field_name=field,
                field_schema=(
                    models.PayloadSchemaType.INTEGER
                ),
                wait=True,
            )
        except Exception:
            # Index may already exist.
            pass


async def authorize_company(
    company_id: int,
    user_id: int,
) -> bool:
    connection = await db()

    async with connection:
        cursor = await connection.execute(
            """
            SELECT 1
            FROM companies
            WHERE id = %s
              AND owner_user_id = %s
            """,
            (
                company_id,
                user_id,
            ),
        )

        return (
            await cursor.fetchone()
        ) is not None


async def claim_job() -> dict[str, Any] | None:
    connection = await db()

    async with connection:
        async with connection.transaction():
            cursor = await connection.execute(
                """
                SELECT
                    j.id AS job_id,
                    m.id AS memory_id,
                    m.company_id,
                    m.user_id,
                    m.kind,
                    m.content,
                    m.verified
                FROM memory_jobs j
                JOIN company_memory m
                  ON m.id = j.memory_id
                WHERE
                    j.status IN (
                        'pending',
                        'failed'
                    )
                    AND j.attempts < 3
                    AND m.indexed_at IS NULL
                ORDER BY j.id
                FOR UPDATE SKIP LOCKED
                LIMIT 1
                """
            )

            row = await cursor.fetchone()

            if row is None:
                return None

            await connection.execute(
                """
                UPDATE memory_jobs
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


async def mark_done(
    job_id: int,
    memory_id: int,
) -> None:
    connection = await db()

    async with connection:
        async with connection.transaction():
            await connection.execute(
                """
                UPDATE company_memory
                SET
                    indexed_at = now(),
                    updated_at = now()
                WHERE id = %s
                """,
                (
                    memory_id,
                ),
            )

            await connection.execute(
                """
                UPDATE memory_jobs
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


async def mark_failed(
    job_id: int,
    error: Exception,
) -> None:
    connection = await db()

    message = (
        f"{type(error).__name__}: {error}"
    )[:500]

    async with connection:
        await connection.execute(
            """
            UPDATE memory_jobs
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


async def process_job(
    job: dict[str, Any],
) -> None:
    vector = await embed(
        job["content"],
    )

    await qdrant.upsert(
        collection_name=COLLECTION,
        wait=True,
        points=[
            models.PointStruct(
                id=job["memory_id"],
                vector=vector,
                payload={
                    "memory_id":
                        job["memory_id"],

                    "company_id":
                        job["company_id"],

                    "user_id":
                        job["user_id"],

                    "kind":
                        job["kind"],

                    "verified":
                        job["verified"],

                    "content":
                        job["content"],
                },
            )
        ],
    )

    await mark_done(
        job_id=job["job_id"],
        memory_id=job["memory_id"],
    )


async def memory_writer() -> None:
    while True:
        try:
            job = await claim_job()

            if job is None:
                await asyncio.sleep(1)
                continue

            try:
                await process_job(job)

            except asyncio.CancelledError:
                raise

            except Exception as error:
                await mark_failed(
                    job["job_id"],
                    error,
                )

        except asyncio.CancelledError:
            raise

        except Exception:
            await asyncio.sleep(1)


@asynccontextmanager
async def lifespan(
    app: FastAPI,
):
    await ensure_collection()

    nlp_state.configure(
        db=db,
        embed=embed,
        qdrant=qdrant,
        models=models,
    )

    await nlp_state.ensure_state_collection()
    await nlp_state.reset_jobs()

    connection = await db()

    async with connection:
        await connection.execute(
            """
            UPDATE memory_jobs
            SET
                status = 'pending',
                updated_at = now()
            WHERE status = 'running'
            """
        )

    writer = asyncio.create_task(
        memory_writer(),
        name="flwme-memory-writer",
    )

    nlp_workers = (
        nlp_state.start_workers()
    )

    yield

    writer.cancel()

    for task in nlp_workers:
        task.cancel()

    try:
        await writer
    except asyncio.CancelledError:
        pass

    for task in nlp_workers:
        try:
            await task
        except asyncio.CancelledError:
            pass

    await qdrant.close()


app = FastAPI(
    title="FLWME Memory Runtime",
    lifespan=lifespan,
)


@app.get("/healthz")
async def healthz():
    connection = await db()

    async with connection:
        cursor = await connection.execute(
            "SELECT 1 AS ok"
        )

        row = await cursor.fetchone()

        if not row:
            raise HTTPException(
                status_code=503,
                detail="postgres unavailable",
            )

    await qdrant.get_collection(
        COLLECTION,
    )

    await qdrant.get_collection(
        nlp_state.STATE_COLLECTION,
    )

    return {
        "ok": True,
        "collection": COLLECTION,
        "embedding_model": EMBED_MODEL,
    }


@app.post("/memory/recall")
async def recall_memory(
    request: RecallRequest,
) -> dict[str, Any]:
    await authorize_company(
        request.company_id,
        request.user_id,
    )

    return await nlp_state.recall(
        company_id=request.company_id,
        user_id=request.user_id,
        query=request.query,
        limit=request.limit,
    )
