BEGIN;

CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    thread_id BIGINT NOT NULL,

    source_message_id BIGINT,

    assigned_to TEXT NOT NULL,

    title TEXT NOT NULL,

    objective TEXT NOT NULL,

    context TEXT,

    constraints_json TEXT NOT NULL
        DEFAULT '[]',

    status TEXT NOT NULL
        DEFAULT 'queued',

    attempt_count INTEGER NOT NULL
        DEFAULT 0,

    result_json TEXT,

    last_error TEXT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    started_at TIMESTAMPTZ,

    completed_at TIMESTAMPTZ,

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT tasks_status_valid
        CHECK (
            status IN (
                'queued',
                'running',
                'completed',
                'failed',
                'need_attention'
            )
        ),

    CONSTRAINT tasks_attempt_valid
        CHECK (
            attempt_count >= 0
        )
);

CREATE INDEX tasks_company_live_idx
    ON tasks (
        company_id,
        user_id,
        status,
        updated_at DESC
    );

CREATE INDEX tasks_queue_idx
    ON tasks (
        status,
        created_at
    )
    WHERE status = 'queued';

CREATE TABLE task_events (
    id BIGSERIAL PRIMARY KEY,

    task_id TEXT NOT NULL
        REFERENCES tasks(task_id)
        ON DELETE CASCADE,

    event_type TEXT NOT NULL,

    detail TEXT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now()
);

CREATE INDEX task_events_task_idx
    ON task_events (
        task_id,
        id
    );

ALTER TABLE executive_runs
    ADD COLUMN task_id TEXT;

CREATE INDEX executive_runs_task_idx
    ON executive_runs (
        task_id,
        created_at DESC
    );

ALTER TABLE chat_messages
    ADD COLUMN task_id TEXT;

CREATE INDEX chat_messages_task_idx
    ON chat_messages (
        task_id
    );

COMMIT;
