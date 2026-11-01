CREATE TABLE learning_evaluations (
    id BIGSERIAL PRIMARY KEY,

    evidence_id BIGINT NOT NULL
        REFERENCES learning_evidence(id)
        ON DELETE CASCADE,

    evaluator TEXT NOT NULL
        DEFAULT 'future-agi-agent-learning-kit',

    evaluator_version TEXT NOT NULL,

    schema_version TEXT NOT NULL,

    status TEXT NOT NULL,

    score DOUBLE PRECISION NOT NULL,

    threshold DOUBLE PRECISION NOT NULL,

    findings_json JSONB NOT NULL
        DEFAULT '[]'::jsonb,

    metrics_json JSONB NOT NULL
        DEFAULT '{}'::jsonb,

    result_json JSONB NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT learning_evaluations_evidence_unique
        UNIQUE (evidence_id),

    CONSTRAINT learning_evaluations_schema_check
        CHECK (
            schema_version =
            'agent-learning.artifact-evaluation.v1'
        ),

    CONSTRAINT learning_evaluations_status_check
        CHECK (
            status IN ('passed', 'failed')
        ),

    CONSTRAINT learning_evaluations_score_check
        CHECK (
            score >= 0.0
            AND score <= 1.0
        ),

    CONSTRAINT learning_evaluations_threshold_check
        CHECK (
            threshold >= 0.0
            AND threshold <= 1.0
        )
);

CREATE INDEX learning_evaluations_created_idx
    ON learning_evaluations (
        created_at DESC
    );
