-- Initial schema. This migration intentionally targets a newly created database.
--
-- Squashed from the incremental migrations that preceded it; it describes the schema's current
-- state, not the order it was arrived at.

CREATE EXTENSION citext;

CREATE TABLE users (
    id UUID PRIMARY KEY,
    username CITEXT NOT NULL UNIQUE,
    email CITEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Written from a form field and read straight into an `<img src>`, so the one scheme rule the
    -- renderer relies on is enforced where it cannot be bypassed.
    avatar_url TEXT,
    CONSTRAINT users_username_not_blank CHECK (btrim(username::text) <> ''),
    CONSTRAINT users_email_not_blank CHECK (btrim(email::text) <> ''),
    CONSTRAINT users_avatar_url_scheme_check
        CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://')
);

-- A registration waiting on a code mailed to the address it claims. An account only exists in
-- `users` once that code comes back, so an unconfirmed address is never one anyone can sign in as.
CREATE TABLE pending_user_registrations (
    email CITEXT PRIMARY KEY,
    username CITEXT NOT NULL,
    password_hash TEXT NOT NULL,
    confirmation_code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT pending_user_registrations_username_not_blank CHECK (btrim(username::text) <> ''),
    CONSTRAINT pending_user_registrations_email_not_blank CHECK (btrim(email::text) <> '')
);

CREATE UNIQUE INDEX pending_user_registrations_username_key
    ON pending_user_registrations (username);

-- A change to an account that is waiting on a code mailed out to prove it was really asked for.
--
-- The two kinds prove different things and so mail the code to different places: an email change
-- sends it to the *new* address (proving the account owner can read it), a password change sends
-- it to the address the account already has. That is why the new address lives here rather than
-- being written to `users` and confirmed in place -- an unconfirmed address must never be one the
-- account can sign in or receive mail as.
CREATE TABLE pending_account_changes (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    -- Set for a 'email' change and null for a 'password' one, and vice versa: the CHECK below is
    -- what keeps a row from claiming to be one kind while carrying the other's payload.
    new_email CITEXT,
    new_password_hash TEXT,
    confirmation_code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- One pending change of each kind per account. Asking again replaces the earlier request, so
    -- an abandoned code cannot still be confirmed after a second one was sent.
    PRIMARY KEY (user_id, kind),
    CONSTRAINT pending_account_changes_kind_check CHECK (kind IN ('email', 'password')),
    CONSTRAINT pending_account_changes_payload_matches_kind CHECK (
        (kind = 'email' AND new_email IS NOT NULL AND new_password_hash IS NULL)
        OR (kind = 'password' AND new_password_hash IS NOT NULL AND new_email IS NULL)
    ),
    CONSTRAINT pending_account_changes_email_not_blank
        CHECK (new_email IS NULL OR btrim(new_email::text) <> '')
);

-- Authentication methods are explicit: finding the same email through another provider must not
-- silently turn that provider into a way into the account.
CREATE TABLE user_login_methods (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_subject TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, provider),
    CONSTRAINT user_login_methods_provider_check
        CHECK (provider IN ('password', 'google', 'apple')),
    CONSTRAINT user_login_methods_subject_check CHECK (
        (provider = 'password' AND provider_subject IS NULL)
        OR (provider IN ('google', 'apple')
            AND provider_subject IS NOT NULL
            AND btrim(provider_subject) <> '')
    )
);

CREATE UNIQUE INDEX user_login_methods_provider_subject_key
    ON user_login_methods (provider, provider_subject)
    WHERE provider_subject IS NOT NULL;

-- The shape every `created_by` column carries. Written once as a function rather than repeated as
-- a CHECK body per table, so "what provenance looks like" has one definition: an actor kind, the
-- id that kind implies (a system actor has none, an agent additionally carries the channel and
-- task it acted from), a non-blank name, and no keys beyond those.
CREATE FUNCTION valid_creation_provenance(provenance JSONB) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN
    jsonb_typeof(provenance) = 'object'
    AND (provenance - ARRAY[
        'actor_type', 'actor_id', 'actor_name', 'source_channel_id', 'source_task_id'
    ]::TEXT[]) = '{}'::JSONB
    AND jsonb_typeof(provenance->'actor_type') = 'string'
    AND provenance->>'actor_type' IN ('user', 'agent', 'system')
    AND jsonb_typeof(provenance->'actor_name') = 'string'
    AND btrim(provenance->>'actor_name') <> ''
    AND CASE provenance->>'actor_type'
        WHEN 'system' THEN
            provenance ? 'actor_id'
            AND provenance->'actor_id' = 'null'::JSONB
            AND COALESCE(provenance->'source_channel_id' = 'null'::JSONB, true)
            AND COALESCE(provenance->'source_task_id' = 'null'::JSONB, true)
        WHEN 'user' THEN
            jsonb_typeof(provenance->'actor_id') = 'string'
            AND provenance->>'actor_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND COALESCE(provenance->'source_channel_id' = 'null'::JSONB, true)
            AND COALESCE(provenance->'source_task_id' = 'null'::JSONB, true)
        WHEN 'agent' THEN
            jsonb_typeof(provenance->'actor_id') = 'string'
            AND provenance->>'actor_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND jsonb_typeof(provenance->'source_channel_id') = 'string'
            AND provenance->>'source_channel_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
            AND jsonb_typeof(provenance->'source_task_id') = 'string'
            AND provenance->>'source_task_id' ~* '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
        ELSE false
    END;

CREATE TABLE companies (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    slug CITEXT NOT NULL UNIQUE,
    enable_llm_spam_guardrail BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- A company gets a picture of its own, on the same terms as a user's or an agent's: an
    -- http(s) URL or nothing, so what a page renders into an `<img src>` can never be an active
    -- scheme.
    avatar_url TEXT,
    -- NULL means the company keeps no memory at all. There is no 'none' sentinel: two ways to
    -- write "off" is one way for a query to miss half the companies that have it off.
    memory_provider TEXT,
    CONSTRAINT companies_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT companies_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$'
    ),
    CONSTRAINT companies_avatar_url_scheme_check
        CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://'),
    CONSTRAINT companies_memory_provider_check
        CHECK (memory_provider IS NULL OR memory_provider IN ('hydradb', 'hindsight'))
);

CREATE INDEX companies_user_created_idx
    ON companies (user_id, created_at DESC, id DESC);

-- One credential per provider per company, plus the exact models that company's agents may
-- select. Credentials live here and nowhere else: an agent picks a provider and a model, never a
-- key, so a leaked agent or channel row carries nothing usable.
CREATE TABLE company_model_connections (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    -- Stored in the envelope form `enc:v1:<key version>:<ciphertext>`; the key version travels
    -- inside the envelope, so there is no separate column to keep in step with it.
    api_key TEXT NOT NULL,
    models TEXT[] NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, provider),
    CONSTRAINT company_model_connections_provider_check CHECK (
        provider IN ('google', 'openai', 'anthropic', 'groq')
        AND length(provider) <= 64
    ),
    CONSTRAINT company_model_connections_api_key_check CHECK (
        btrim(api_key) <> '' AND octet_length(api_key) <= 16384
    ),
    CONSTRAINT company_model_connections_models_count_check CHECK (
        cardinality(models) BETWEEN 1 AND 32
    ),
    CONSTRAINT company_model_connections_models_have_no_nulls CHECK (
        array_position(models, NULL) IS NULL AND array_position(models, '') IS NULL
    )
);

-- At most one default per company, so "which provider does an agent inherit" has one answer.
CREATE UNIQUE INDEX company_model_connections_one_default_idx
    ON company_model_connections (company_id)
    WHERE is_default;

