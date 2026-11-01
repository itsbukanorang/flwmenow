from importlib.metadata import PackageNotFoundError, version
from typing import Any, Literal

import fi.alk  # noqa: F401
from fastapi import FastAPI
from pydantic import BaseModel, Field


SCHEMA_VERSION = "flwme.learning.evidence.v1"


def alk_version() -> str:
    try:
        return version("agent-learning-kit")
    except PackageNotFoundError:
        return "unavailable"


class LearningEvidence(BaseModel):
    schema_version: Literal["flwme.learning.evidence.v1"] = SCHEMA_VERSION

    company_id: int = Field(gt=0)
    user_id: int = Field(gt=0)

    agent: str = Field(
        min_length=2,
        max_length=64,
        pattern=r"^[A-Za-z0-9._:-]+$",
    )

    run_id: str = Field(
        min_length=8,
        max_length=128,
    )

    task_id: str | None = Field(
        default=None,
        min_length=8,
        max_length=128,
    )

    objective: str = Field(
        min_length=1,
        max_length=8000,
    )

    output: dict[str, Any]

    evidence: list[dict[str, Any]] = Field(
        default_factory=list,
        max_length=64,
    )

    verification_status: Literal["pending"] = "pending"
    learning_status: Literal["observed"] = "observed"


class ValidationOutput(BaseModel):
    accepted: bool
    schema_version: str
    alk_version: str
    company_id: int
    agent: str
    run_id: str
    verification_status: str
    learning_status: str


app = FastAPI(
    title="FLWME Learning Runtime",
    version="0.1.0",
)


@app.get("/healthz")
async def healthz() -> dict[str, Any]:
    installed = alk_version()

    return {
        "ok": installed != "unavailable",
        "service": "flwme-learning",
        "alk": "future-agi-agent-learning-kit",
        "alk_version": installed,
        "persistence": "disabled",
        "promotion": "disabled",
    }


@app.post(
    "/v1/evidence/validate",
    response_model=ValidationOutput,
)
async def validate_evidence(
    evidence: LearningEvidence,
) -> ValidationOutput:
    return ValidationOutput(
        accepted=True,
        schema_version=evidence.schema_version,
        alk_version=alk_version(),
        company_id=evidence.company_id,
        agent=evidence.agent,
        run_id=evidence.run_id,
        verification_status=evidence.verification_status,
        learning_status=evidence.learning_status,
    )
