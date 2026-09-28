-- Admission records own immutable selections, independently of removable binding heads.
-- Runtime leasing remains owned by background_tasks/task_attempts. No second queue is created.
CREATE TABLE workflow_runs (
    company_id uuid NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    id uuid NOT NULL,
    binding_id uuid NOT NULL,
    binding_revision bigint NOT NULL CHECK (binding_revision > 0),
    workflow_id uuid NOT NULL,
    version_id uuid NOT NULL,
    channel_id uuid,
    thread_id uuid,
    actor_id uuid NOT NULL,
    trigger_id uuid NOT NULL,
    correlation_id uuid NOT NULL,
    bundle bytea NOT NULL CHECK (octet_length(bundle) BETWEEN 1 AND 4194304),
    input jsonb NOT NULL CHECK (octet_length(input::text) <= 4194304),
    params jsonb NOT NULL CHECK (octet_length(params::text) <= 4194304),
    resources jsonb NOT NULL CHECK (jsonb_typeof(resources) = 'object'
        AND octet_length(resources::text) <= 262144),
    max_steps integer NOT NULL CHECK (max_steps BETWEEN 1 AND 100000),
    max_context_bytes integer NOT NULL CHECK (max_context_bytes BETWEEN 1 AND 1048576),
    deadline timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, id, binding_id),
    UNIQUE (company_id, id, channel_id, thread_id),
    CHECK (thread_id IS NULL OR channel_id IS NOT NULL),
    CHECK (deadline > created_at),
    FOREIGN KEY (company_id, workflow_id, version_id)
        REFERENCES workflow_versions(company_id, workflow_id, id),
    FOREIGN KEY (company_id, channel_id) REFERENCES channels(company_id, id),
    FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id)
);

-- Only identity is introduced here. Activation inputs/results, transition state and
-- attempt fencing are added by their runtime owner, not inferred from queue attempts.
CREATE TABLE workflow_executions (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    id uuid NOT NULL,
    step_id text NOT NULL CHECK (octet_length(step_id) BETWEEN 1 AND 128),
    activation bigint NOT NULL CHECK (activation > 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, run_id, id),
    UNIQUE (company_id, run_id, step_id, activation),
    FOREIGN KEY (company_id, run_id) REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE
);

CREATE TABLE workflow_waits (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    id uuid NOT NULL,
    reason text NOT NULL CHECK (reason IN ('decision', 'event', 'timer', 'child_run', 'effect', 'reconciliation')),
    deadline timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, id),
    UNIQUE (company_id, run_id, execution_id, id),
    CHECK (deadline > created_at),
    FOREIGN KEY (company_id, run_id, execution_id)
        REFERENCES workflow_executions(company_id, run_id, id) ON DELETE CASCADE
);

-- The canonical source key is a bounded, versioned encoding of TriggerSource IDs.
-- Transactional source authority is additionally required before insertion/replay.
-- Distinct command keys can alias this one binding/source admission.
CREATE TABLE workflow_admissions (
    company_id uuid NOT NULL,
    binding_id uuid NOT NULL,
    source_key text NOT NULL CHECK (octet_length(source_key) BETWEEN 1 AND 512),
    run_id uuid NOT NULL,
    PRIMARY KEY (company_id, binding_id, source_key),
    UNIQUE (company_id, run_id),
    FOREIGN KEY (company_id, run_id, binding_id)
        REFERENCES workflow_runs(company_id, id, binding_id) ON DELETE CASCADE
);

CREATE TABLE workflow_admission_commands (
    company_id uuid NOT NULL,
    command_key text NOT NULL CHECK (octet_length(command_key) BETWEEN 1 AND 128),
    run_id uuid NOT NULL,
    trigger_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, command_key),
    FOREIGN KEY (company_id, run_id) REFERENCES workflow_admissions(company_id, run_id)
        ON DELETE CASCADE
);

-- Membership is pinned, not selected again by timestamp. Composite keys prove the
-- pinned message and run belong to the exact same company/channel/thread.
ALTER TABLE thread_messages ADD CONSTRAINT thread_messages_workflow_history_key
    UNIQUE (company_id, channel_id, thread_id, message_id);
CREATE TABLE workflow_history_members (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    message_id uuid NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal BETWEEN 1 AND 10000),
    PRIMARY KEY (company_id, run_id, message_id),
    UNIQUE (company_id, run_id, ordinal),
    FOREIGN KEY (company_id, run_id, channel_id, thread_id)
        REFERENCES workflow_runs(company_id, id, channel_id, thread_id) ON DELETE CASCADE,
    FOREIGN KEY (company_id, channel_id, thread_id, message_id)
        REFERENCES thread_messages(company_id, channel_id, thread_id, message_id)
);

CREATE TABLE workflow_run_events (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    sequence bigint NOT NULL CHECK (sequence > 0),
    event_kind text NOT NULL CHECK (octet_length(event_kind) BETWEEN 1 AND 128),
    actor_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, run_id, sequence),
    FOREIGN KEY (company_id, run_id) REFERENCES workflow_runs(company_id, id) ON DELETE CASCADE
);
