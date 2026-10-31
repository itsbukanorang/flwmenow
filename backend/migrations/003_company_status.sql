BEGIN;

ALTER TABLE companies
ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active';

ALTER TABLE companies
DROP CONSTRAINT IF EXISTS companies_status_valid;

ALTER TABLE companies
ADD CONSTRAINT companies_status_valid
CHECK (status IN ('active', 'paused'));

COMMIT;
