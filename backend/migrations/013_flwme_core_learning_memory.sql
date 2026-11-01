CREATE TABLE flwme_core_learning_memory (
    id BIGSERIAL PRIMARY KEY,

    source_candidate_id BIGINT NOT NULL
        REFERENCES learning_promotion_candidates(id)
        ON DELETE RESTRICT,

    behavior_key TEXT NOT NULL,

    category TEXT NOT NULL,

    agent_role TEXT NOT NULL,

    behavior_text TEXT NOT NULL,

    behavior_metrics JSONB NOT NULL
        DEFAULT '{}'::jsonb,

    quality_score DOUBLE PRECISION NOT NULL,

    ethics_status TEXT NOT NULL
        DEFAULT 'approved',

    sanitization_version TEXT NOT NULL
        DEFAULT 'flwme.core.sanitize.v1',

    policy_version TEXT NOT NULL,

    index_status TEXT NOT NULL
        DEFAULT 'pending',

    indexed_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT now(),

    CONSTRAINT flwme_core_learning_candidate_unique
        UNIQUE (source_candidate_id),

    CONSTRAINT flwme_core_learning_behavior_key_valid
        CHECK (
            char_length(behavior_key)
            BETWEEN 3 AND 160
        ),

    CONSTRAINT flwme_core_learning_behavior_text_valid
        CHECK (
            char_length(behavior_text)
            BETWEEN 20 AND 4000
        ),

    CONSTRAINT flwme_core_learning_category_valid
        CHECK (
            category IN (
                'reasoning',
                'planning',
                'tool_use',
                'safety',
                'verification',
                'orchestration',
                'memory',
                'execution'
            )
        ),

    CONSTRAINT flwme_core_learning_agent_valid
        CHECK (
            agent_role IN (
                'CEO',
                'COO',
                'CFO',
                'CMO',
                'CTO',
                'Worker'
            )
        ),

    CONSTRAINT flwme_core_learning_score_valid
        CHECK (
            quality_score >= 0.0
            AND quality_score <= 1.0
        ),

    CONSTRAINT flwme_core_learning_ethics_valid
        CHECK (
            ethics_status = 'approved'
        ),

    CONSTRAINT flwme_core_learning_sanitize_valid
        CHECK (
            sanitization_version =
            'flwme.core.sanitize.v1'
        ),

    CONSTRAINT flwme_core_learning_index_valid
        CHECK (
            index_status IN (
                'pending',
                'indexed',
                'failed'
            )
        ),

    CONSTRAINT flwme_core_learning_metrics_object
        CHECK (
            jsonb_typeof(behavior_metrics) =
            'object'
        )
);

CREATE INDEX
    flwme_core_learning_behavior_idx
ON flwme_core_learning_memory (
    behavior_key
);

CREATE INDEX
    flwme_core_learning_category_idx
ON flwme_core_learning_memory (
    category,
    quality_score DESC
);

CREATE INDEX
    flwme_core_learning_index_status_idx
ON flwme_core_learning_memory (
    index_status,
    id
);


CREATE TABLE flwme_core_learning_jobs (
    id BIGSERIAL PRIMARY KEY,

    core_memory_id BIGINT NOT NULL
        REFERENCES flwme_core_learning_memory(id)
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

    CONSTRAINT flwme_core_learning_job_memory_unique
        UNIQUE (core_memory_id),

    CONSTRAINT flwme_core_learning_job_status_valid
        CHECK (
            status IN (
                'pending',
                'running',
                'done',
                'failed'
            )
        ),

    CONSTRAINT flwme_core_learning_job_attempts_valid
        CHECK (
            attempts >= 0
            AND attempts <= 10
        )
);

CREATE INDEX
    flwme_core_learning_jobs_pending_idx
ON flwme_core_learning_jobs (
    status,
    id
);


CREATE OR REPLACE FUNCTION
flwme_core_learning_candidate_guard()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    candidate_status TEXT;
    candidate_source TEXT;
    evidence_learning_status TEXT;
    evidence_verification_status TEXT;
    evidence_source_event TEXT;
    evaluation_status TEXT;
    evaluation_score DOUBLE PRECISION;
    evaluation_threshold DOUBLE PRECISION;
    verification_verdict TEXT;
    oracle_source TEXT;
BEGIN
    SELECT
        pc.status,
        pc.oracle_source_type,
        le.learning_status,
        le.verification_status,
        le.source_event,
        ev.status,
        ev.score,
        ev.threshold,
        v.verdict,
        o.source_type
    INTO
        candidate_status,
        candidate_source,
        evidence_learning_status,
        evidence_verification_status,
        evidence_source_event,
        evaluation_status,
        evaluation_score,
        evaluation_threshold,
        verification_verdict,
        oracle_source
    FROM learning_promotion_candidates pc
    JOIN learning_evidence le
      ON le.id = pc.evidence_id
    JOIN learning_evaluations ev
      ON ev.id = pc.evaluation_id
     AND ev.evidence_id = le.id
    JOIN learning_verifications v
      ON v.id = pc.verification_id
     AND v.evidence_id = le.id
    JOIN learning_verification_oracles o
      ON o.id = v.oracle_id
    WHERE pc.id = NEW.source_candidate_id;

    IF NOT FOUND THEN
        RAISE EXCEPTION
            'core_learning_candidate_not_found';
    END IF;

    IF candidate_status <> 'approved' THEN
        RAISE EXCEPTION
            'core_learning_candidate_not_approved';
    END IF;

    IF candidate_source <> 'database_verified'
       OR oracle_source <> 'database_verified'
    THEN
        RAISE EXCEPTION
            'core_learning_truth_source_not_verified';
    END IF;

    IF evidence_learning_status <> 'candidate'
       OR evidence_verification_status <> 'verified'
       OR verification_verdict <> 'verified'
    THEN
        RAISE EXCEPTION
            'core_learning_evidence_not_verified';
    END IF;

    IF evaluation_status <> 'passed'
       OR evaluation_score < evaluation_threshold
    THEN
        RAISE EXCEPTION
            'core_learning_evaluation_not_passed';
    END IF;

    IF evidence_source_event <>
       'executive_task_completion'
    THEN
        RAISE EXCEPTION
            'core_learning_source_event_not_allowed';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER
    flwme_core_learning_candidate_guard_trigger
BEFORE INSERT
ON flwme_core_learning_memory
FOR EACH ROW
EXECUTE FUNCTION
    flwme_core_learning_candidate_guard();