CREATE TABLE company_invites (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- The role the invite grants on acceptance, so an admin invite does not have to be re-granted
    -- as a second step after the member row exists.
    role TEXT NOT NULL DEFAULT 'member',
    CONSTRAINT company_invites_company_email_key UNIQUE (company_id, email),
    CONSTRAINT company_invites_status_check
        CHECK (status IN ('pending', 'accepted', 'declined')),
    CONSTRAINT company_invites_role_check CHECK (role IN ('member', 'admin'))
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT company_members_company_user_key UNIQUE (company_id, user_id),
    CONSTRAINT company_members_role_check CHECK (role IN ('member', 'admin'))
);

CREATE INDEX company_members_user_company_idx ON company_members (user_id, company_id);
CREATE INDEX company_members_company_created_idx
    ON company_members (company_id, created_at, id);

-- A NULL `company_id` is an operator-managed global library agent: visible to every company,
-- owned by none.
CREATE TABLE agents (
    id UUID PRIMARY KEY,
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug CITEXT NOT NULL,
    provider TEXT,
    model TEXT,
    system_prompt TEXT,
    -- What this agent is for, in one line. Read by the `list_company_agents` tool so a sibling
    -- agent can pick the right colleague without its address book living in a system prompt.
    description TEXT,
    config_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    avatar_url TEXT,
    created_by JSONB NOT NULL,
    -- Wall-clock budget for a single agent run. NULL leaves the runner's own default in place.
    run_timeout_secs INTEGER,
    memory_recall_mode TEXT NOT NULL DEFAULT 'fast'
        CHECK (memory_recall_mode IN ('fast', 'thinking')),
    memory_max_results SMALLINT NOT NULL DEFAULT 5
        CHECK (memory_max_results BETWEEN 1 AND 20),
    memory_persistence_mode TEXT NOT NULL DEFAULT 'audience_only'
        CHECK (memory_persistence_mode IN ('audience_only', 'scope_specific_facts')),
    CONSTRAINT agents_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT agents_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT agents_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT agents_slug_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    ),
    CONSTRAINT agents_config_object_check CHECK (
        config_json IS NULL OR jsonb_typeof(config_json) = 'object'
    ),
    CONSTRAINT agents_avatar_url_scheme_check
        CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://'),
    CONSTRAINT agents_created_by_shape_check CHECK (valid_creation_provenance(created_by)),
    CONSTRAINT agents_run_timeout_secs_check
        CHECK (run_timeout_secs BETWEEN 1 AND 3600)
);

CREATE INDEX agents_company_created_idx
    ON agents (company_id, created_at DESC, id DESC);

-- `agents_company_slug_key` does not constrain library agents: UNIQUE treats every NULL
-- `company_id` as distinct, so the library needs its own uniqueness over slug alone.
CREATE UNIQUE INDEX agents_library_slug_key
    ON agents (slug) WHERE company_id IS NULL;

-- Deleting a company still cascades through its channels and its own agents. Only global library
-- definitions need an in-use deletion guard, since nothing cascades them away.
CREATE FUNCTION prevent_assigned_library_agent_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.company_id IS NULL
       AND EXISTS (SELECT 1 FROM channel_agents WHERE agent_id = OLD.id) THEN
        RAISE EXCEPTION 'library agent is assigned to one or more channels'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER library_agent_delete_guard
BEFORE DELETE ON agents
FOR EACH ROW EXECUTE FUNCTION prevent_assigned_library_agent_delete();

-- A channel's addresses live in `channel_slugs`, not here; see that table.
CREATE TABLE channels (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    access_mode TEXT NOT NULL DEFAULT 'team',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- A reversible off switch: disabling stops the channel taking traffic without deleting its
    -- threads, tasks and approvals the way DELETE FROM channels does.
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    -- Whether a trusted sender may pull CC'd outsiders onto this channel's threads. Off means the
    -- channel is internal: outsiders never join a thread and never appear on an agent reply's Cc.
    add_3rd_party BOOLEAN NOT NULL DEFAULT TRUE,
    created_by JSONB NOT NULL,
    -- Memory is opt-in per scope and per direction: reading someone's memory into a prompt and
    -- writing a turn back out to it are separate grants, so a channel can recall without
    -- recording.
    retrieve_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    retrieve_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    retrieve_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    persist_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    persist_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    persist_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    -- What this channel is for, in one line. Read back to a teammate who mails an address that
    -- does not exist, so they can find the channel they meant without asking anyone.
    description TEXT,
    CONSTRAINT channels_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT channels_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT channels_access_mode_check
        CHECK (access_mode IN ('team', 'allowlist', 'public')),
    CONSTRAINT channels_created_by_shape_check CHECK (valid_creation_provenance(created_by))
);

CREATE INDEX channels_company_created_idx
    ON channels (company_id, created_at DESC, id DESC);

-- The whole per-company channel address namespace in one table, so a channel can answer on more
-- than one local part. Canonical slug and aliases share a single UNIQUE (company_id, slug), which
-- is what makes canonical-vs-alias collisions impossible without a trigger or a racy app check.
CREATE TABLE channel_slugs (
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    slug CITEXT NOT NULL,
    is_primary BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (channel_id, slug),
    CONSTRAINT channel_slugs_company_slug_key UNIQUE (company_id, slug),
    CONSTRAINT channel_slugs_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT channel_slugs_format CHECK (
        slug::text = lower(slug::text)
        AND slug::text ~ '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$'
    )
);

-- Exactly one canonical slug per channel; aliases are unlimited.
CREATE UNIQUE INDEX channel_slugs_primary_idx ON channel_slugs (channel_id) WHERE is_primary;

-- The agent FK is on `agent_id` alone, not the compound (company_id, agent_id), because a library
-- agent has no company to match. `channel_agents_scope_check` below is what replaces the tenancy
-- the compound key used to carry.
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
        FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

CREATE INDEX channel_agents_agent_idx ON channel_agents (agent_id, channel_id);

CREATE FUNCTION enforce_channel_agent_scope() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM agents AS agent
        WHERE agent.id = NEW.agent_id
          AND (agent.company_id IS NULL OR agent.company_id = NEW.company_id)
    ) THEN
        RAISE EXCEPTION 'agent must belong to the channel company or the global library'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER channel_agents_scope_check
BEFORE INSERT OR UPDATE ON channel_agents
FOR EACH ROW EXECUTE FUNCTION enforce_channel_agent_scope();

CREATE FUNCTION enforce_enabled_channel_has_active_agent() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    checked_channel_id UUID;
BEGIN
    IF TG_TABLE_NAME = 'channels' THEN
        checked_channel_id := COALESCE(NEW.id, OLD.id);
    ELSE
        checked_channel_id := COALESCE(NEW.channel_id, OLD.channel_id);
    END IF;
    IF EXISTS (
        SELECT 1 FROM channels AS channel
        WHERE channel.id = checked_channel_id AND channel.enabled
    ) AND NOT EXISTS (
        SELECT 1 FROM channel_agents AS assignment
        WHERE assignment.channel_id = checked_channel_id AND assignment.position = 0
    ) THEN
        RAISE EXCEPTION 'enabled channel must have an active agent at position 0'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER enabled_channel_active_agent_check
AFTER INSERT OR UPDATE OF enabled ON channels
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_enabled_channel_has_active_agent();

CREATE CONSTRAINT TRIGGER channel_assignment_active_agent_check
AFTER INSERT OR UPDATE OR DELETE ON channel_agents
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_enabled_channel_has_active_agent();

