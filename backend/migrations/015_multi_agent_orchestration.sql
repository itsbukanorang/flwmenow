BEGIN;

CREATE TABLE task_orchestrations (
    task_id TEXT PRIMARY KEY
        REFERENCES tasks(task_id)
        ON DELETE CASCADE,

    active_roster_json JSONB NOT NULL,

    requested_batches_json JSONB NOT NULL,

    executives_json JSONB NOT NULL,

    resolved_batches_json JSONB,

    status TEXT NOT NULL
        DEFAULT 'planned',

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT task_orchestrations_status_valid
        CHECK (
            status IN (
                'planned',
                'running',
                'completed',
                'failed'
            )
        ),

    CONSTRAINT task_orchestrations_active_roster_array
        CHECK (
            jsonb_typeof(active_roster_json)
                = 'array'
        ),

    CONSTRAINT task_orchestrations_requested_batches_array
        CHECK (
            jsonb_typeof(requested_batches_json)
                = 'array'
        ),

    CONSTRAINT task_orchestrations_executives_array
        CHECK (
            jsonb_typeof(executives_json)
                = 'array'
        )
);

CREATE INDEX task_orchestrations_status_idx
    ON task_orchestrations (
        status,
        updated_at DESC
    );

COMMIT;
