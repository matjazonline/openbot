-- Initial schema. This migration intentionally targets a newly created database.

CREATE EXTENSION citext;

CREATE TABLE users (
    id UUID PRIMARY KEY,
    username CITEXT NOT NULL UNIQUE,
    email CITEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT users_username_not_blank CHECK (btrim(username::text) <> ''),
    CONSTRAINT users_email_not_blank CHECK (btrim(email::text) <> '')
);

CREATE TABLE companies (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    slug CITEXT NOT NULL UNIQUE,
    api_key TEXT,
    provider TEXT,
    model TEXT,
    enable_llm_spam_guardrail BOOLEAN,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT companies_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT companies_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$'
    )
);

CREATE INDEX companies_user_created_idx
    ON companies (user_id, created_at DESC, id DESC);

CREATE TABLE company_invites (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT company_invites_company_email_key UNIQUE (company_id, email),
    CONSTRAINT company_invites_status_check
        CHECK (status IN ('pending', 'accepted', 'declined'))
);

CREATE INDEX company_invites_company_created_idx
    ON company_invites (company_id, created_at DESC, id DESC);
CREATE INDEX company_invites_email_created_idx
    ON company_invites (email, created_at DESC, id DESC);

CREATE TABLE company_members (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'member',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT company_members_company_user_key UNIQUE (company_id, user_id),
    CONSTRAINT company_members_role_check CHECK (role IN ('member', 'admin'))
);

CREATE INDEX company_members_user_company_idx ON company_members (user_id, company_id);
CREATE INDEX company_members_company_created_idx
    ON company_members (company_id, created_at, id);

CREATE TABLE agents (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug CITEXT NOT NULL,
    provider TEXT,
    model TEXT,
    api_key TEXT,
    system_prompt TEXT,
    config_json JSONB,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT agents_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT agents_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT agents_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT agents_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT agents_config_object_check CHECK (
        config_json IS NULL OR jsonb_typeof(config_json) = 'object'
    )
);

CREATE INDEX agents_company_created_idx
    ON agents (company_id, created_at DESC, id DESC);

CREATE TABLE channels (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug CITEXT NOT NULL,
    access_mode TEXT NOT NULL DEFAULT 'team',
    channel_config JSONB,
    api_key TEXT,
    provider TEXT,
    model TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT channels_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT channels_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT channels_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT channels_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT channels_access_mode_check
        CHECK (access_mode IN ('team', 'allowlist', 'public')),
    CONSTRAINT channels_config_object_check CHECK (
        channel_config IS NULL OR jsonb_typeof(channel_config) = 'object'
    )
);

CREATE INDEX channels_company_created_idx
    ON channels (company_id, created_at DESC, id DESC);

CREATE TABLE channel_agents (
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    agent_id UUID NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (channel_id, agent_id),
    CONSTRAINT channel_agents_channel_position_key UNIQUE (channel_id, position),
    CONSTRAINT channel_agents_position_check CHECK (position >= 0),
    CONSTRAINT channel_agents_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT channel_agents_agent_fk
        FOREIGN KEY (company_id, agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE
);

CREATE INDEX channel_agents_agent_idx ON channel_agents (agent_id, channel_id);

CREATE TABLE channel_participants (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, email)
);

CREATE INDEX channel_participants_email_idx
    ON channel_participants (email, channel_id);

CREATE TABLE threads (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    subject TEXT NOT NULL,
    external_thread_key TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT threads_company_channel_id_key UNIQUE (company_id, channel_id, id),
    CONSTRAINT threads_channel_id_key UNIQUE (channel_id, id),
    CONSTRAINT threads_channel_external_key UNIQUE (channel_id, external_thread_key),
    CONSTRAINT threads_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE
);

CREATE INDEX threads_channel_updated_idx
    ON threads (channel_id, updated_at DESC, id DESC);

CREATE TABLE thread_participants (
    thread_id UUID NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (thread_id, email)
);

CREATE INDEX thread_participants_email_idx
    ON thread_participants (email, thread_id);

CREATE TABLE email_messages (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    content_hash BYTEA NOT NULL,
    in_reply_to TEXT,
    references_list TEXT[] NOT NULL DEFAULT '{}',
    sender CITEXT NOT NULL,
    recipients_to TEXT[] NOT NULL DEFAULT '{}',
    recipients_cc TEXT[] NOT NULL DEFAULT '{}',
    subject TEXT NOT NULL,
    raw_text_body TEXT,
    raw_html_body TEXT,
    attachments JSONB,
    thread_index TEXT,
    received_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT email_messages_company_message_key UNIQUE (company_id, message_id),
    CONSTRAINT email_messages_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT email_messages_attachments_array_check CHECK (
        attachments IS NULL OR jsonb_typeof(attachments) = 'array'
    )
);

