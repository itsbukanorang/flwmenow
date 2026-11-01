from typing import Any, TypedDict

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from langgraph.checkpoint.memory import InMemorySaver
from langgraph.constants import END, START
from langgraph.graph import StateGraph
from langgraph.types import Command, interrupt

SUPPORTED_EXECUTIVES = {
    "CTO",
    "COO",
    "CFO",
    "CMO",
}



class DelegationState(TypedDict, total=False):
    run_id: str
    company_id: int
    user_id: int
    executive: str
    task: dict[str, Any]

    status: str
    attempt: int
    max_attempts: int

    plan: list[str]
    retry_state: str
    verifier_state: str

    executive_result: dict[str, Any]
    last_error: str | None


class StartRequest(BaseModel):
    run_id: str = Field(
        min_length=8,
        max_length=128,
    )

    company_id: int = Field(gt=0)
    user_id: int = Field(gt=0)

    executive: str

    task: dict[str, Any]


class ResumeRequest(BaseModel):
    run_id: str = Field(
        min_length=8,
        max_length=128,
    )

    ok: bool

    output: dict[str, Any] | None = None
    error: str | None = None


def validate_and_plan(
    state: DelegationState,
) -> dict[str, Any]:
    executive = state["executive"]

    if executive not in SUPPORTED_EXECUTIVES:
        raise ValueError(
            "unsupported executive"
        )

    task = state.get("task")

    if not isinstance(task, dict):
        raise ValueError(
            "typed task required"
        )

    objective = str(
        task.get("objective", "")
    ).strip()

    if not objective:
        raise ValueError(
            "task objective required"
        )

    return {
        "status": "planned",
        "attempt": 0,
        "max_attempts": 1,
        "plan": [
            f"delegate:{executive}",
        ],
        "retry_state": "idle",
        "verifier_state": "not_started",
    }


def dispatch_executive(
    state: DelegationState,
) -> dict[str, Any]:
    executive = state["executive"]

    receipt = interrupt(
        {
            "action":
                f"execute_{executive.lower()}",
            "executive":
                executive,
            "run_id":
                state["run_id"],
            "attempt":
                state["attempt"],
        }
    )

    if not isinstance(receipt, dict):
        return {
            "status": "failed",
            "retry_state": "exhausted",
            "last_error":
                "invalid executive receipt",
        }

    if receipt.get("ok") is not True:
        return {
            "status": "failed",
            "retry_state": "exhausted",
            "last_error": str(
                receipt.get(
                    "error",
                    "executive execution failed",
                )
            )[:2000],
        }

    output = receipt.get("output")

    if not isinstance(output, dict):
        return {
            "status": "failed",
            "retry_state": "exhausted",
            "last_error":
                "executive output missing",
        }

    return {
        "status": "executive_completed",
        "executive_result": output,
    }


def finalize(
    state: DelegationState,
) -> dict[str, Any]:
    if state.get("status") == "failed":
        return {
            "status": "failed",
        }

    return {
        "status": "completed",
        # Verification is API-08.
        "verifier_state": "not_started",
        "retry_state": "idle",
    }


builder = StateGraph(
    DelegationState
)

builder.add_node(
    "validate_and_plan",
    validate_and_plan,
)

builder.add_node(
    "dispatch_executive",
    dispatch_executive,
)

builder.add_node(
    "finalize",
    finalize,
)

builder.add_edge(
    START,
    "validate_and_plan",
)

builder.add_edge(
    "validate_and_plan",
    "dispatch_executive",
)

builder.add_edge(
    "dispatch_executive",
    "finalize",
)

builder.add_edge(
    "finalize",
    END,
)

checkpointer = InMemorySaver()

graph = builder.compile(
    checkpointer=checkpointer
)

app = FastAPI(
    title="FLWME LangGraph Runtime",
)


def config_for(
    run_id: str,
) -> dict[str, Any]:
    return {
        "configurable": {
            "thread_id": run_id,
        }
    }


