#!/usr/bin/env python3

import argparse
import json
import os
import shutil
import subprocess
from typing import Any


POLICY_VERSION = (
    "flwme-promotion-policy-v1"
)


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


def decide(
    record: dict[str, Any],
) -> dict[str, Any]:
    if (
        record.get(
            "verification_status"
        )
        != "verified"
        or record.get(
            "verification_verdict"
        )
        != "verified"
    ):
        return {
            "eligible":
                False,

            "decision":
                "not_candidate",

            "reason_code":
                "verification_not_verified",
        }

    if (
        record.get(
            "learning_status"
        )
        not in {
            "evaluated",
            "candidate",
        }
    ):
        return {
            "eligible":
                False,

            "decision":
                "not_candidate",

            "reason_code":
                "learning_status_not_eligible",
        }

    if (
        record.get(
            "evaluation_status"
        )
        != "passed"
    ):
        return {
            "eligible":
                True,

            "decision":
                "blocked",

            "reason_code":
                "alk_evaluation_not_passed",
        }

    score = float(
        record["evaluation_score"]
    )

    threshold = float(
        record["evaluation_threshold"]
    )

    if score < threshold:
        return {
            "eligible":
                True,

            "decision":
                "blocked",

            "reason_code":
                "alk_score_below_threshold",
        }

    source_type = record.get(
        "oracle_source_type"
    )

    if source_type != "database_verified":
        return {
            "eligible":
                True,

            "decision":
                "blocked",

            "reason_code":
                "truth_source_not_promotable",
        }

    if (
        record.get(
            "source_event"
        )
        != "executive_task_completion"
    ):
        return {
            "eligible":
                True,

            "decision":
                "blocked",

            "reason_code":
                "unsupported_source_event",
        }

    return {
        "eligible":
            True,

        "decision":
            "approved",

        "reason_code":
            "promotion_policy_passed",
    }


def load_evidence(
    evidence_id: int,
) -> dict[str, Any]:
    raw = psql(
        f"""
        SELECT json_build_object(
            'evidence_id',
                le.id,

            'company_id',
                le.company_id,

            'learning_status',
                le.learning_status,

            'verification_status',
                le.verification_status,

            'source_event',
                le.source_event,

            'evaluation_id',
                ev.id,

            'evaluation_status',
                ev.status,

            'evaluation_score',
                ev.score,

            'evaluation_threshold',
                ev.threshold,

            'verification_id',
                v.id,

            'verification_verdict',
                v.verdict,

            'oracle_source_type',
                o.source_type
        )::text

        FROM learning_evidence le

        LEFT JOIN learning_evaluations ev
          ON ev.evidence_id = le.id

        LEFT JOIN learning_verifications v
          ON v.evidence_id = le.id

        LEFT JOIN learning_verification_oracles o
          ON o.id = v.oracle_id

        WHERE le.id = {int(evidence_id)}

        LIMIT 1;
        """
    )

    if not raw:
        raise RuntimeError(
            "evidence not found"
        )

    return json.loads(
        raw
    )


def selftest() -> None:
    base = {
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

        "source_event":
            "executive_task_completion",
    }

    fixture = {
        **base,
        "oracle_source_type":
            "fixture",
    }

    fixture_result = decide(
        fixture
    )

    assert (
        fixture_result["eligible"]
        is True
    )

    assert (
        fixture_result["decision"]
        == "blocked"
    )

    assert (
        fixture_result["reason_code"]
        == "truth_source_not_promotable"
    )

    trusted = {
        **base,
        "oracle_source_type":
            "database_verified",
    }

    trusted_result = decide(
        trusted
    )

    assert (
        trusted_result["decision"]
        == "approved"
    )

    rejected = {
        **base,

        "learning_status":
            "rejected",

        "verification_status":
            "rejected",

        "verification_verdict":
            "rejected",

        "oracle_source_type":
            "database_verified",
    }

    rejected_result = decide(
        rejected
    )

    assert (
        rejected_result["eligible"]
        is False
    )

    assert (
        rejected_result["decision"]
        == "not_candidate"
    )

    low_score = {
        **base,

        "evaluation_score":
            0.60,

        "oracle_source_type":
            "database_verified",
    }

    low_result = decide(
        low_score
    )

    assert (
        low_result["decision"]
        == "blocked"
    )

    assert (
        low_result["reason_code"]
        == "alk_score_below_threshold"
    )

    print(
        "SELFTEST_VERIFIED_FIXTURE="
        "CANDIDATE_BLOCKED"
    )

    print(
        "SELFTEST_VERIFIED_DATABASE_TRUTH="
        "APPROVED"
    )

    print(
        "SELFTEST_REJECTED_EVIDENCE="
        "NOT_CANDIDATE"
    )

    print(
        "SELFTEST_LOW_ALK_SCORE="
        "BLOCKED"
    )

    print(
        "PROMOTION_POLICY_SELFTEST=PASS"
    )


def inspect(
    evidence_id: int,
) -> None:
    record = load_evidence(
        evidence_id
    )

    decision = decide(
        record
    )

    print(
        json.dumps(
            {
                "policy_version":
                    POLICY_VERSION,

                "record":
                    record,

                "policy_result":
                    decision,
            },
            sort_keys=True,
            indent=2,
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--selftest",
        action="store_true",
    )

    parser.add_argument(
        "--inspect-evidence",
        type=int,
    )

    args = parser.parse_args()

    if args.selftest:
        selftest()
        return 0

    if args.inspect_evidence:
        inspect(
            args.inspect_evidence
        )
        return 0

    parser.error(
        "--selftest or "
        "--inspect-evidence required"
    )


if __name__ == "__main__":
    raise SystemExit(main())