CREATE INDEX email_messages_in_reply_to_idx
    ON email_messages (in_reply_to) WHERE in_reply_to IS NOT NULL;
CREATE TABLE thread_messages (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    email_message_id UUID NOT NULL,
    clean_text_body TEXT NOT NULL,
    direction TEXT NOT NULL,
    role TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT thread_messages_thread_email_key UNIQUE (thread_id, email_message_id),
    CONSTRAINT thread_messages_channel_email_key UNIQUE (channel_id, email_message_id),
    CONSTRAINT thread_messages_thread_fk
        FOREIGN KEY (channel_id, thread_id)
        REFERENCES threads(channel_id, id) ON DELETE CASCADE,
    CONSTRAINT thread_messages_email_fk
        FOREIGN KEY (company_id, email_message_id)
        REFERENCES email_messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT thread_messages_direction_check CHECK (direction IN ('inbound', 'outbound')),
    CONSTRAINT thread_messages_role_check CHECK (role IN ('human', 'agent', 'system'))
);

CREATE INDEX thread_messages_thread_created_idx
    ON thread_messages (thread_id, created_at, id);
CREATE INDEX thread_messages_email_thread_idx
    ON thread_messages (email_message_id, thread_id);
CREATE INDEX thread_messages_outbound_thread_idx
    ON thread_messages (thread_id, email_message_id, created_at DESC)
    WHERE direction = 'outbound';

CREATE FUNCTION delete_orphan_email_message() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM email_messages em
    WHERE em.id = OLD.email_message_id
      AND NOT EXISTS (
          SELECT 1 FROM thread_messages tm
          WHERE tm.email_message_id = OLD.email_message_id
      );
    RETURN NULL;
END;
$$;

CREATE TRIGGER thread_messages_delete_orphan_email
AFTER DELETE ON thread_messages
FOR EACH ROW EXECUTE FUNCTION delete_orphan_email_message();

CREATE TABLE background_tasks (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL,
    thread_id UUID,
    source_message_id TEXT,
    task_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload JSONB NOT NULL DEFAULT '{}',
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    last_error TEXT,
    worker_id UUID,
    locked_at TIMESTAMP,
    lock_expires_at TIMESTAMP,
    wait_expires_at TIMESTAMP,
    run_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT background_tasks_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT background_tasks_company_source_key UNIQUE (company_id, source_message_id),
    CONSTRAINT background_tasks_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT background_tasks_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE SET NULL (thread_id),
    CONSTRAINT background_tasks_status_check CHECK (status IN (
        'pending', 'processing', 'pending_approval',
        'waiting_for_third_party_reply', 'completed', 'failed',
        'dead_letter', 'stopped'
    )),
    CONSTRAINT background_tasks_retry_count_check CHECK (retry_count >= 0),
    CONSTRAINT background_tasks_max_retries_check CHECK (max_retries > 0),
    CONSTRAINT background_tasks_payload_object_check CHECK (jsonb_typeof(payload) = 'object'),
    CONSTRAINT background_tasks_lease_check CHECK (
        (worker_id IS NULL AND locked_at IS NULL AND lock_expires_at IS NULL)
        OR (worker_id IS NOT NULL AND locked_at IS NOT NULL AND lock_expires_at IS NOT NULL
            AND lock_expires_at > locked_at)
    )
);

CREATE INDEX background_tasks_pending_ready_idx
    ON background_tasks (run_at, created_at, id)
    WHERE status = 'pending';
CREATE INDEX background_tasks_processing_lease_idx
    ON background_tasks (lock_expires_at, id)
    WHERE status = 'processing';
CREATE INDEX background_tasks_company_created_idx
    ON background_tasks (company_id, created_at DESC, id DESC);
CREATE INDEX background_tasks_company_channel_created_idx
    ON background_tasks (company_id, channel_id, created_at DESC, id DESC);
CREATE INDEX background_tasks_company_status_created_idx
    ON background_tasks (company_id, status, created_at DESC, id DESC);
CREATE INDEX background_tasks_thread_idx
    ON background_tasks (thread_id) WHERE thread_id IS NOT NULL;
CREATE INDEX background_tasks_waiting_due_idx
    ON background_tasks (wait_expires_at, id)
    WHERE status = 'waiting_for_third_party_reply' AND wait_expires_at IS NOT NULL;

CREATE TABLE task_channel_targets (
    task_id UUID NOT NULL,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    recipient_role TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (task_id, position),
    CONSTRAINT task_channel_targets_channel_key UNIQUE (task_id, channel_id),
    CONSTRAINT task_channel_targets_position_check CHECK (position >= 0),
    CONSTRAINT task_channel_targets_role_check CHECK (recipient_role IN ('to', 'cc')),
    CONSTRAINT task_channel_targets_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_channel_targets_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE
);

