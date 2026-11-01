CREATE TABLE learning_verification_benchmarks (
    id BIGSERIAL PRIMARY KEY,

    benchmark_key TEXT NOT NULL UNIQUE,

    company_id BIGINT NOT NULL
        REFERENCES companies(id)
        ON DELETE CASCADE,

    source_type TEXT NOT NULL,

    comparison TEXT NOT NULL,

    expected_json JSONB NOT NULL,

    tolerance NUMERIC,

    actual_path TEXT NOT NULL,

    active BOOLEAN NOT NULL
        DEFAULT TRUE,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT learning_benchmark_key_check
        CHECK (
            benchmark_key ~
            '^[A-Z0-9][A-Z0-9_-]{7,127}$'
        ),

    CONSTRAINT learning_benchmark_source_check
        CHECK (
            source_type IN (
                'fixture',
                'database_verified'
            )
        ),

    CONSTRAINT learning_benchmark_comparison_check
        CHECK (
            comparison IN (
                'numeric_exact',
                'numeric_tolerance',
                'exact_text',
                'boolean_exact'
            )
        ),

    CONSTRAINT learning_benchmark_path_check
        CHECK (
            length(trim(actual_path)) > 0
        ),

    CONSTRAINT learning_benchmark_tolerance_check
        CHECK (
            (
                comparison = 'numeric_tolerance'
                AND tolerance IS NOT NULL
                AND tolerance >= 0
            )
            OR
            (
                comparison <> 'numeric_tolerance'
                AND tolerance IS NULL
            )
        )
);

ALTER TABLE learning_verification_oracles
ADD COLUMN benchmark_id BIGINT
    REFERENCES learning_verification_benchmarks(id)
    ON DELETE RESTRICT;


CREATE OR REPLACE FUNCTION
prevent_learning_benchmark_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION
        'learning verification benchmarks are immutable';
END;
$$;


CREATE TRIGGER
learning_verification_benchmarks_immutable
BEFORE UPDATE OR DELETE
ON learning_verification_benchmarks
FOR EACH ROW
EXECUTE FUNCTION
prevent_learning_benchmark_mutation();
