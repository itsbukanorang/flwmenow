#!/usr/bin/env python3

import argparse
import json
from importlib.metadata import version
from pathlib import Path

from fi.alk.evals import (
    evaluate_task_evidence_auto,
)


ALK_PACKAGE = "agent-learning-kit"
ALK_VERSION = "0.3.4"


def main() -> int:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "evidence",
        type=Path,
    )

    parser.add_argument(
        "--output",
        type=Path,
        required=True,
    )

    parser.add_argument(
        "--threshold",
        type=float,
        default=0.70,
    )

    args = parser.parse_args()

    installed = version(ALK_PACKAGE)

    if installed != ALK_VERSION:
        raise RuntimeError(
            "ALK version drift: "
            f"expected {ALK_VERSION}, "
            f"got {installed}"
        )

    with args.evidence.open(
        "r",
        encoding="utf-8",
    ) as handle:
        evidence = json.load(handle)

    evidence_id = evidence.get("id")

    if evidence_id is None:
        raise ValueError(
            "FLWME evidence id is required"
        )

    result = evaluate_task_evidence_auto(
        evidence,
        threshold=args.threshold,
        name=f"flwme-evidence-{evidence_id}",
        source_path=".",
        metadata={
            "runtime": "flwme",
            "phase": "ALK-01C",
            "flwme_evidence_id": evidence_id,
        },
    )

    if (
        result.get("schema_version")
        != "agent-learning.artifact-evaluation.v1"
    ):
        raise RuntimeError(
            "Unexpected ALK evaluation schema"
        )

    status = result.get("status")

    if status not in {"passed", "failed"}:
        raise RuntimeError(
            f"Unexpected ALK status: {status!r}"
        )

    summary = result.get("summary") or {}

    score = summary.get("score")
    threshold = summary.get("threshold")

    if not isinstance(score, (int, float)):
        raise RuntimeError(
            "ALK evaluation score missing"
        )

    if not isinstance(
        threshold,
        (int, float),
    ):
        raise RuntimeError(
            "ALK evaluation threshold missing"
        )

    args.output.write_text(
        json.dumps(
            result,
            separators=(",", ":"),
            sort_keys=True,
            default=str,
        )
        + "\n",
        encoding="utf-8",
    )

    print(
        "ALK_EVALUATION_STATUS="
        + str(status)
    )

    print(
        "ALK_EVALUATION_SCORE="
        + str(score)
    )

    print(
        "ALK_EVALUATION_THRESHOLD="
        + str(threshold)
    )

    print(
        "ALK_EVALUATION_SCHEMA="
        + str(result["schema_version"])
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
