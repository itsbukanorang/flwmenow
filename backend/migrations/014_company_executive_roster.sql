BEGIN;

CREATE TABLE company_executive_roster (
    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    executive TEXT NOT NULL,

    active BOOLEAN NOT NULL
        DEFAULT TRUE,

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    PRIMARY KEY (
        company_id,
        executive
    ),

    CONSTRAINT company_executive_roster_role_valid
        CHECK (
            executive IN (
                'CTO',
                'COO',
                'CFO',
                'CMO'
            )
        )
);

INSERT INTO company_executive_roster (
    company_id,
    executive,
    active
)
SELECT
    c.id,
    role.executive,
    TRUE
FROM companies c
CROSS JOIN (
    VALUES
        ('CTO'),
        ('COO'),
        ('CFO'),
        ('CMO')
) AS role(executive)
ON CONFLICT DO NOTHING;


CREATE OR REPLACE FUNCTION
seed_company_executive_roster()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO company_executive_roster (
        company_id,
        executive,
        active
    )
    VALUES
        (NEW.id, 'CTO', TRUE),
        (NEW.id, 'COO', TRUE),
        (NEW.id, 'CFO', TRUE),
        (NEW.id, 'CMO', TRUE)
    ON CONFLICT DO NOTHING;

    RETURN NEW;
END;
$$;


CREATE TRIGGER companies_seed_executive_roster
AFTER INSERT ON companies
FOR EACH ROW
EXECUTE FUNCTION
seed_company_executive_roster();


CREATE INDEX company_executive_roster_active_idx
    ON company_executive_roster (
        company_id,
        active,
        executive
    );

COMMIT;
