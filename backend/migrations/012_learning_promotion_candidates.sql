CREATE TABLE learning_promotion_candidates (
    id BIGSERIAL PRIMARY KEY,

    evidence_id BIGINT NOT NULL
        REFERENCES learning_evidence(id)
        ON DELETE CASCADE,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    evaluation_id BIGINT NOT NULL
        REFERENCES learning_evaluations(id)
        ON DELETE RESTRICT,

    verification_id BIGINT NOT NULL
        REFERENCES learning_verifications(id)
        ON DELETE RESTRICT,

    policy_version TEXT NOT NULL,

    status TEXT NOT NULL
        DEFAULT 'pending',

    reason_code TEXT,

    evaluation_score DOUBLE PRECISION
        NOT NULL,

    evaluation_threshold DOUBLE PRECISION
        NOT NULL,

    oracle_source_type TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    decided_at TIMESTAMPTZ,

    CONSTRAINT learning_promotion_candidates_evidence_unique
        UNIQUE (evidence_id),

    CONSTRAINT learning_promotion_candidate_status_check
        CHECK (
            status IN (
                'pending',
                'approved',
                'blocked'
            )
        ),

    CONSTRAINT learning_promotion_candidate_source_check
        CHECK (
            oracle_source_type IN (
                'fixture',
                'database_verified'
            )
        ),

    CONSTRAINT learning_promotion_candidate_decision_check
        CHECK (
            (
                status = 'pending'
                AND reason_code IS NULL
                AND decided_at IS NULL
            )
            OR
            (
                status IN (
                    'approved',
                    'blocked'
                )
                AND reason_code IS NOT NULL
                AND decided_at IS NOT NULL
            )
        )
);

CREATE INDEX
    learning_promotion_candidates_status_idx
ON learning_promotion_candidates (
    status,
    id
);

CREATE INDEX
    learning_promotion_candidates_company_idx
ON learning_promotion_candidates (
    company_id,
    created_at DESC
);
