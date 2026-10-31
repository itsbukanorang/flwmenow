BEGIN;

CREATE TABLE IF NOT EXISTS companies (
    id BIGSERIAL PRIMARY KEY,
    owner_user_id BIGINT NOT NULL
        REFERENCES auth_users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT companies_name_length
        CHECK (char_length(btrim(name)) BETWEEN 2 AND 120),

    CONSTRAINT companies_owner_name_unique
        UNIQUE (owner_user_id, name_key)
);

CREATE INDEX IF NOT EXISTS companies_owner_created_idx
    ON companies(owner_user_id, created_at DESC);

COMMIT;