def start_run(
    request: StartRequest,
) -> dict[str, Any]:
    initial: DelegationState = {
        "run_id": request.run_id,
        "company_id":
            request.company_id,
        "user_id":
            request.user_id,
        "executive":
            request.executive,
        "task":
            request.task,
        "status":
            "created",
        "attempt":
            0,
        "max_attempts":
            1,
        "plan":
            [],
        "retry_state":
            "idle",
        "verifier_state":
            "not_started",
    }

    result = graph.invoke(
        initial,
        config_for(
            request.run_id
        ),
    )

    interrupts = result.get(
        "__interrupt__",
        (),
    )

    if len(interrupts) != 1:
        raise RuntimeError(
            "expected one executive interrupt"
        )

    payload = interrupts[0].value

    if not isinstance(payload, dict):
        raise RuntimeError(
            "invalid interrupt payload"
        )

    return {
        **payload,
        "status":
            "awaiting_executive",
        "plan":
            result.get(
                "plan",
                [
                    f"delegate:{request.executive}"
                ],
            ),
        "retry_state":
            result.get(
                "retry_state",
                "idle",
            ),
        "verifier_state":
            result.get(
                "verifier_state",
                "not_started",
            ),
    }


def resume_run(
    request: ResumeRequest,
) -> dict[str, Any]:
    resume_payload = {
        "ok":
            request.ok,
        "output":
            request.output,
        "error":
            request.error,
    }

    result = graph.invoke(
        Command(
            resume=resume_payload
        ),
        config_for(
            request.run_id
        ),
    )

    status = result.get(
        "status",
        "failed",
    )

    return {
        "run_id":
            request.run_id,
        "executive":
            result.get(
                "executive",
                "CTO",
            ),
        "status":
            status,
        "attempt":
            result.get(
                "attempt",
                0,
            ),
        "plan":
            result.get(
                "plan",
                [],
            ),
        "retry_state":
            result.get(
                "retry_state",
                "idle",
            ),
        "verifier_state":
            result.get(
                "verifier_state",
                "not_started",
            ),
        "result":
            result.get(
                "executive_result"
            ),
        "error":
            result.get(
                "last_error"
            ),
    }


@app.get("/healthz")
async def healthz():
    return {
        "ok": True,
        "runtime": "langgraph",
        "version": "1.3.0",
        "graph": "multi-executive-routing-v1",
    }


@app.post(
    "/v1/delegations/executive/start"
)
@app.post(
    "/v1/delegations/cto/start"
)
async def start(
    request: StartRequest,
):
    try:
        return start_run(
            request
        )

    except Exception as exc:
        raise HTTPException(
            status_code=422,
            detail=str(exc),
        ) from exc


@app.post(
    "/v1/delegations/executive/resume"
)
@app.post(
    "/v1/delegations/cto/resume"
)
async def resume(
    request: ResumeRequest,
):
    try:
        return resume_run(
            request
        )

    except Exception as exc:
        raise HTTPException(
            status_code=409,
            detail=str(exc),
        ) from exc


# FLWME API-05 MULTI-AGENT ORGANIZATION PLANNER

class OrganizationPlanRequest(BaseModel):
    task_id: str = Field(
        min_length=8,
        max_length=128,
    )

    active_executives: list[str]

    requested_batches: list[list[str]]


def resolve_organization_plan(
    request: OrganizationPlanRequest,
) -> list[list[str]]:
    active = set(
        request.active_executives
    )

    if not active.issubset(
        SUPPORTED_EXECUTIVES
    ):
        raise ValueError(
            "active roster contains unsupported executive"
        )

    if not request.requested_batches:
        raise ValueError(
            "organization plan requires at least one batch"
        )

    resolved: list[list[str]] = []
    seen: set[str] = set()

    for batch in request.requested_batches:
        if not batch:
            raise ValueError(
                "organization batch cannot be empty"
            )

        clean: list[str] = []

        for executive in batch:
            if executive not in SUPPORTED_EXECUTIVES:
                raise ValueError(
                    "unsupported executive in organization plan"
                )

            if executive not in active:
                raise ValueError(
                    "inactive executive in organization plan"
                )

            if executive in seen:
                raise ValueError(
                    "executive appears more than once"
                )

            seen.add(executive)
            clean.append(executive)

        resolved.append(clean)

    if len(seen) < 2:
        raise ValueError(
            "multi-agent plan requires at least two executives"
        )

    if len(seen) > 4:
        raise ValueError(
            "organization plan exceeds executive limit"
        )

    return resolved


@app.post(
    "/v1/orchestrations/plan"
)
async def organization_plan(
    request: OrganizationPlanRequest,
):
    try:
        batches = resolve_organization_plan(
            request
        )

        return {
            "task_id":
                request.task_id,
            "orchestration":
                "langgraph",
            "batches":
                batches,
        }

    except Exception as exc:
        raise HTTPException(
            status_code=422,
            detail=str(exc),
        ) from exc