CREATE INDEX task_channel_targets_channel_task_idx
    ON task_channel_targets (company_id, channel_id, task_id);
CREATE INDEX task_channel_targets_thread_idx
    ON task_channel_targets (company_id, channel_id, thread_id);

CREATE FUNCTION delete_channel_target_tasks() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM background_tasks task
    WHERE EXISTS (
        SELECT 1 FROM task_channel_targets target
        WHERE target.task_id = task.id AND target.channel_id = OLD.id
    );
    RETURN OLD;
END;
$$;

CREATE TRIGGER channels_delete_target_tasks
BEFORE DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION delete_channel_target_tasks();

CREATE TABLE task_outreaches (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES background_tasks(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    required_threshold_percent NUMERIC(5,2),
    expires_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT task_outreaches_kind_check CHECK (kind IN ('delegation', 'quorum')),
    CONSTRAINT task_outreaches_status_check CHECK (
        status IN ('waiting', 'quorum_met', 'timed_out', 'cancelled', 'completed')
    ),
    CONSTRAINT task_outreaches_threshold_check CHECK (
        required_threshold_percent IS NULL
        OR required_threshold_percent BETWEEN 0 AND 100
    )
);

CREATE INDEX task_outreaches_due_idx
    ON task_outreaches (expires_at, id)
    WHERE status = 'waiting' AND expires_at IS NOT NULL;
CREATE INDEX task_outreaches_task_idx ON task_outreaches (task_id);

CREATE TABLE task_outreach_targets (
    outreach_id UUID NOT NULL REFERENCES task_outreaches(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    responded_at TIMESTAMP,
    response_message_id UUID REFERENCES email_messages(id) ON DELETE SET NULL,
    PRIMARY KEY (outreach_id, email)
);

CREATE INDEX task_outreach_targets_email_waiting_idx
    ON task_outreach_targets (email, outreach_id) WHERE responded_at IS NULL;
CREATE INDEX task_outreach_targets_response_message_idx
    ON task_outreach_targets (response_message_id)
    WHERE response_message_id IS NOT NULL;

CREATE TABLE task_attempts (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES background_tasks(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    result JSONB,
    started_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TIMESTAMP,
    CONSTRAINT task_attempts_task_attempt_key UNIQUE (task_id, attempt_number),
    CONSTRAINT task_attempts_status_check CHECK (status IN ('processing', 'completed', 'failed')),
    CONSTRAINT task_attempts_token_check CHECK (
        (prompt_tokens IS NULL OR prompt_tokens >= 0)
        AND (completion_tokens IS NULL OR completion_tokens >= 0)
    )
);

CREATE TABLE human_approvals (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID,
    task_id UUID,
    step_key TEXT NOT NULL,
    approver_email CITEXT NOT NULL,
    action_type TEXT NOT NULL,
    action_title TEXT NOT NULL,
    action_summary TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    token UUID NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT human_approvals_thread_step_key UNIQUE NULLS NOT DISTINCT
        (company_id, channel_id, thread_id, step_key),
    CONSTRAINT human_approvals_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT human_approvals_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT human_approvals_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT human_approvals_status_check
        CHECK (status IN ('pending', 'approved', 'rejected', 'expired')),
    CONSTRAINT human_approvals_expiry_check CHECK (expires_at > created_at),
    CONSTRAINT human_approvals_payload_object_check CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX human_approvals_channel_created_idx
    ON human_approvals (company_id, channel_id, created_at DESC, id DESC);
CREATE INDEX human_approvals_pending_expiry_idx
    ON human_approvals (expires_at, id) WHERE status = 'pending';
CREATE INDEX human_approvals_task_idx
    ON human_approvals (task_id) WHERE task_id IS NOT NULL;

CREATE TABLE email_outbox (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    task_id UUID,
    idempotency_key TEXT NOT NULL UNIQUE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    provider_message_id TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    worker_id UUID,
    locked_at TIMESTAMP,
    lock_expires_at TIMESTAMP,
    available_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    sent_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT email_outbox_status_check CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
    CONSTRAINT email_outbox_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE SET NULL (task_id),
    CONSTRAINT email_outbox_retry_check CHECK (retry_count >= 0),
    CONSTRAINT email_outbox_payload_object_check CHECK (jsonb_typeof(payload) = 'object')
);

CREATE INDEX email_outbox_pending_idx
    ON email_outbox (available_at, id) WHERE status = 'pending';
CREATE INDEX email_outbox_sending_lease_idx
    ON email_outbox (lock_expires_at, id) WHERE status = 'sending';
CREATE INDEX email_outbox_company_created_idx
    ON email_outbox (company_id, created_at DESC, id DESC);
CREATE INDEX email_outbox_task_idx
    ON email_outbox (task_id) WHERE task_id IS NOT NULL;
