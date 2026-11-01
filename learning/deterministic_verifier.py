#!/usr/bin/env python3

import argparse
import json
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any


REQUEST_SCHEMA = "flwme.verification.request.v1"
RESULT_SCHEMA = "flwme.verification.result.v1"

VERIFIER_NAME = "flwme-deterministic-verifier"
VERIFIER_VERSION = "0.1.0"

TRUSTED_SOURCES = {
    "fixture",
    "database_verified",
}

SUPPORTED_COMPARISONS = {
    "numeric_exact",
    "numeric_tolerance",
    "exact_text",
    "boolean_exact",
}


class ContractError(ValueError):
    pass


def result(
    *,
    evidence_id: int,
    company_id: int,
    verdict: str,
    reason_code: str,
    comparison: str,
    expected: Any,
    actual: Any,
) -> dict[str, Any]:
    return {
        "schema_version": RESULT_SCHEMA,
        "verifier": VERIFIER_NAME,
        "verifier_version": VERIFIER_VERSION,
        "evidence_id": evidence_id,
        "company_id": company_id,
        "verdict": verdict,
        "reason_code": reason_code,
        "comparison": comparison,
        "expected": expected,
        "actual": actual,
        "deterministic": True,
        "alk_quality_is_authority": False,
    }


def to_decimal(value: Any) -> Decimal:
    if isinstance(value, bool):
        raise InvalidOperation

    return Decimal(str(value))


def verify(request: dict[str, Any]) -> dict[str, Any]:
    if request.get("schema_version") != REQUEST_SCHEMA:
        raise ContractError(
            "unsupported request schema"
        )

    try:
        evidence_id = int(
            request["evidence_id"]
        )

        company_id = int(
            request["company_id"]
        )

        oracle = request["oracle"]

    except (
        KeyError,
        TypeError,
        ValueError,
    ) as exc:
        raise ContractError(
            "invalid verification request"
        ) from exc

    if not isinstance(oracle, dict):
        raise ContractError(
            "oracle must be an object"
        )

    source_type = oracle.get(
        "source_type"
    )

    comparison = oracle.get(
        "comparison"
    )

    expected = oracle.get(
        "expected"
    )

    actual = request.get(
        "actual"
    )

    if comparison not in SUPPORTED_COMPARISONS:
        raise ContractError(
            "unsupported comparison"
        )

    if source_type not in TRUSTED_SOURCES:
        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict="rejected",
            reason_code=(
                "untrusted_ground_truth_source"
            ),
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    try:
        source_company_id = int(
            oracle["source_company_id"]
        )
    except (
        KeyError,
        TypeError,
        ValueError,
    ):
        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict="rejected",
            reason_code=(
                "missing_ground_truth_company"
            ),
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if source_company_id != company_id:
        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict="rejected",
            reason_code="cross_tenant_source",
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if expected is None:
        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict="rejected",
            reason_code="missing_ground_truth",
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if actual is None:
        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict="rejected",
            reason_code="missing_actual_value",
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if comparison in {
        "numeric_exact",
        "numeric_tolerance",
    }:
        try:
            expected_number = to_decimal(
                expected
            )
            actual_number = to_decimal(
                actual
            )
        except (
            InvalidOperation,
            ValueError,
        ):
            return result(
                evidence_id=evidence_id,
                company_id=company_id,
                verdict="rejected",
                reason_code=(
                    "invalid_numeric_value"
                ),
                comparison=comparison,
                expected=expected,
                actual=actual,
            )

        if comparison == "numeric_exact":
            passed = (
                actual_number
                == expected_number
            )

            reason = (
                "verified_exact_numeric_match"
                if passed
                else "wrong_numeric_answer"
            )

        else:
            tolerance_raw = oracle.get(
                "tolerance"
            )

            if tolerance_raw is None:
                raise ContractError(
                    "numeric_tolerance requires "
                    "tolerance"
                )

            try:
                tolerance = to_decimal(
                    tolerance_raw
                )
            except (
                InvalidOperation,
                ValueError,
            ) as exc:
                raise ContractError(
                    "invalid tolerance"
                ) from exc

            if tolerance < 0:
                raise ContractError(
                    "tolerance cannot be negative"
                )

            difference = abs(
                actual_number
                - expected_number
            )

            passed = (
                difference <= tolerance
            )

            reason = (
                "verified_within_tolerance"
                if passed
                else "outside_numeric_tolerance"
            )

        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict=(
                "verified"
                if passed
                else "rejected"
            ),
            reason_code=reason,
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if comparison == "exact_text":
        passed = (
            str(actual)
            == str(expected)
        )

        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict=(
                "verified"
                if passed
                else "rejected"
            ),
            reason_code=(
                "verified_exact_text_match"
                if passed
                else "wrong_text_answer"
            ),
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    if comparison == "boolean_exact":
        if (
            not isinstance(expected, bool)
            or not isinstance(actual, bool)
        ):
            return result(
                evidence_id=evidence_id,
                company_id=company_id,
                verdict="rejected",
                reason_code=(
                    "invalid_boolean_value"
                ),
                comparison=comparison,
                expected=expected,
                actual=actual,
            )

        passed = actual is expected

        return result(
            evidence_id=evidence_id,
            company_id=company_id,
            verdict=(
                "verified"
                if passed
                else "rejected"
            ),
            reason_code=(
                "verified_exact_boolean_match"
                if passed
                else "wrong_boolean_answer"
            ),
            comparison=comparison,
            expected=expected,
            actual=actual,
        )

    raise ContractError(
        "unreachable comparison"
    )


