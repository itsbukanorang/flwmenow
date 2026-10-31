BEGIN;

CREATE TABLE IF NOT EXISTS chat_threads (
    id BIGSERIAL PRIMARY KEY,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    archived_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS
chat_threads_one_active_idx
ON chat_threads(company_id, user_id)
WHERE archived_at IS NULL;


CREATE TABLE IF NOT EXISTS chat_messages (
    id BIGSERIAL PRIMARY KEY,

    thread_id BIGINT NOT NULL
        REFERENCES chat_threads(id)
        ON DELETE CASCADE,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    user_id BIGINT NOT NULL
        REFERENCES auth_users(id)
        ON DELETE CASCADE,

    role TEXT NOT NULL,

    content TEXT NOT NULL,

    model TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT chat_messages_role_valid
        CHECK (
            role IN (
                'user',
                'assistant'
            )
        ),

    CONSTRAINT chat_messages_content_valid
        CHECK (
            char_length(content)
            BETWEEN 1 AND 100000
        )
);

CREATE INDEX IF NOT EXISTS
chat_messages_thread_idx
ON chat_messages(
    thread_id,
    id
);

CREATE INDEX IF NOT EXISTS
chat_messages_company_idx
ON chat_messages(
    company_id,
    created_at DESC
);


CREATE TABLE IF NOT EXISTS company_memory (
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

    kind TEXT NOT NULL,

    content TEXT NOT NULL,

    verified BOOLEAN NOT NULL
        DEFAULT FALSE,

    indexed_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT company_memory_kind_valid
        CHECK (
            kind IN (
                'user_episode',
                'fact',
                'decision',
                'preference',
                'outcome'
            )
        ),

    CONSTRAINT company_memory_content_valid
        CHECK (
            char_length(content)
            BETWEEN 1 AND 100000
        )
);

CREATE UNIQUE INDEX IF NOT EXISTS
company_memory_source_unique
ON company_memory(source_message_id)
WHERE source_message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS
company_memory_company_idx
ON company_memory(
    company_id,
    created_at DESC
);


CREATE TABLE IF NOT EXISTS memory_jobs (
    id BIGSERIAL PRIMARY KEY,

    memory_id BIGINT NOT NULL UNIQUE
        REFERENCES company_memory(id)
        ON DELETE CASCADE,

    status TEXT NOT NULL
        DEFAULT 'pending',

    attempts INTEGER NOT NULL
        DEFAULT 0,

    last_error TEXT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT memory_jobs_status_valid
        CHECK (
            status IN (
                'pending',
                'running',
                'done',
                'failed'
            )
        )
);

CREATE INDEX IF NOT EXISTS
memory_jobs_pending_idx
ON memory_jobs(
    status,
    id
);

COMMIT;
