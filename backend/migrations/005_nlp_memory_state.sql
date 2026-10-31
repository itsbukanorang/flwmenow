BEGIN;

CREATE TABLE memory_ingest_jobs (
    id BIGSERIAL PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    source_message_id BIGINT
        REFERENCES chat_messages(id)
        ON DELETE SET NULL,

    content TEXT NOT NULL,

    status TEXT NOT NULL DEFAULT 'pending',

    attempts INTEGER NOT NULL DEFAULT 0,

    last_error TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT memory_ingest_status_valid
        CHECK (
            status IN (
                'pending',
                'running',
                'done',
                'failed'
            )
        )
);

CREATE UNIQUE INDEX memory_ingest_source_unique
    ON memory_ingest_jobs(source_message_id)
    WHERE source_message_id IS NOT NULL;

CREATE INDEX memory_ingest_pending_idx
    ON memory_ingest_jobs(status, id);


CREATE TABLE company_memory_state (
    id BIGSERIAL PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    memory_key TEXT NOT NULL,

    value TEXT NOT NULL,

    kind TEXT NOT NULL,

    source_message_id BIGINT
        REFERENCES chat_messages(id)
        ON DELETE SET NULL,

    version INTEGER NOT NULL DEFAULT 1,

    confidence DOUBLE PRECISION NOT NULL DEFAULT 1.0,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT company_memory_state_kind_valid
        CHECK (
            kind IN (
                'fact',
                'decision',
                'preference',
                'outcome'
            )
        ),

    CONSTRAINT company_memory_state_key_valid
        CHECK (
            char_length(memory_key)
            BETWEEN 2 AND 128
        ),

    CONSTRAINT company_memory_state_value_valid
        CHECK (
            char_length(value)
            BETWEEN 1 AND 10000
        ),

    CONSTRAINT company_memory_state_confidence_valid
        CHECK (
            confidence >= 0.0
            AND confidence <= 1.0
        ),

    UNIQUE (
        company_id,
        user_id,
        memory_key
    )
);

CREATE INDEX company_memory_state_company_idx
    ON company_memory_state(
        company_id,
        user_id,
        updated_at DESC
    );


CREATE TABLE company_memory_state_history (
    id BIGSERIAL PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    memory_key TEXT NOT NULL,

    value TEXT NOT NULL,

    kind TEXT NOT NULL,

    source_message_id BIGINT
        REFERENCES chat_messages(id)
        ON DELETE SET NULL,

    confidence DOUBLE PRECISION NOT NULL,

    applied BOOLEAN NOT NULL DEFAULT FALSE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (
        source_message_id,
        memory_key
    )
);

CREATE INDEX company_memory_state_history_idx
    ON company_memory_state_history(
        company_id,
        user_id,
        memory_key,
        source_message_id
    );


CREATE TABLE memory_state_jobs (
    id BIGSERIAL PRIMARY KEY,

    state_id BIGINT NOT NULL UNIQUE
        REFERENCES company_memory_state(id)
        ON DELETE CASCADE,

    target_version INTEGER NOT NULL,

    status TEXT NOT NULL DEFAULT 'pending',

    attempts INTEGER NOT NULL DEFAULT 0,

    last_error TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT memory_state_jobs_status_valid
        CHECK (
            status IN (
                'pending',
                'running',
                'done',
                'failed'
            )
        )
);

CREATE INDEX memory_state_jobs_pending_idx
    ON memory_state_jobs(status, id);


-- Backfill every existing USER message.
--
-- Questions will be rejected by the NLP extractor.
-- Natural statements that the old keyword filter missed
-- can now become durable state automatically.
INSERT INTO memory_ingest_jobs (
    company_id,
    user_id,
    source_message_id,
    content
)
SELECT
    company_id,
    user_id,
    id,
    content
FROM chat_messages
WHERE role = 'user'
ON CONFLICT (source_message_id)
WHERE source_message_id IS NOT NULL
DO NOTHING;

COMMIT;