def fixture(
    *,
    actual: Any,
    expected: Any = 30,
    company_id: int = 1,
    source_company_id: int = 1,
    comparison: str = "numeric_exact",
    source_type: str = "fixture",
    tolerance: Any = None,
    alk_quality_score: float | None = None,
) -> dict[str, Any]:
    oracle = {
        "source_type": source_type,
        "source_company_id":
            source_company_id,
        "comparison": comparison,
        "expected": expected,
    }

    if tolerance is not None:
        oracle["tolerance"] = tolerance

    request = {
        "schema_version":
            REQUEST_SCHEMA,
        "evidence_id": 9001,
        "company_id": company_id,
        "oracle": oracle,
        "actual": actual,
    }

    if alk_quality_score is not None:
        request["alk_quality_score"] = (
            alk_quality_score
        )

    return request


def selftest() -> None:
    cases = [
        (
            "correct_numeric",
            fixture(actual=30),
            "verified",
            "verified_exact_numeric_match",
        ),
        (
            "wrong_numeric",
            fixture(actual=45),
            "rejected",
            "wrong_numeric_answer",
        ),
        (
            "high_alk_but_wrong",
            fixture(
                actual=45,
                alk_quality_score=0.99,
            ),
            "rejected",
            "wrong_numeric_answer",
        ),
        (
            "cross_tenant",
            fixture(
                actual=30,
                company_id=1,
                source_company_id=2,
            ),
            "rejected",
            "cross_tenant_source",
        ),
        (
            "untrusted_truth",
            fixture(
                actual=30,
                source_type="model_generated",
            ),
            "rejected",
            "untrusted_ground_truth_source",
        ),
        (
            "within_tolerance",
            fixture(
                actual="10.004",
                expected="10",
                comparison="numeric_tolerance",
                tolerance="0.01",
            ),
            "verified",
            "verified_within_tolerance",
        ),
        (
            "outside_tolerance",
            fixture(
                actual="10.02",
                expected="10",
                comparison="numeric_tolerance",
                tolerance="0.01",
            ),
            "rejected",
            "outside_numeric_tolerance",
        ),
        (
            "exact_text",
            fixture(
                actual="approved",
                expected="approved",
                comparison="exact_text",
            ),
            "verified",
            "verified_exact_text_match",
        ),
        (
            "wrong_text",
            fixture(
                actual="rejected",
                expected="approved",
                comparison="exact_text",
            ),
            "rejected",
            "wrong_text_answer",
        ),
        (
            "boolean_true",
            fixture(
                actual=True,
                expected=True,
                comparison="boolean_exact",
            ),
            "verified",
            "verified_exact_boolean_match",
        ),
    ]

    for (
        name,
        request,
        expected_verdict,
        expected_reason,
    ) in cases:
        output = verify(request)

        assert (
            output["verdict"]
            == expected_verdict
        ), name

        assert (
            output["reason_code"]
            == expected_reason
        ), name

        assert (
            output[
                "alk_quality_is_authority"
            ]
            is False
        ), name

        print(
            f"SELFTEST_{name}=PASS "
            f"verdict={output['verdict']} "
            f"reason={output['reason_code']}"
        )

    request = fixture(
        actual=45,
        alk_quality_score=0.9999,
    )

    first = verify(request)

    serialized = json.dumps(
        first,
        sort_keys=True,
        separators=(",", ":"),
    )

    for _ in range(100):
        current = json.dumps(
            verify(request),
            sort_keys=True,
            separators=(",", ":"),
        )

        assert current == serialized

    print(
        "SELFTEST_DETERMINISTIC_REPEAT_100=PASS"
    )

    print(
        "SELFTEST_ALK_HIGH_SCORE_CANNOT_OVERRIDE_TRUTH=PASS"
    )


def main() -> int:
    parser = argparse.ArgumentParser()

    parser.add_argument(
        "--selftest",
        action="store_true",
    )

    parser.add_argument(
        "--input",
        type=Path,
    )

    args = parser.parse_args()

    if args.selftest:
        selftest()
        return 0

    if args.input is None:
        parser.error(
            "--input or --selftest required"
        )

    request = json.loads(
        args.input.read_text(
            encoding="utf-8"
        )
    )

    output = verify(request)

    print(
        json.dumps(
            output,
            sort_keys=True,
            indent=2,
        )
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
