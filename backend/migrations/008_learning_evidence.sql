-- FLWME ALK-00C
-- Durable raw learning evidence.
-- Raw evidence is NOT trusted organizational memory.
-- Promotion into trusted memory requires later verification/evaluation gates.

CREATE TABLE learning_evidence (
    id BIGSERIAL PRIMARY KEY,

    schema_version TEXT NOT NULL
        DEFAULT 'flwme.learning.evidence.v1',

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    task_id TEXT NOT NULL
        REFERENCES tasks(task_id)
        ON DELETE CASCADE,

    run_id TEXT NOT NULL
        REFERENCES executive_runs(run_id)
        ON DELETE CASCADE,

    agent TEXT NOT NULL,

    objective TEXT NOT NULL,

    output_json JSONB NOT NULL,

    evidence_json JSONB NOT NULL
        DEFAULT '[]'::jsonb,

    verification_status TEXT NOT NULL
        DEFAULT 'pending',

    learning_status TEXT NOT NULL
        DEFAULT 'observed',

    source_event TEXT NOT NULL
        DEFAULT 'executive_task_completion',

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT learning_evidence_schema_valid
        CHECK (
            schema_version = 'flwme.learning.evidence.v1'
        ),

    CONSTRAINT learning_evidence_agent_valid
        CHECK (
            char_length(agent) BETWEEN 2 AND 64
        ),

    CONSTRAINT learning_evidence_objective_valid
        CHECK (
            char_length(objective) BETWEEN 1 AND 8000
        ),

    CONSTRAINT learning_evidence_verification_valid
        CHECK (
            verification_status IN (
                'pending',
                'evaluated',
                'verified',
                'rejected'
            )
        ),

    CONSTRAINT learning_evidence_learning_valid
        CHECK (
            learning_status IN (
                'observed',
                'evaluated',
                'candidate',
                'promoted',
                'rejected'
            )
        ),

    CONSTRAINT learning_evidence_source_valid
        CHECK (
            source_event IN (
                'executive_task_completion'
            )
        ),

    CONSTRAINT learning_evidence_run_event_unique
        UNIQUE (
            run_id,
            source_event
        )
);

CREATE INDEX learning_evidence_company_created_idx
    ON learning_evidence (
        company_id,
        created_at DESC
    );

CREATE INDEX learning_evidence_task_idx
    ON learning_evidence (
        task_id
    );

CREATE INDEX learning_evidence_learning_status_idx
    ON learning_evidence (
        learning_status,
        created_at
    );