CREATE TABLE channel_participants (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    received_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT thread_messages_thread_email_key UNIQUE (thread_id, email_message_id),
    CONSTRAINT thread_messages_channel_email_key UNIQUE (channel_id, email_message_id),
    -- All three tenancy columns are in the key, so a message cannot name a thread that belongs to
    -- another company or another channel than the one it recorded.
    CONSTRAINT thread_messages_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
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

-- Announce every persisted message so open `/ui` mailboxes can append it live.
--
-- This lives in a trigger rather than in the Rust writer for three reasons: every writer is
-- covered, including ones added later; the notification is bound to the same transaction as the
-- row, so it is delivered only on commit and never announces a message a reader cannot yet see;
-- and `create_message` does not have `company_id` in hand -- it derives it in SQL from `threads`.
--
-- The payload carries identifiers only. `pg_notify` caps payloads at 8000 bytes, and listeners
-- re-query for the message body anyway so that a reader resuming after a dropped connection takes
-- the same path as one receiving a live message.
CREATE FUNCTION notify_thread_message() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify(
        'thread_message',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

-- INSERT only: `create_message` upserts, but the UPDATE branch just rewrites the body of a message
-- that was already announced, and re-announcing it would append a duplicate bubble.
CREATE TRIGGER thread_messages_notify
    AFTER INSERT ON thread_messages
    FOR EACH ROW
    EXECUTE FUNCTION notify_thread_message();

CREATE TABLE background_tasks (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL,
    thread_id UUID,
    source_message_id TEXT,
    -- The inbound event this task descends from, minted once at ingress and inherited by every
    -- task the run goes on to spawn (an outreach in another channel, an approval resume, a
    -- schedule's next occurrence). Never re-minted here: the `ON CONFLICT` below returns the
    -- task a redelivered message already has, correlation id and all, so a duplicate delivery
    -- joins the original chain instead of starting a second one.
    correlation_id UUID NOT NULL,
    task_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload JSONB NOT NULL DEFAULT '{}',
    retry_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    last_error TEXT,
    worker_id UUID,
    -- Fences one execution against the next. `worker_id` alone cannot: a worker whose lease
    -- lapsed and which then re-claims the same task would match its own stale guard. The
    -- generation is minted afresh at every claim, so a write from a superseded run matches
    -- nothing. `schedule_runs.materialization_generation` does the same job for that queue.
    execution_generation UUID,
    locked_at TIMESTAMPTZ,
    lock_expires_at TIMESTAMPTZ,
    wait_expires_at TIMESTAMPTZ,
    run_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Why the row's current status was written, and by whom. Carried on the row rather than in
    -- transaction-local settings so that a status change and its attribution are the same write:
    -- one statement, one round trip, and no pooled-session state for a later query to inherit.
    -- Every status-changing UPDATE sets all five, binding NULL where a value is absent -- a column
    -- left out of a SET list would keep the previous transition's value, which the trigger below
    -- cannot tell from deliberate reuse. An INSERT leaves all five NULL: a new row has no prior
    -- transition, and `enqueued` is derivable without being told.
    --
    -- These describe the latest intended transition, not history: `task_status_events` is the
    -- ledger. They are deliberately unindexed -- nothing queries by them.
    transition_reason TEXT,
    transition_actor_kind TEXT,
    transition_actor_id UUID,
    transition_approval_id UUID,
    transition_outreach_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    -- Only a processing row may hold a lease, and it must hold all of it. Previously this said
    -- merely "all four set or all four null", which let a completed or suspended row keep the
    -- worker that last touched it and made "is this task claimed?" ambiguous. Clearing was left
    -- to each UPDATE getting it right; now the database refuses the alternative.
    CONSTRAINT background_tasks_lease_check CHECK (
        (status = 'processing'
         AND worker_id IS NOT NULL
         AND execution_generation IS NOT NULL
         AND locked_at IS NOT NULL
         AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'processing'
         AND worker_id IS NULL
         AND execution_generation IS NULL
         AND locked_at IS NULL
         AND lock_expires_at IS NULL)
    ),
    CONSTRAINT background_tasks_transition_reason_check CHECK (
        transition_reason IS NULL OR transition_reason IN (
            'enqueued', 'claimed', 'completed',
            'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
            'lease_lost', 'approval_requested', 'approval_accepted', 'approval_rejected',
            'outreach_started', 'outreach_reply_received', 'outreach_timed_out',
            'outreach_extended', 'operator_stopped', 'operator_resumed', 'unknown'
        )
    ),
    CONSTRAINT background_tasks_transition_actor_kind_check CHECK (
        transition_actor_kind IS NULL OR transition_actor_kind IN (
            'system', 'worker', 'operator', 'approval', 'outreach'
        )
    ),
    -- An actor kind names exactly one shape, and the shape is what the ledger reads. `worker` and
    -- `operator` are identified by `transition_actor_id`; `approval` and `outreach` are identified
    -- by the row that caused the transition; `system` is identified by nothing. Stating a kind
    -- without its id, or two sources at once, is the corruption this table refuses to store.
    CONSTRAINT background_tasks_transition_shape_check CHECK (
        (transition_reason IS NULL
         AND transition_actor_kind IS NULL
         AND transition_actor_id IS NULL
         AND transition_approval_id IS NULL
         AND transition_outreach_id IS NULL)
        OR
        (transition_reason IS NOT NULL
         AND CASE transition_actor_kind
             WHEN 'system' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'worker' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'operator' THEN transition_actor_id IS NOT NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NULL
             WHEN 'approval' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NOT NULL AND transition_outreach_id IS NULL
             WHEN 'outreach' THEN transition_actor_id IS NULL
                 AND transition_approval_id IS NULL AND transition_outreach_id IS NOT NULL
             ELSE FALSE
         END)
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
-- The whole-chain lookup: every task one inbound event caused, in the order it caused them.
CREATE INDEX background_tasks_correlation_idx
    ON background_tasks (correlation_id, created_at);

CREATE INDEX background_tasks_thread_idx
    ON background_tasks (thread_id) WHERE thread_id IS NOT NULL;
CREATE INDEX background_tasks_waiting_due_idx
    ON background_tasks (wait_expires_at, id)
    WHERE status = 'waiting_for_third_party_reply' AND wait_expires_at IS NOT NULL;
-- The Kanban board selects chains that are unfinished *or* touched recently. The unfinished arm is
-- served by background_tasks_company_status_created_idx; this is the recency arm, so the two
-- resolve as a BitmapOr instead of a full scan of every task the company has ever run.
CREATE INDEX background_tasks_company_updated_idx
    ON background_tasks (company_id, updated_at DESC);

CREATE TABLE agent_channel_provisions (
    task_id UUID NOT NULL REFERENCES background_tasks(id) ON DELETE CASCADE,
    request_hash TEXT NOT NULL,
    agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (task_id, request_hash)
);

-- The runs column filters background_tasks by the schedule id inside the payload. Without this the
-- lookup scans every task ever queued, scheduled or not.
CREATE INDEX background_tasks_schedule_idx
    ON background_tasks ((payload->>'schedule_id'))
    WHERE task_type = 'scheduled_agent_run';

-- Announce task status changes so an open mailbox can show what an agent is doing.
--
-- `UPDATE OF status` is load-bearing: the worker renews a task's lease every few seconds while it
-- runs (`renew_task_lease` touches only `lock_expires_at`), and a trigger on any UPDATE would turn
-- every heartbeat of every running task into a broadcast to every connected mailbox.
--
-- Tasks with no thread -- and there are some, `thread_id` is nullable and a deleted thread nulls it
-- -- have nothing to display against, so they are skipped rather than published and filtered later.
CREATE FUNCTION notify_thread_activity() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify(
        'thread_activity',
        json_build_object(
            'thread_id', NEW.thread_id,
            'channel_id', NEW.channel_id,
            'company_id', NEW.company_id
        )::text
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_notify_activity
    AFTER INSERT OR UPDATE OF status ON background_tasks
    FOR EACH ROW
    WHEN (NEW.thread_id IS NOT NULL)
    EXECUTE FUNCTION notify_thread_activity();

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

CREATE TABLE task_attempts (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES background_tasks(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    -- Why the attempt stopped running, beyond the coarse `status`. NULL for attempts still in
    -- flight and for rows written before the worker started recording it.
    stop_reason TEXT,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    result JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TIMESTAMPTZ,
    -- Which run of the task this attempt belongs to. A task that is stopped and started again
    -- keeps counting `attempt_number` from where it left off, so the generation is what separates
    -- one execution's attempts from the next's.
    execution_generation UUID NOT NULL,
    CONSTRAINT task_attempts_task_attempt_key UNIQUE (task_id, attempt_number),
    CONSTRAINT task_attempts_status_check CHECK (status IN ('processing', 'completed', 'failed')),
    CONSTRAINT task_attempts_token_check CHECK (
        (prompt_tokens IS NULL OR prompt_tokens >= 0)
        AND (completion_tokens IS NULL OR completion_tokens >= 0)
    ),
    CONSTRAINT task_attempts_stop_reason_check CHECK (stop_reason IN (
        'completed', 'retryable_failure', 'terminal_failure',
        'timed_out', 'shutdown', 'lease_lost'
    ))
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
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
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
    -- Inherited from the task whose work produced this email. `task_id` is cleared when the task
    -- is deleted, so this is what keeps a delivered email attached to the chain that sent it.
    correlation_id UUID NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    provider_message_id TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    worker_id UUID,
    locked_at TIMESTAMPTZ,
    lock_expires_at TIMESTAMPTZ,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    sent_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- The channel an outbound email goes out as, a real column rather than a JSONB payload field:
    -- the payload is the poller's data, not a queryable dimension, and filtering the outbox by
    -- channel through it meant an unindexed JSONB scan.
    channel_id UUID,
    CONSTRAINT email_outbox_status_check CHECK (status IN ('pending', 'sending', 'sent', 'failed')),
    CONSTRAINT email_outbox_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE SET NULL (task_id),
    -- Compound, like `email_outbox_task_fk`: the channel must belong to the same company as the
    -- email. Deleting a channel must not delete the record that mail was queued for it, hence
    -- SET NULL.
    CONSTRAINT email_outbox_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE SET NULL (channel_id),
    CONSTRAINT email_outbox_retry_check CHECK (retry_count >= 0),
    CONSTRAINT email_outbox_payload_object_check CHECK (jsonb_typeof(payload) = 'object'),
    -- Lease metadata belongs to 'sending' and to nothing else. Without the second arm a row that
    -- failed or was sent keeps the worker id that last touched it, and a stale lease on a
    -- terminal row reads as an in-flight delivery to anything sweeping for expired ones.
    CONSTRAINT email_outbox_lease_check CHECK (
        (status = 'sending'
         AND worker_id IS NOT NULL
         AND locked_at IS NOT NULL
         AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'sending'
         AND worker_id IS NULL
         AND locked_at IS NULL
         AND lock_expires_at IS NULL)
    )
);

CREATE INDEX email_outbox_pending_idx
    ON email_outbox (available_at, id) WHERE status = 'pending';
CREATE INDEX email_outbox_sending_lease_idx
    ON email_outbox (lock_expires_at, id) WHERE status = 'sending';
CREATE INDEX email_outbox_company_created_idx
    ON email_outbox (company_id, created_at DESC, id DESC);
CREATE INDEX email_outbox_correlation_idx
    ON email_outbox (correlation_id, created_at);

CREATE INDEX email_outbox_task_idx
    ON email_outbox (task_id) WHERE task_id IS NOT NULL;
CREATE INDEX email_outbox_company_channel_created_idx
    ON email_outbox (company_id, channel_id, created_at DESC, id DESC);
-- The board's delivery-side recency arm; see background_tasks_company_updated_idx.
CREATE INDEX email_outbox_company_updated_idx
    ON email_outbox (company_id, updated_at DESC);
-- Its unfinished arm. Added on measurement rather than symmetry: over 10k chains the board's
-- `status IN (...) OR updated_at >= cutoff` still sequentially scanned the outbox without it, and
-- only with both is the disjunction a BitmapOr of two index scans.
CREATE INDEX email_outbox_company_status_idx
    ON email_outbox (company_id, status);

CREATE TABLE task_outreaches (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES background_tasks(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    required_threshold_percent NUMERIC(5,2) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    outreach_key TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    CONSTRAINT task_outreaches_task_key UNIQUE (task_id, outreach_key),
    CONSTRAINT task_outreaches_status_check CHECK (
        status IN (
            'waiting', 'threshold_met', 'timeout_pending_approval',
            'proceed_partial', 'cancelled', 'completed'
        )
    ),
    CONSTRAINT task_outreaches_threshold_check CHECK (
        required_threshold_percent > 0
        AND required_threshold_percent <= 100
    ),
    CONSTRAINT task_outreaches_expiry_check CHECK (expires_at > created_at),
    CONSTRAINT task_outreaches_subject_check CHECK (length(btrim(subject)) > 0),
    CONSTRAINT task_outreaches_body_check CHECK (length(btrim(body)) > 0)
);

CREATE INDEX task_outreaches_due_idx
    ON task_outreaches (expires_at, id)
    WHERE status = 'waiting' AND expires_at IS NOT NULL;
CREATE INDEX task_outreaches_task_idx ON task_outreaches (task_id);
CREATE INDEX task_outreaches_task_status_idx
    ON task_outreaches (task_id, status);

CREATE TABLE task_outreach_targets (
    outreach_id UUID NOT NULL REFERENCES task_outreaches(id) ON DELETE CASCADE,
    email CITEXT NOT NULL,
    responded_at TIMESTAMPTZ,
    response_message_id UUID,
    outbox_id UUID REFERENCES email_outbox(id) ON DELETE SET NULL,
    PRIMARY KEY (outreach_id, email),
    CONSTRAINT task_outreach_targets_response_message_id_fkey
        FOREIGN KEY (response_message_id) REFERENCES thread_messages(id) ON DELETE SET NULL,
    CONSTRAINT task_outreach_targets_response_check CHECK (
        response_message_id IS NULL OR responded_at IS NOT NULL
    )
);

CREATE INDEX task_outreach_targets_email_waiting_idx
    ON task_outreach_targets (email, outreach_id) WHERE responded_at IS NULL;
CREATE INDEX task_outreach_targets_response_message_idx
    ON task_outreach_targets (response_message_id)
    WHERE response_message_id IS NOT NULL;
CREATE UNIQUE INDEX task_outreach_targets_outbox_idx
    ON task_outreach_targets (outbox_id) WHERE outbox_id IS NOT NULL;

-- Immutable, metadata-only history for every background-task status transition.
CREATE TABLE task_status_events (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    task_id UUID NOT NULL,
    correlation_id UUID NOT NULL,
    sequence INTEGER NOT NULL,
    from_status TEXT,
    to_status TEXT NOT NULL,
    reason TEXT NOT NULL,
    actor_kind TEXT NOT NULL,
    actor_id UUID,
    related_approval_id UUID REFERENCES human_approvals(id) ON DELETE SET NULL,
    related_outreach_id UUID REFERENCES task_outreaches(id) ON DELETE SET NULL,
    retry_count INTEGER NOT NULL,
    run_at TIMESTAMPTZ NOT NULL,
    execution_generation UUID,
    transitioned_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT task_status_events_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE,
    CONSTRAINT task_status_events_task_sequence_key UNIQUE (task_id, sequence),
    CONSTRAINT task_status_events_sequence_check CHECK (sequence > 0),
    CONSTRAINT task_status_events_retry_count_check CHECK (retry_count >= 0),
    CONSTRAINT task_status_events_from_status_check CHECK (
        from_status IS NULL OR from_status IN (
            'pending', 'processing', 'pending_approval',
            'waiting_for_third_party_reply', 'completed', 'failed',
            'dead_letter', 'stopped'
        )
    ),
    CONSTRAINT task_status_events_to_status_check CHECK (to_status IN (
        'pending', 'processing', 'pending_approval',
        'waiting_for_third_party_reply', 'completed', 'failed',
        'dead_letter', 'stopped'
    )),
    CONSTRAINT task_status_events_reason_check CHECK (reason IN (
        'enqueued', 'claimed', 'completed',
        'retryable_failure', 'terminal_failure', 'timed_out', 'shutdown',
        'lease_lost', 'approval_requested', 'approval_accepted', 'approval_rejected',
        'outreach_started', 'outreach_reply_received', 'outreach_timed_out',
        'outreach_extended', 'operator_stopped', 'operator_resumed', 'unknown'
    )),
    CONSTRAINT task_status_events_actor_kind_check CHECK (actor_kind IN (
        'system', 'worker', 'operator', 'approval', 'outreach'
    )),
    CONSTRAINT task_status_events_related_source_check CHECK (
        related_approval_id IS NULL OR related_outreach_id IS NULL
    )
);

CREATE INDEX task_status_events_task_history_idx
    ON task_status_events (task_id, transitioned_at, sequence, id);
CREATE INDEX task_status_events_company_correlation_timeline_idx
    ON task_status_events (company_id, correlation_id, transitioned_at, task_id, sequence, id);

-- The row-local attribution columns declared with `background_tasks` above, tied to the tables
-- they name now that those exist. No cascade action: the only way either row disappears is with
-- the task that owns it, and that deletion takes the referencing row with it.
ALTER TABLE background_tasks
    ADD CONSTRAINT background_tasks_transition_approval_fk
        FOREIGN KEY (transition_approval_id) REFERENCES human_approvals(id),
    ADD CONSTRAINT background_tasks_transition_outreach_fk
        FOREIGN KEY (transition_outreach_id) REFERENCES task_outreaches(id);

-- The transition that produced this row wrote its own attribution into `NEW.transition_*`, so the
-- ledger row is assembled from the same tuple that changed the status. The deterministic mapping
-- below is the fallback for the writes that genuinely have nothing to add -- an INSERT, and status
-- changes whose cause is fully determined by the pair of statuses.
--
-- Lease loss is deliberately absent from that mapping. It used to be recognised by matching
-- `NEW.last_error` against a copy of the Rust `LEASE_EXPIRED_ERROR` string, so editing the
-- constant would silently have reclassified every future lease loss as `retryable_failure`. The
-- sweep now names itself: it sets `transition_reason = 'lease_lost'` and copies each row's own
-- `worker_id` into `transition_actor_id`, so the event records the worker that actually lost that
-- lease and the duplicated string is gone rather than kept in sync.
CREATE FUNCTION record_task_status_event() RETURNS TRIGGER AS $$
DECLARE
    transition_reason TEXT;
    transition_actor_kind TEXT;
    transition_actor_id UUID;
    approval_id UUID;
    outreach_id UUID;
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.status = OLD.status THEN
        RETURN NULL;
    END IF;

    transition_reason := NEW.transition_reason;
    transition_actor_kind := NEW.transition_actor_kind;
    transition_actor_id := NEW.transition_actor_id;
    approval_id := NEW.transition_approval_id;
    outreach_id := NEW.transition_outreach_id;

    IF transition_reason IS NULL THEN
        transition_reason := CASE
            WHEN TG_OP = 'INSERT' THEN 'enqueued'
            WHEN OLD.status = 'pending' AND NEW.status = 'processing' THEN 'claimed'
            WHEN OLD.status = 'processing' AND NEW.status = 'completed' THEN 'completed'
            WHEN OLD.status = 'processing' AND NEW.status = 'pending' THEN 'retryable_failure'
            WHEN OLD.status = 'processing' AND NEW.status = 'dead_letter' THEN 'terminal_failure'
            WHEN OLD.status = 'processing' AND NEW.status = 'pending_approval' THEN 'approval_requested'
            WHEN OLD.status = 'processing' AND NEW.status = 'waiting_for_third_party_reply'
                THEN 'outreach_started'
            WHEN OLD.status = 'pending_approval' AND NEW.status = 'pending' THEN 'approval_accepted'
            WHEN OLD.status = 'waiting_for_third_party_reply' AND NEW.status = 'pending'
                THEN 'outreach_reply_received'
            WHEN OLD.status = 'waiting_for_third_party_reply' AND NEW.status = 'pending_approval'
                THEN 'outreach_timed_out'
            WHEN OLD.status = 'pending_approval'
                 AND NEW.status = 'waiting_for_third_party_reply' THEN 'outreach_extended'
            WHEN NEW.status = 'stopped' THEN 'operator_stopped'
            WHEN OLD.status = 'stopped' AND NEW.status = 'pending' THEN 'operator_resumed'
            -- Nothing above established a cause, so none is claimed. `retryable_failure` used to
            -- stand here, which turned every unattributed transition into a fabricated worker
            -- failure -- an operator resuming a dead-lettered task was filed as the worker failing
            -- it again. A row that says "unclassified" is greppable; a row that says the wrong
            -- thing is not.
            ELSE 'unknown'
        END;
    END IF;

    IF transition_actor_kind IS NULL THEN
        transition_actor_kind := CASE
            WHEN transition_reason IN ('claimed', 'completed', 'retryable_failure',
                                       'terminal_failure', 'timed_out', 'shutdown', 'lease_lost')
                THEN 'worker'
            WHEN transition_reason IN ('approval_requested', 'approval_accepted') THEN 'approval'
            WHEN transition_reason IN ('outreach_started', 'outreach_reply_received',
                                       'outreach_timed_out', 'outreach_extended') THEN 'outreach'
            WHEN transition_reason IN ('operator_stopped', 'operator_resumed') THEN 'operator'
            ELSE 'system'
        END;
    END IF;

    IF transition_actor_id IS NULL AND transition_actor_kind = 'worker' THEN
        transition_actor_id := COALESCE(NEW.worker_id, CASE WHEN TG_OP = 'UPDATE' THEN OLD.worker_id END);
    END IF;

    INSERT INTO task_status_events (
        id, company_id, task_id, correlation_id, sequence, from_status, to_status,
        reason, actor_kind, actor_id, related_approval_id, related_outreach_id,
        retry_count, run_at, execution_generation, transitioned_at
    ) VALUES (
        gen_random_uuid(), NEW.company_id, NEW.id, NEW.correlation_id,
        COALESCE((SELECT MAX(event.sequence) + 1
                  FROM task_status_events AS event WHERE event.task_id = NEW.id), 1),
        CASE WHEN TG_OP = 'UPDATE' THEN OLD.status ELSE NULL END,
        NEW.status, transition_reason, transition_actor_kind, transition_actor_id,
        approval_id, outreach_id, NEW.retry_count, NEW.run_at,
        COALESCE(NEW.execution_generation,
                 CASE WHEN TG_OP = 'UPDATE' THEN OLD.execution_generation END),
        CURRENT_TIMESTAMP
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER background_tasks_record_status_event
AFTER INSERT OR UPDATE OF status ON background_tasks
FOR EACH ROW EXECUTE FUNCTION record_task_status_event();

-- One small, identifier-only notification wakes every Tasks board that may need to reconcile.
CREATE FUNCTION notify_task_chain_changed() RETURNS TRIGGER AS $$
DECLARE
    notified_company_id UUID;
    notified_correlation_id UUID;
BEGIN
    -- `UPDATE OF status` fires whenever the column appears in a SET list, whether or not the value
    -- moved. A write that leaves the status alone changes nothing the board draws, so it must not
    -- wake every connected viewer of the company. This suppresses no real transition:
    -- `pending -> sending -> sent` is three material changes and still emits three notifications,
    -- which the stream coalesces on its own. The checks are per table because these are the only
    -- notifying tables that have a `status` column at all.
    IF TG_OP = 'UPDATE' THEN
        IF TG_TABLE_NAME = 'email_outbox' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        ELSIF TG_TABLE_NAME = 'human_approvals' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        ELSIF TG_TABLE_NAME = 'task_outreaches' THEN
            IF NEW.status IS NOT DISTINCT FROM OLD.status THEN
                RETURN NULL;
            END IF;
        END IF;
    END IF;

    IF TG_TABLE_NAME = 'task_status_events' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'email_outbox' THEN
        notified_company_id := NEW.company_id;
        notified_correlation_id := NEW.correlation_id;
    ELSIF TG_TABLE_NAME = 'human_approvals' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id;
    ELSIF TG_TABLE_NAME = 'task_outreaches' THEN
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM background_tasks AS task WHERE task.id = NEW.task_id;
    ELSE
        SELECT task.company_id, task.correlation_id
          INTO notified_company_id, notified_correlation_id
          FROM task_outreaches AS outreach
          JOIN background_tasks AS task ON task.id = outreach.task_id
         WHERE outreach.id = NEW.outreach_id;
    END IF;

    IF notified_company_id IS NOT NULL AND notified_correlation_id IS NOT NULL THEN
        PERFORM pg_notify(
            'task_chain_changed',
            json_build_object(
                'company_id', notified_company_id,
                'correlation_id', notified_correlation_id
            )::text
        );
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_status_events_notify_chain
AFTER INSERT ON task_status_events
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER email_outbox_notify_chain
AFTER INSERT OR UPDATE OF status ON email_outbox
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER human_approvals_notify_chain
AFTER INSERT OR UPDATE OF status ON human_approvals
FOR EACH ROW WHEN (NEW.task_id IS NOT NULL)
EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER task_outreaches_notify_chain
AFTER INSERT OR UPDATE OF status ON task_outreaches
FOR EACH ROW EXECUTE FUNCTION notify_task_chain_changed();

CREATE TRIGGER task_outreach_targets_notify_chain
AFTER UPDATE OF responded_at ON task_outreach_targets
FOR EACH ROW WHEN (OLD.responded_at IS DISTINCT FROM NEW.responded_at)
EXECUTE FUNCTION notify_task_chain_changed();

CREATE TABLE channel_schedules (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL,
    name TEXT NOT NULL,
    schedule_type TEXT NOT NULL,
    interval_seconds BIGINT,
    subject_template TEXT NOT NULL,
    prompt_template TEXT NOT NULL,
    delivery_mode TEXT NOT NULL DEFAULT 'mailbox_only',
    recipient_emails CITEXT[] NOT NULL DEFAULT '{}',
    timezone TEXT NOT NULL DEFAULT 'UTC',
    enabled BOOLEAN NOT NULL DEFAULT true,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT channel_schedules_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT channel_schedules_type_check CHECK (schedule_type IN ('interval', 'one_off')),
    CONSTRAINT channel_schedules_interval_check CHECK (
        (schedule_type = 'interval' AND interval_seconds IS NOT NULL AND interval_seconds >= 60)
        OR (schedule_type = 'one_off' AND interval_seconds IS NULL)
    ),
    CONSTRAINT channel_schedules_delivery_mode_check CHECK (
        delivery_mode IN ('mailbox_only', 'email_participants', 'email_custom')
    ),
    -- A schedule renders its templates and counts its days in this zone, so an unknown name has to
    -- be refused at write time: the claim query would otherwise fail on every tick.
    CONSTRAINT channel_schedules_timezone_check CHECK (now() AT TIME ZONE timezone IS NOT NULL),
    -- Compound so the schedule's company and its channel's company cannot drift apart.
    CONSTRAINT channel_schedules_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE
);

CREATE INDEX channel_schedules_due_idx
    ON channel_schedules (next_run_at, id)
    WHERE enabled = true AND next_run_at IS NOT NULL;

CREATE INDEX channel_schedules_company_idx
    ON channel_schedules (company_id, created_at DESC, id DESC);

CREATE INDEX channel_schedules_channel_idx
    ON channel_schedules (channel_id, created_at DESC, id DESC);

-- One row per slot a schedule was due for, written before any work is attempted. The slot's UNIQUE
-- key is what makes a tick idempotent: a second scheduler that wakes for the same slot collides
-- rather than running the agent twice.
--
-- `schedule_snapshot` freezes the templates and delivery settings as they were when the slot came
-- due, so editing a schedule does not retroactively change a run that is still materializing.
CREATE TABLE schedule_runs (
    id UUID PRIMARY KEY,
    schedule_id UUID NOT NULL REFERENCES channel_schedules(id) ON DELETE CASCADE,
    scheduled_for TIMESTAMPTZ NOT NULL,
    schedule_snapshot JSONB NOT NULL,
    thread_id UUID REFERENCES threads(id) ON DELETE SET NULL,
    task_id UUID REFERENCES background_tasks(id) ON DELETE SET NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Turning a due slot into a thread and a task is itself leased durable work: the run is the
    -- queue row, and `materialization_status` is its state machine.
    materialization_status TEXT NOT NULL DEFAULT 'pending',
    materialization_attempts INTEGER NOT NULL DEFAULT 0,
    materialization_available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    materialization_worker_id UUID,
    materialization_generation UUID,
    materialization_locked_at TIMESTAMPTZ,
    materialization_lock_expires_at TIMESTAMPTZ,
    CONSTRAINT schedule_runs_schedule_slot_key UNIQUE (schedule_id, scheduled_for),
    CONSTRAINT schedule_runs_snapshot_object_check
        CHECK (jsonb_typeof(schedule_snapshot) = 'object'),
    CONSTRAINT schedule_runs_task_requires_thread_check
        CHECK (task_id IS NULL OR thread_id IS NOT NULL),
    CONSTRAINT schedule_runs_materialization_attempts_check
        CHECK (materialization_attempts >= 0 AND materialization_attempts <= 5),
    -- Each status names exactly which of the lease columns may be set, so a lost worker cannot
    -- leave a row that looks both claimed and free, and 'failed' is reachable only once the
    -- attempt budget is spent.
    CONSTRAINT schedule_runs_materialization_state_check CHECK (
        (materialization_status = 'pending'
         AND task_id IS NULL
         AND materialization_attempts < 5
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
        OR
        (materialization_status = 'materializing'
         AND task_id IS NULL
         AND materialization_attempts BETWEEN 1 AND 5
         AND materialization_worker_id IS NOT NULL
         AND materialization_generation IS NOT NULL
         AND materialization_locked_at IS NOT NULL
         AND materialization_lock_expires_at IS NOT NULL
         AND materialization_lock_expires_at > materialization_locked_at)
        OR
        (materialization_status = 'materialized'
         AND task_id IS NOT NULL
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
        OR
        (materialization_status = 'failed'
         AND task_id IS NULL
         AND materialization_attempts = 5
         AND materialization_worker_id IS NULL
         AND materialization_generation IS NULL
         AND materialization_locked_at IS NULL
         AND materialization_lock_expires_at IS NULL)
    )
);

CREATE INDEX schedule_runs_schedule_created_idx
    ON schedule_runs (schedule_id, created_at DESC, id DESC);
CREATE INDEX schedule_runs_materialization_ready_idx
    ON schedule_runs (materialization_available_at, created_at, id)
    WHERE materialization_status = 'pending';
CREATE INDEX schedule_runs_materialization_expired_idx
    ON schedule_runs (materialization_lock_expires_at, created_at, id)
    WHERE materialization_status = 'materializing';

-- Declared before `memory_remote_resource_lifecycles` on purpose, and the order is load-bearing.
-- Both tables carry a `company_id` FK to `companies`, and Postgres runs a delete's referential
-- actions in the order those constraints were created. This table's ON DELETE CASCADE has to run
-- first, because deleting the connection is what fires
-- `memory_connection_lifecycle_compatibility_delete` and flips the lifecycle row to 'absent'.
-- Declare the lifecycle table first and its ON DELETE SET NULL runs while `desired_state` is still
-- 'present', which its own CHECK rejects. Nothing in a schema dump records this; moving these two
-- definitions past each other breaks company deletion.
CREATE TABLE memory_provider_connections (
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb', 'hindsight')),
    remote_database_id TEXT NOT NULL,
    readiness TEXT NOT NULL DEFAULT 'pending'
        CHECK (readiness IN ('pending', 'provisioning', 'ready', 'failed')),
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, provider),
    UNIQUE (provider, remote_database_id)
);

-- What the provider is meant to be holding for a company, kept after the company row is gone.
--
-- Deleting a company must not lose the fact that a remote database still exists and has to be torn
-- down, so `company_id` is nullable and the intent lives here rather than on the connection.
-- `operation_generation` fences the workers: an operation leased under an older generation cannot
-- apply its result over a newer decision.
CREATE TABLE memory_remote_resource_lifecycles (
    provider TEXT NOT NULL CHECK (provider IN ('hydradb', 'hindsight')),
    remote_database_id TEXT NOT NULL,
    company_id UUID NULL REFERENCES companies(id) ON DELETE SET NULL,
    desired_state TEXT NOT NULL CHECK (desired_state IN ('present', 'absent')),
    operation_generation BIGINT NOT NULL DEFAULT 0 CHECK (operation_generation >= 0),
    operation_lease_token UUID NULL,
    operation_lease_expires_at TIMESTAMPTZ NULL,
    quiesce_until TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (provider, remote_database_id),
    UNIQUE (company_id, provider),
    CHECK (
        (operation_lease_token IS NULL AND operation_lease_expires_at IS NULL)
        OR
        (operation_lease_token IS NOT NULL AND operation_lease_expires_at IS NOT NULL)
    ),
    CHECK (desired_state = 'absent' OR company_id IS NOT NULL)
);

-- Creating the remote database and waiting for it to come up are separate durable phases, hence
-- `phase` alongside `status`: `status` is the queue state a worker leases on, `phase` is where the
-- provisioning itself has got to. `attempts` counts leases, `failure_attempts` counts only
-- classified provider failures, so polling a slow-but-healthy database never exhausts the budget.
CREATE TABLE memory_provisioning_jobs (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb', 'hindsight')),
    remote_database_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_token UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    operation_generation BIGINT NULL,
    phase TEXT NOT NULL DEFAULT 'create_pending',
    failure_attempts INTEGER NOT NULL DEFAULT 0,
    readiness_deadline TIMESTAMPTZ NULL,
    next_poll_at TIMESTAMPTZ NULL,
    UNIQUE (company_id, provider),
    UNIQUE (provider, remote_database_id),
    CHECK (
        (status = 'leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (status <> 'leased' AND lease_token IS NULL AND lease_expires_at IS NULL)
    ),
    CONSTRAINT memory_provisioning_jobs_generation_state_check CHECK (
        (status = 'leased' AND operation_generation IS NOT NULL)
        OR
        (status <> 'leased' AND operation_generation IS NULL)
    ),
    CONSTRAINT memory_provisioning_jobs_lifecycle_fkey
        FOREIGN KEY (provider, remote_database_id)
        REFERENCES memory_remote_resource_lifecycles(provider, remote_database_id),
    CONSTRAINT memory_provisioning_jobs_phase_check
        CHECK (phase IN ('create_pending', 'waiting_ready', 'ready', 'failed')),
    CONSTRAINT memory_provisioning_jobs_failure_attempts_check
        CHECK (failure_attempts >= 0),
    CONSTRAINT memory_provisioning_jobs_phase_state_check CHECK (
        (status IN ('pending', 'leased') AND phase IN ('create_pending', 'waiting_ready'))
        OR (status = 'completed' AND phase = 'ready')
        OR (status = 'failed' AND phase = 'failed')
    ),
    CONSTRAINT memory_provisioning_jobs_readiness_window_check CHECK (
        (phase = 'create_pending' AND readiness_deadline IS NULL AND next_poll_at IS NULL)
        OR
        (phase = 'waiting_ready' AND readiness_deadline IS NOT NULL AND next_poll_at IS NOT NULL)
        OR phase IN ('ready', 'failed')
    )
);

-- A waiting_ready job is due at its next poll, everything else at its backoff. One index over the
-- CASE keeps both phases on the same claim query.
CREATE INDEX memory_provisioning_jobs_due_idx
    ON memory_provisioning_jobs (
        (CASE phase WHEN 'waiting_ready' THEN next_poll_at ELSE available_at END),
        created_at,
        id
    )
    WHERE status = 'pending';

CREATE TABLE memory_cleanup_jobs (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider IN ('hydradb', 'hindsight')),
    remote_database_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'leased', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_expires_at TIMESTAMPTZ NULL,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_token UUID NULL,
    operation_generation BIGINT NULL,
    UNIQUE (provider, remote_database_id),
    CONSTRAINT memory_cleanup_jobs_lease_state_check CHECK (
        (status = 'leased' AND lease_token IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (status <> 'leased' AND lease_token IS NULL AND lease_expires_at IS NULL)
    ),
    CONSTRAINT memory_cleanup_jobs_generation_state_check CHECK (
        (status = 'leased' AND operation_generation IS NOT NULL)
        OR
        (status <> 'leased' AND operation_generation IS NULL)
    ),
    CONSTRAINT memory_cleanup_jobs_lifecycle_fkey
        FOREIGN KEY (provider, remote_database_id)
        REFERENCES memory_remote_resource_lifecycles(provider, remote_database_id)
);

CREATE INDEX memory_cleanup_jobs_due_idx
    ON memory_cleanup_jobs (available_at, created_at, id)
    WHERE status = 'pending';

-- Keep lifecycle intent coherent while an older application version is still serving traffic.
-- The explicit application writes remain authoritative; these triggers cover only legacy writes.
CREATE FUNCTION create_memory_lifecycle_for_legacy_connection() RETURNS trigger AS $$
BEGIN
    INSERT INTO memory_remote_resource_lifecycles
        (provider, remote_database_id, company_id, desired_state)
    VALUES (NEW.provider, NEW.remote_database_id, NEW.company_id, 'present')
    ON CONFLICT (provider, remote_database_id) DO UPDATE
    SET company_id = EXCLUDED.company_id,
        desired_state = 'present',
        quiesce_until = CURRENT_TIMESTAMP,
        last_error = NULL,
        updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER memory_connection_lifecycle_compatibility_insert
AFTER INSERT ON memory_provider_connections
FOR EACH ROW EXECUTE FUNCTION create_memory_lifecycle_for_legacy_connection();

CREATE FUNCTION retire_memory_lifecycle_for_legacy_connection() RETURNS trigger AS $$
DECLARE
    cleanup_available_at TIMESTAMPTZ;
BEGIN
    UPDATE memory_remote_resource_lifecycles
    SET company_id = NULL,
        desired_state = 'absent',
        quiesce_until = GREATEST(
            quiesce_until,
            CURRENT_TIMESTAMP + INTERVAL '180 seconds'
        ),
        last_error = NULL,
        updated_at = CURRENT_TIMESTAMP
    WHERE provider = OLD.provider
      AND remote_database_id = OLD.remote_database_id
      AND (desired_state <> 'absent' OR company_id IS NOT NULL)
    RETURNING quiesce_until INTO cleanup_available_at;

    IF FOUND THEN
        INSERT INTO memory_cleanup_jobs
            (id, provider, remote_database_id, available_at)
        VALUES (
            md5(OLD.provider || ':' || OLD.remote_database_id)::uuid,
            OLD.provider,
            OLD.remote_database_id,
            cleanup_available_at
        )
        ON CONFLICT (provider, remote_database_id) DO UPDATE
        SET status = 'pending',
            attempts = 0,
            available_at = EXCLUDED.available_at,
            lease_token = NULL,
            lease_expires_at = NULL,
            operation_generation = NULL,
            last_error = NULL,
            updated_at = CURRENT_TIMESTAMP
        WHERE memory_cleanup_jobs.status <> 'leased';
    END IF;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER memory_connection_lifecycle_compatibility_delete
BEFORE DELETE ON memory_provider_connections
FOR EACH ROW EXECUTE FUNCTION retire_memory_lifecycle_for_legacy_connection();

-- Keep the phase constraint compatible with workers from a preceding application release during a
-- rolling deploy. New workers write phase explicitly; this trigger fills only legacy status-only
-- transitions.
CREATE FUNCTION synchronize_legacy_memory_provisioning_phase() RETURNS trigger AS $$
BEGIN
    IF NEW.status = 'completed' AND NEW.phase NOT IN ('ready', 'failed') THEN
        NEW.phase = 'ready';
    ELSIF NEW.status = 'failed' AND NEW.phase <> 'failed' THEN
        NEW.phase = 'failed';
    ELSIF NEW.status = 'pending' AND OLD.status IN ('completed', 'failed')
            AND NEW.phase IN ('ready', 'failed') THEN
        NEW.phase = 'create_pending';
        NEW.failure_attempts = 0;
        NEW.readiness_deadline = NULL;
        NEW.next_poll_at = NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER memory_provisioning_phase_compatibility_update
BEFORE UPDATE ON memory_provisioning_jobs
FOR EACH ROW EXECUTE FUNCTION synchronize_legacy_memory_provisioning_phase();

CREATE TABLE runtime_metric_samples (
    machine_id TEXT NOT NULL,
    machine_region TEXT,
    sampled_at TIMESTAMPTZ NOT NULL,
    process_rss_bytes BIGINT,
    memory_limit_bytes BIGINT,
    cpu_utilization_percent DOUBLE PRECISION,
    cpu_steal_percent DOUBLE PRECISION,
    cpu_throttle_percent DOUBLE PRECISION,
    database_acquire_duration_ms DOUBLE PRECISION NOT NULL,
    database_acquire_succeeded BOOLEAN NOT NULL,
    pool_size INTEGER NOT NULL,
    pool_idle INTEGER NOT NULL,
    pool_active INTEGER NOT NULL,
    active_task_executions INTEGER NOT NULL DEFAULT 0,
    task_worker_concurrency_limit INTEGER NOT NULL DEFAULT 1,
    -- Memory provider calls counted per ten-second sample rather than probed, so the figures are
    -- the latency and failures memory recall and ingestion actually paid, and an idle machine
    -- polls nobody. The aggregate spans every configured provider; the `hydradb_` column names
    -- predate the second one and are kept deliberately, because renaming them would mean a
    -- migration on a table whose CHECK constraints are coupled to the column list.
    hydradb_calls INTEGER NOT NULL DEFAULT 0,
    hydradb_failures INTEGER NOT NULL DEFAULT 0,
    hydradb_duration_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (machine_id, sampled_at),
    CONSTRAINT runtime_metric_samples_rss_nonnegative
        CHECK (process_rss_bytes IS NULL OR process_rss_bytes >= 0),
    CONSTRAINT runtime_metric_samples_memory_limit_nonnegative
        CHECK (memory_limit_bytes IS NULL OR memory_limit_bytes >= 0),
    CONSTRAINT runtime_metric_samples_cpu_utilization_nonnegative
        CHECK (cpu_utilization_percent IS NULL OR cpu_utilization_percent >= 0),
    CONSTRAINT runtime_metric_samples_cpu_steal_nonnegative
        CHECK (cpu_steal_percent IS NULL OR cpu_steal_percent >= 0),
    CONSTRAINT runtime_metric_samples_cpu_throttle_nonnegative
        CHECK (cpu_throttle_percent IS NULL OR cpu_throttle_percent >= 0),
    CONSTRAINT runtime_metric_samples_acquire_duration_nonnegative
        CHECK (database_acquire_duration_ms >= 0),
    CONSTRAINT runtime_metric_samples_pool_size_nonnegative CHECK (pool_size >= 0),
    CONSTRAINT runtime_metric_samples_pool_idle_nonnegative CHECK (pool_idle >= 0),
    CONSTRAINT runtime_metric_samples_pool_active_nonnegative CHECK (pool_active >= 0),
    CONSTRAINT runtime_metric_samples_pool_parts_fit
        CHECK (pool_idle + pool_active = pool_size),
    CONSTRAINT runtime_metric_samples_active_tasks_nonnegative
        CHECK (active_task_executions >= 0),
    CONSTRAINT runtime_metric_samples_worker_limit_positive
        CHECK (task_worker_concurrency_limit > 0),
    CONSTRAINT runtime_metric_samples_active_tasks_within_limit
        CHECK (active_task_executions <= task_worker_concurrency_limit),
    CONSTRAINT runtime_metric_samples_hydradb_calls_nonnegative
        CHECK (hydradb_calls >= 0),
    CONSTRAINT runtime_metric_samples_hydradb_failures_within_calls
        CHECK (hydradb_failures >= 0 AND hydradb_failures <= hydradb_calls),
    CONSTRAINT runtime_metric_samples_hydradb_duration_nonnegative
        CHECK (hydradb_duration_ms >= 0),
    CONSTRAINT runtime_metric_samples_hydradb_duration_needs_calls
        CHECK (hydradb_calls > 0 OR hydradb_duration_ms = 0)
);

-- The primary key is also the covering B-tree for reads and pruning by machine and sample time.
COMMENT ON CONSTRAINT runtime_metric_samples_pkey ON runtime_metric_samples IS
    'Supports runtime history reads on (machine_id, sampled_at)';
