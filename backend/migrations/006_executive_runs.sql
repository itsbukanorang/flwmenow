BEGIN;

CREATE TABLE executive_runs (
    run_id TEXT PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    executive TEXT NOT NULL,

    orchestration TEXT NOT NULL
        DEFAULT 'langgraph',

    status TEXT NOT NULL,

    attempt INTEGER NOT NULL
        DEFAULT 0,

    retry_state TEXT NOT NULL
        DEFAULT 'idle',

    verifier_state TEXT NOT NULL
        DEFAULT 'not_started',

    last_error TEXT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT executive_runs_executive_valid
        CHECK (
            executive IN (
                'CTO',
                'COO',
                'CFO',
                'CMO'
            )
        ),

    CONSTRAINT executive_runs_orchestration_valid
        CHECK (
            orchestration = 'langgraph'
        ),

    CONSTRAINT executive_runs_status_valid
        CHECK (
            status IN (
                'created',
                'routed',
                'executing',
                'completed',
                'failed'
            )
        ),

    CONSTRAINT executive_runs_retry_valid
        CHECK (
            retry_state IN (
                'idle',
                'retrying',
                'exhausted'
            )
        ),

    CONSTRAINT executive_runs_verifier_valid
        CHECK (
            verifier_state IN (
                'not_started',
                'pending',
                'passed',
                'failed'
            )
        )
);

CREATE INDEX executive_runs_company_idx
    ON executive_runs(
        company_id,
        user_id,
        created_at DESC
    );

CREATE INDEX executive_runs_status_idx
    ON executive_runs(
        status,
        updated_at DESC
    );

COMMIT;
