CREATE TABLE learning_verification_oracles (
    id BIGSERIAL PRIMARY KEY,

    company_id BIGINT NOT NULL,

    task_id TEXT NOT NULL,

    source_type TEXT NOT NULL,

    comparison TEXT NOT NULL,

    expected_json JSONB NOT NULL,

    tolerance NUMERIC,

    actual_path TEXT NOT NULL,

    active BOOLEAN NOT NULL
        DEFAULT TRUE,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT learning_oracle_source_check
        CHECK (
            source_type IN (
                'fixture',
                'database_verified'
            )
        ),

    CONSTRAINT learning_oracle_comparison_check
        CHECK (
            comparison IN (
                'numeric_exact',
                'numeric_tolerance',
                'exact_text',
                'boolean_exact'
            )
        ),

    CONSTRAINT learning_oracle_actual_path_check
        CHECK (
            length(trim(actual_path)) > 0
        ),

    CONSTRAINT learning_oracle_tolerance_check
        CHECK (
            (
                comparison = 'numeric_tolerance'
                AND tolerance IS NOT NULL
                AND tolerance >= 0
            )
            OR
            (
                comparison <> 'numeric_tolerance'
                AND tolerance IS NULL
            )
        )
);

CREATE UNIQUE INDEX
    learning_verification_oracles_active_unique
ON learning_verification_oracles (
    company_id,
    task_id
)
WHERE active = TRUE;


CREATE TABLE learning_verifications (
    id BIGSERIAL PRIMARY KEY,

    evidence_id BIGINT NOT NULL
        REFERENCES learning_evidence(id)
        ON DELETE CASCADE,

    oracle_id BIGINT NOT NULL
        REFERENCES learning_verification_oracles(id)
        ON DELETE RESTRICT,

    verifier TEXT NOT NULL,

    verifier_version TEXT NOT NULL,

    schema_version TEXT NOT NULL,

    verdict TEXT NOT NULL,

    reason_code TEXT NOT NULL,

    comparison TEXT NOT NULL,

    expected_json JSONB NOT NULL,

    actual_json JSONB NOT NULL,

    result_json JSONB NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT learning_verifications_evidence_unique
        UNIQUE (evidence_id),

    CONSTRAINT learning_verification_schema_check
        CHECK (
            schema_version =
            'flwme.verification.result.v1'
        ),

    CONSTRAINT learning_verification_verdict_check
        CHECK (
            verdict IN (
                'verified',
                'rejected'
            )
        )
);

CREATE INDEX learning_verifications_created_idx
    ON learning_verifications (
        created_at DESC
    );
