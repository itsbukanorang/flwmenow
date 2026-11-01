#!/usr/bin/env python3

from __future__ import annotations

import argparse
import asyncio
import json
import os
from contextlib import asynccontextmanager
from typing import Any

from fastapi import FastAPI
from fastembed import TextEmbedding
from pydantic import (
    BaseModel,
    ConfigDict,
    Field,
)
from qdrant_client import (
    AsyncQdrantClient,
    models,
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

SANITIZATION_VERSION = (
    "flwme.core.sanitize.v1"
)

ALLOWED_ROLES = {
    "CEO",
    "COO",
    "CFO",
    "CMO",
    "CTO",
    "Worker",
}

_embedding_model: TextEmbedding | None = None

embedding_limit = asyncio.Semaphore(2)

qdrant = AsyncQdrantClient(
    url=QDRANT_URL,
    timeout=10,
)


def get_embedding_model() -> TextEmbedding:
    global _embedding_model

    if _embedding_model is None:
        _embedding_model = TextEmbedding(
            model_name=EMBED_MODEL,
            cache_dir=EMBED_CACHE,
        )

    return _embedding_model


class CoreRecallRequest(BaseModel):
    model_config = ConfigDict(
        extra="forbid",
    )

    query: str = Field(
        min_length=1,
        max_length=16000,
    )

    consumer_role: str = Field(
        min_length=2,
        max_length=32,
    )

    limit: int = Field(
        default=4,
        ge=1,
        le=8,
    )


class CoreGuidance(BaseModel):
    behavior_key: str
    category: str
    source_role: str
    behavior_text: str
    quality_score: float
    policy_version: str


class CoreRecallResponse(BaseModel):
    guidance: list[CoreGuidance]
    collection_ready: bool


@asynccontextmanager
async def lifespan(app: FastAPI):
    del app

    yield

    await qdrant.close()


app = FastAPI(
    title="FLWME Core Behavioral Recall",
    lifespan=lifespan,
)


async def embed(
    text: str,
) -> list[float]:

    async with embedding_limit:
        vector = await asyncio.to_thread(
            lambda: next(
                get_embedding_model().embed(
                    [text],
                )
            )
        )

    return vector.tolist()


def safe_guidance(
    payload: dict[str, Any],
) -> CoreGuidance | None:

    if (
        payload.get("ethics_status")
        != "approved"
    ):
        return None

    if (
        payload.get(
            "sanitization_version"
        )
        != SANITIZATION_VERSION
    ):
        return None

    behavior_key = str(
        payload.get(
            "behavior_key",
            "",
        )
    ).strip()

    category = str(
        payload.get(
            "category",
            "",
        )
    ).strip()

    source_role = str(
        payload.get(
            "agent_role",
            "",
        )
    ).strip()

    behavior_text = str(
        payload.get(
            "behavior_text",
            "",
        )
    ).strip()

    policy_version = str(
        payload.get(
            "policy_version",
            "",
        )
    ).strip()

    try:
        quality_score = float(
            payload.get(
                "quality_score",
                0.0,
            )
        )
    except (
        TypeError,
        ValueError,
    ):
        return None

    if not (
        behavior_key
        and category
        and source_role
        and behavior_text
        and policy_version
    ):
        return None

    if not (
        0.0
        <= quality_score
        <= 1.0
    ):
        return None

    return CoreGuidance(
        behavior_key=behavior_key,
        category=category,
        source_role=source_role,
        behavior_text=behavior_text,
        quality_score=quality_score,
        policy_version=policy_version,
    )


@app.get("/healthz")
async def healthz() -> dict[str, Any]:

    ready = await qdrant.collection_exists(
        COLLECTION
    )

    return {
        "ok": True,
        "service":
            "flwme-core-learning-recall",
        "collection":
            COLLECTION,
        "collection_ready":
            ready,
        "embedding_model":
            EMBED_MODEL,
        "tenant_scope":
            "none",
    }


@app.post(
    "/core/recall",
    response_model=CoreRecallResponse,
)
async def core_recall(
    request: CoreRecallRequest,
) -> CoreRecallResponse:

    if (
        request.consumer_role
        not in ALLOWED_ROLES
    ):
        return CoreRecallResponse(
            guidance=[],
            collection_ready=(
                await qdrant.collection_exists(
                    COLLECTION
                )
            ),
        )

    ready = await qdrant.collection_exists(
        COLLECTION
    )

    if not ready:
        return CoreRecallResponse(
            guidance=[],
            collection_ready=False,
        )

    query_text = (
        f"Consumer role: "
        f"{request.consumer_role}\n"
        f"Decision context: "
        f"{request.query}"
    )

    vector = await embed(
        query_text
    )

    result = await qdrant.query_points(
        collection_name=COLLECTION,
        query=vector,
        query_filter=models.Filter(
            must=[
                models.FieldCondition(
                    key="ethics_status",
                    match=models.MatchValue(
                        value="approved",
                    ),
                ),
                models.FieldCondition(
                    key="sanitization_version",
                    match=models.MatchValue(
                        value=SANITIZATION_VERSION,
                    ),
                ),
            ]
        ),
        limit=max(
            request.limit * 3,
            8,
        ),
        with_payload=True,
    )

    guidance: list[
        CoreGuidance
    ] = []

    seen: set[str] = set()

    for point in result.points:
        payload = point.payload or {}

        item = safe_guidance(
            payload
        )

        if item is None:
            continue

        if (
            item.behavior_key
            in seen
        ):
            continue

        seen.add(
            item.behavior_key
        )

        guidance.append(
            item
        )

        if (
            len(guidance)
            >= request.limit
        ):
            break

    return CoreRecallResponse(
        guidance=guidance,
        collection_ready=True,
    )


def selftest() -> None:

    fields = set(
        CoreRecallRequest
        .model_fields
        .keys()
    )

    assert fields == {
        "query",
        "consumer_role",
        "limit",
    }

    forbidden_request_fields = {
        "company_id",
        "user_id",
        "evidence_id",
        "task_id",
        "run_id",
        "objective",
        "output_json",
        "source_message_id",
    }

    assert not (
        fields
        & forbidden_request_fields
    )

    fake = {
        "behavior_key":
            "cto:verification",
        "category":
            "verification",
        "agent_role":
            "CTO",
        "behavior_text":
            (
                "Verify evidence before "
                "accepting an operational "
                "result."
            ),
        "quality_score":
            0.97,
        "ethics_status":
            "approved",
        "sanitization_version":
            SANITIZATION_VERSION,
        "policy_version":
            "flwme-core-policy-v1",

        # Simulated upstream tenant fields.
        # They MUST never be returned.
        "company_id":
            123,
        "user_id":
            456,
        "customer":
            "PRIVATE CUSTOMER",
        "objective":
            "PRIVATE OBJECTIVE",
        "secret":
            "TENANT-SECRET-XYZ",
    }

    safe = safe_guidance(
        fake
    )

    assert safe is not None

    serialized = json.dumps(
        safe.model_dump(),
        sort_keys=True,
    )

    for forbidden in (
        "company_id",
        "user_id",
        "customer",
        "PRIVATE CUSTOMER",
        "objective",
        "PRIVATE OBJECTIVE",
        "TENANT-SECRET-XYZ",
    ):
        assert (
            forbidden
            not in serialized
        )

    assert COLLECTION == (
        "flwme_core_learning_v1"
    )

    assert (
        "company"
        not in CoreRecallRequest
        .model_json_schema()
        .get(
            "properties",
            {},
        )
    )

    print(
        "SELFTEST_REQUEST_TENANT_SCOPE=0"
    )
    print(
        "SELFTEST_RESPONSE_TENANT_DATA=0"
    )
    print(
        "SELFTEST_ETHICS_FILTER=APPROVED_ONLY"
    )
    print(
        "SELFTEST_SANITIZATION_FILTER="
        + SANITIZATION_VERSION
    )
    print(
        "SELFTEST_COLLECTION="
        + COLLECTION
    )
    print(
        "CORE_RECALL_SELFTEST=PASS"
    )


def main() -> None:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--selftest",
        action="store_true",
    )

    args = parser.parse_args()

    if args.selftest:
        selftest()
        return

    parser.error(
        "run with uvicorn or --selftest"
    )


if __name__ == "__main__":
    main()
