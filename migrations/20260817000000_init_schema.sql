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
    default_add_3rd_party BOOLEAN NOT NULL DEFAULT TRUE,
    default_participant_emails CITEXT[],
    default_retrieve_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    default_retrieve_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    default_retrieve_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
    default_persist_company_memory BOOLEAN NOT NULL DEFAULT FALSE,
    default_persist_agent_memory BOOLEAN NOT NULL DEFAULT FALSE,
    default_persist_user_memory BOOLEAN NOT NULL DEFAULT FALSE,
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
        CHECK (memory_provider IS NULL OR memory_provider IN ('hydradb', 'hindsight')),
    CONSTRAINT companies_default_participants_bounded CHECK (
        default_participant_emails IS NULL
        OR (
            cardinality(default_participant_emails) <= 64
            AND array_position(default_participant_emails, NULL) IS NULL
            AND array_position(default_participant_emails, ''::citext) IS NULL
        )
    )
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
    CONSTRAINT company_members_role_check CHECK (role IN ('owner', 'member', 'admin'))
);

-- The owner is also a member row, so a person principal can prove company membership with one
-- foreign key instead of a union against `companies.user_id`.
CREATE UNIQUE INDEX company_members_one_owner_idx
    ON company_members (company_id) WHERE role = 'owner';

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
    memory_enabled BOOLEAN NOT NULL DEFAULT FALSE,
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

-- A principal is one company-scoped actor: a teammate, an agent, an outsider we have seen, or the
-- platform itself.  Every authorization and thread-participation decision names a principal, so
-- that none of them is keyed by a mutable address string.
CREATE TABLE principals (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    user_id UUID,
    agent_id UUID,
    display_label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT principals_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT principals_kind_check CHECK (kind IN ('person', 'agent', 'external', 'system')),
    CONSTRAINT principals_display_label_check CHECK (
        btrim(display_label) <> '' AND octet_length(display_label) <= 255
    ),
    -- Exactly the reference its kind allows: an external or system principal cannot smuggle in a
    -- user or agent id and inherit that actor's access.
    CONSTRAINT principals_shape_check CHECK (
        (kind = 'person' AND user_id IS NOT NULL AND agent_id IS NULL)
        OR (kind = 'agent' AND user_id IS NULL AND agent_id IS NOT NULL)
        OR (kind IN ('external', 'system') AND user_id IS NULL AND agent_id IS NULL)
    ),
    -- Composite references prove the referenced user or agent belongs to the *same* company, so a
    -- cross-tenant principal cannot be written at all.
    CONSTRAINT principals_company_user_fk
        FOREIGN KEY (company_id, user_id)
        REFERENCES company_members(company_id, user_id) ON DELETE CASCADE,
    CONSTRAINT principals_company_agent_fk
        FOREIGN KEY (company_id, agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX principals_company_user_key
    ON principals (company_id, user_id) WHERE user_id IS NOT NULL;
CREATE UNIQUE INDEX principals_company_agent_key
    ON principals (company_id, agent_id) WHERE agent_id IS NOT NULL;
-- The platform itself is one actor per company, so a schedule prompt and an approval note written
-- months apart are attributed to the same principal rather than to a growing pile of look-alikes.
CREATE UNIQUE INDEX principals_company_system_key
    ON principals (company_id) WHERE kind = 'system';

-- One transport-qualified handle for a principal.  `(transport, namespace, subject)` is the whole
-- key: an email mailbox and a Slack user id in two workspaces are three distinct rows that never
-- collide, and nothing here is compared case-insensitively -- the email writer stores the
-- normalized lower-case mailbox instead, so the generic column keeps provider-exact bytes.
CREATE TABLE participant_identities (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    transport TEXT NOT NULL,
    namespace TEXT NOT NULL,
    subject TEXT NOT NULL,
    display_label TEXT,
    status TEXT NOT NULL,
    claim_metadata JSONB NOT NULL,
    provenance TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT participant_identities_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT participant_identities_company_principal_id_key
        UNIQUE (company_id, principal_id, id),
    CONSTRAINT participant_identities_qualified_key
        UNIQUE (company_id, transport, namespace, subject),
    CONSTRAINT participant_identities_principal_fk
        FOREIGN KEY (company_id, principal_id)
        REFERENCES principals(company_id, id) ON DELETE CASCADE,
    CONSTRAINT participant_identities_transport_check CHECK (transport IN ('email', 'slack')),
    CONSTRAINT participant_identities_namespace_check CHECK (
        btrim(namespace) <> '' AND octet_length(namespace) <= 255
    ),
    CONSTRAINT participant_identities_subject_check CHECK (
        btrim(subject) <> '' AND octet_length(subject) <= 320
    ),
    CONSTRAINT participant_identities_display_label_check CHECK (
        display_label IS NULL OR octet_length(display_label) <= 255
    ),
    CONSTRAINT participant_identities_status_check
        CHECK (status IN ('observed', 'verified', 'disabled')),
    CONSTRAINT participant_identities_provenance_check CHECK (
        provenance IN ('account', 'agent', 'channel_allowlist', 'transport_ingress',
                       'provider_profile_claim', 'system')
    ),
    -- A claim is enrichment, never a key: a Slack profile email lives in here and merges nothing.
    -- The payload is versioned, discriminated and bounded; Rust still decodes it fallibly because
    -- a structurally valid object can carry a discriminator a rolling deploy has not learned yet.
    CONSTRAINT participant_identities_claim_metadata_check CHECK (
        jsonb_typeof(claim_metadata) = 'object'
        AND claim_metadata->'version' = '1'::JSONB
        AND jsonb_typeof(claim_metadata->'kind') = 'string'
        AND claim_metadata->>'kind' IN ('observation', 'account', 'provider_profile')
        AND octet_length(claim_metadata::text) <= 8192
    )
);

CREATE INDEX participant_identities_principal_idx
    ON participant_identities (company_id, principal_id, created_at, id);

-- Which transports need a company-scoped provider account before anything can be read or sent.
-- Email is a *deployment* transport: this server owns its own mail namespace, so a channel is
-- reachable as soon as it has an address. Slack is an *installed* transport, and a binding onto it
-- is meaningless without the workspace grant behind it.
--
-- Written once as a function because three constraints need the same answer, and mirrored in Rust
-- by `TransportKind::requires_installation`. The equivalence is a test, not a comment: see
-- `rust_and_sql_agree_on_which_transports_require_an_installation`.
CREATE FUNCTION transport_requires_installation(transport TEXT) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN transport = 'slack';

-- Why a binding changed state, for the `disabled_reason` column and the audit log alike. One list
-- so an operator reading an audit row and an operator reading a disabled binding see the same
-- vocabulary, and so a reason can never be recovered by parsing a free-text error.
CREATE FUNCTION valid_binding_change_reason(reason TEXT) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN reason IN (
    'manager_request', 'installation_revoked', 'endpoint_removed',
    'access_revoked', 'channel_disabled', 'provider_drift'
);

-- Why a delivery attempt did not succeed. One list, because both `message_deliveries` and
-- `message_delivery_parts` classify the same failures and an operator alert reads across both.
-- Mirrored in Rust by `FailureClass`; the equivalence is a test rather than a comment.
CREATE FUNCTION valid_delivery_failure_class(class TEXT) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN class IN (
    'authentication', 'rate_limited', 'invalid_payload', 'destination_unavailable',
    'network', 'timeout', 'provider_fault', 'internal',
    'dependency_failed', 'superseded', 'lease_expired'
);

-- One provider account a company has installed. No token is stored here: the broad entity is
-- listed in the UI, logged, and serialized, so the secret lives one table over in
-- `integration_credentials` and is only ever read through an exact-scope query.
CREATE TABLE integration_installations (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    transport TEXT NOT NULL,
    -- The provider's own identifier for the account -- a Slack team id.
    external_tenant_key TEXT NOT NULL,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL,
    -- What the provider says it granted, as the provider spells it. Diagnostic only; it never
    -- substitutes for handling the provider's own authorization errors.
    granted_scopes TEXT[] NOT NULL DEFAULT '{}',
    installed_by JSONB NOT NULL,
    installed_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_by JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_by JSONB,
    revoked_at TIMESTAMPTZ,
    -- Composite keys the tenant-scoped children below point at. The three-column form additionally
    -- proves a binding's `transport` matches the installation it names, so a Slack binding cannot
    -- hang off some future provider's account.
    CONSTRAINT integration_installations_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT integration_installations_company_transport_key UNIQUE (company_id, id, transport),
    -- v1 refuses to let one external workspace install into two app companies: either company's
    -- managers could then link the other's conversations. Revisit only with a written
    -- multi-tenant-workspace threat model, not because a customer asks.
    CONSTRAINT integration_installations_tenant_key UNIQUE (transport, external_tenant_key),
    CONSTRAINT integration_installations_transport_check
        CHECK (transport_requires_installation(transport)),
    CONSTRAINT integration_installations_tenant_key_check CHECK (
        btrim(external_tenant_key) <> '' AND octet_length(external_tenant_key) <= 255
    ),
    CONSTRAINT integration_installations_display_name_check CHECK (
        btrim(display_name) <> '' AND octet_length(display_name) <= 255
    ),
    CONSTRAINT integration_installations_status_check CHECK (
        status IN ('active', 'reauthorization_required', 'revoked', 'disabled')
    ),
    -- A provider can hand back an arbitrary scope list; this is the bound on it.
    CONSTRAINT integration_installations_scopes_check CHECK (
        array_position(granted_scopes, NULL) IS NULL
        AND NOT ('' = ANY (granted_scopes))
        AND COALESCE(array_length(granted_scopes, 1), 0) <= 64
        AND octet_length(array_to_string(granted_scopes, ',')) <= 4096
    ),
    CONSTRAINT integration_installations_installed_by_check
        CHECK (valid_creation_provenance(installed_by)),
    CONSTRAINT integration_installations_updated_by_check
        CHECK (valid_creation_provenance(updated_by)),
    -- Revocation is the one terminal transition, so it is the one that has to name an actor and a
    -- time -- and it cannot be recorded without the status that means it.
    CONSTRAINT integration_installations_revocation_check CHECK (
        (status = 'revoked') = (revoked_at IS NOT NULL)
        AND (revoked_at IS NULL) = (revoked_by IS NULL)
        AND (revoked_by IS NULL OR valid_creation_provenance(revoked_by))
    )
);

CREATE INDEX integration_installations_company_idx
    ON integration_installations (company_id, transport, installed_at DESC, id DESC);

-- One secret, in its own table, keyed by exactly the scope a reader must state.
--
-- `envelope` is the output of the per-credential DEK format in
-- `src/adapters/persistence/credentials/envelope.rs`: a random data key encrypts the token, the
-- key-encryption key wraps the data key, and both layers authenticate the row's own
-- (company, installation, transport, kind) context. Moving a row's ciphertext to another company,
-- installation or credential kind therefore fails to open rather than silently decrypting.
--
-- The CHECK is defence in depth. It recognizes the envelope's *structure* so a plaintext token
-- cannot be written by hand; it proves nothing about authenticity, which is the application's job.
CREATE TABLE integration_credentials (
    company_id UUID NOT NULL,
    installation_id UUID NOT NULL,
    credential_kind TEXT NOT NULL,
    envelope TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, installation_id, credential_kind),
    CONSTRAINT integration_credentials_installation_fk
        FOREIGN KEY (company_id, installation_id)
        REFERENCES integration_installations(company_id, id) ON DELETE CASCADE,
    CONSTRAINT integration_credentials_kind_check CHECK (
        credential_kind IN ('bot_access_token', 'bot_refresh_token', 'user_access_token')
    ),
    -- enc:v2:<kek version>:<dek nonce>:<wrapped dek>:<data nonce>:<ciphertext+tag>
    CONSTRAINT integration_credentials_envelope_check CHECK (
        envelope ~ '^enc:v2:[1-9][0-9]{0,8}(:[A-Za-z0-9+/]+={0,2}){4}$'
        AND octet_length(envelope) <= 8192
    )
);

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
    owner_agent_id UUID,
    CONSTRAINT channels_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT channels_owner_agent_key UNIQUE (owner_agent_id),
    CONSTRAINT channels_owner_agent_fk
        FOREIGN KEY (company_id, owner_agent_id)
        REFERENCES agents(company_id, id) ON DELETE CASCADE,
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

CREATE FUNCTION enforce_owned_channel_position_zero() RETURNS trigger
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
        SELECT 1
        FROM channels AS channel
        WHERE channel.id = checked_channel_id
          AND channel.owner_agent_id IS NOT NULL
          AND NOT EXISTS (
              SELECT 1
              FROM channel_agents AS assignment
              WHERE assignment.channel_id = channel.id
                AND assignment.agent_id = channel.owner_agent_id
                AND assignment.position = 0
          )
    ) THEN
        RAISE EXCEPTION 'owned channel must assign its owner agent at position 0'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER owned_channel_position_zero_check
AFTER INSERT OR UPDATE OF owner_agent_id ON channels
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_owned_channel_position_zero();

CREATE CONSTRAINT TRIGGER owned_channel_assignment_position_zero_check
AFTER INSERT OR UPDATE OR DELETE ON channel_agents
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_owned_channel_position_zero();

CREATE FUNCTION prevent_owned_channel_delete() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.owner_agent_id IS NOT NULL
       AND EXISTS (SELECT 1 FROM agents WHERE id = OLD.owner_agent_id) THEN
        RAISE EXCEPTION 'owned channel must be deleted through its owner agent'
            USING ERRCODE = '23503';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER owned_channel_delete_guard
BEFORE DELETE ON channels
FOR EACH ROW EXECUTE FUNCTION prevent_owned_channel_delete();

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

-- What one principal may do on one channel.  `participate` is permission to send into the channel;
-- `view` is permission to read it in the UI.  The email allowlist form writes both; `@public` is
-- an access *mode* on the channel rather than a grant, so it never confers UI read access, and the
-- owner and team rules stay in `Channel`'s domain policy rather than being denormalized here.
CREATE TABLE channel_principal_grants (
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    capability TEXT NOT NULL,
    provenance TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, channel_id, principal_id, capability),
    CONSTRAINT channel_principal_grants_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT channel_principal_grants_principal_fk
        FOREIGN KEY (company_id, principal_id)
        REFERENCES principals(company_id, id) ON DELETE CASCADE,
    CONSTRAINT channel_principal_grants_capability_check
        CHECK (capability IN ('participate', 'view')),
    CONSTRAINT channel_principal_grants_provenance_check
        CHECK (provenance IN (
            'configured_allowlist', 'manager', 'conversation_membership', 'system'
        ))
);

CREATE INDEX channel_principal_grants_principal_idx
    ON channel_principal_grants (company_id, principal_id, channel_id, capability);

-- One protocol-facing interface onto a business channel.
--
-- A channel is not an inbox and not a Slack conversation: it owns agents, policy and threads, and
-- exposes zero or more bindings. That is what lets a channel gain a second transport without a
-- nullable column per provider, and lets one interface be paused while the rest keeps working.
--
-- `installation_id` is NULL exactly for the deployment transports, which is `transport_requires_
-- installation` stated as a CHECK. The composite foreign key carries the tenancy *and* the
-- transport, so a binding can neither borrow another company's provider account nor point a Slack
-- binding at a non-Slack installation. NULL `installation_id` makes that MATCH SIMPLE key
-- unenforced, which is exactly the wanted behaviour for email -- the CHECK is what keeps the two
-- cases from blurring.
CREATE TABLE channel_bindings (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    installation_id UUID,
    transport TEXT NOT NULL,
    -- The scope in which `external_endpoint_key` is unique, and always an immutable identifier:
    -- the provider workspace for an installed transport, the company id for email. Nothing here is
    -- a slug -- `companies.slug` and `channel_slugs.slug` are both editable, and a key built from
    -- an editable value goes stale the moment someone edits it.
    namespace TEXT NOT NULL,
    external_endpoint_key TEXT NOT NULL,
    display_label TEXT NOT NULL,
    access_policy TEXT NOT NULL,
    delivery_policy TEXT NOT NULL,
    status TEXT NOT NULL,
    disabled_reason TEXT,
    created_by JSONB NOT NULL,
    -- What a human confirmed about the endpoint at link time. Confirmations only: no member lists,
    -- no provider responses, no message content.
    access_snapshot JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT channel_bindings_company_id_id_key UNIQUE (company_id, id),
    -- Carries the transport into the referencing key, so `message_deliveries` proves its stored
    -- `transport` is the one its destination interface actually speaks rather than re-asserting a
    -- literal. Same shape, same reason as `integration_installations_company_transport_key`.
    CONSTRAINT channel_bindings_company_transport_key UNIQUE (company_id, id, transport),
    CONSTRAINT channel_bindings_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT channel_bindings_installation_fk
        FOREIGN KEY (company_id, installation_id, transport)
        REFERENCES integration_installations(company_id, id, transport) ON DELETE CASCADE,
    CONSTRAINT channel_bindings_transport_check CHECK (transport IN ('email', 'slack')),
    CONSTRAINT channel_bindings_installation_coherence_check CHECK (
        transport_requires_installation(transport) = (installation_id IS NOT NULL)
    ),
    CONSTRAINT channel_bindings_namespace_check CHECK (
        btrim(namespace) <> '' AND octet_length(namespace) <= 255
    ),
    CONSTRAINT channel_bindings_endpoint_key_check CHECK (
        btrim(external_endpoint_key) <> '' AND octet_length(external_endpoint_key) <= 512
    ),
    CONSTRAINT channel_bindings_display_label_check CHECK (
        btrim(display_label) <> '' AND octet_length(display_label) <= 255
    ),
    CONSTRAINT channel_bindings_access_policy_check
        CHECK (access_policy IN ('channel_acl', 'conversation_members_read_and_participate')),
    CONSTRAINT channel_bindings_delivery_policy_check
        CHECK (delivery_policy IN ('reply_only', 'reply_and_initiate')),
    CONSTRAINT channel_bindings_status_check
        CHECK (status IN ('active', 'paused', 'disabled', 'orphaned')),
    -- A binding that stopped carrying traffic says why, and one that is carrying traffic cannot
    -- claim it was disabled for a reason.
    CONSTRAINT channel_bindings_disabled_reason_check CHECK (
        (status IN ('disabled', 'orphaned')) = (disabled_reason IS NOT NULL)
        AND (disabled_reason IS NULL OR valid_binding_change_reason(disabled_reason))
    ),
    CONSTRAINT channel_bindings_created_by_check CHECK (valid_creation_provenance(created_by)),
    CONSTRAINT channel_bindings_access_snapshot_check CHECK (
        jsonb_typeof(access_snapshot) = 'object'
        AND access_snapshot->'version' = '1'::JSONB
        AND jsonb_typeof(access_snapshot->'kind') = 'string'
        AND access_snapshot->>'kind' IN ('deployment_endpoint', 'provider_conversation')
        AND octet_length(access_snapshot::text) <= 4096
    )
);

-- `active` and `paused` are the statuses that still *claim* an endpoint; `disabled` and `orphaned`
-- release it so the same conversation can be linked to a different channel. The three partial
-- unique indexes below are all defined over that same set, and `BindingStatus::
-- holds_endpoint_claim` states it in Rust.

-- Two channels in one workspace cannot consume the same conversation. Scoped by installation
-- rather than by company because the installation is what the provider's ids are unique within.
CREATE UNIQUE INDEX channel_bindings_installed_endpoint_idx
    ON channel_bindings (installation_id, transport, namespace, external_endpoint_key)
    WHERE installation_id IS NOT NULL AND status IN ('active', 'paused');

-- The same rule for the deployment transports, written separately rather than folded into the
-- index above: a NULL `installation_id` makes a composite unique index match nothing, so one
-- combined index would let every email binding collide silently. For email this reads as one live
-- binding per (company, local part), which is the guarantee `channel_slugs` already makes.
CREATE UNIQUE INDEX channel_bindings_deployment_endpoint_idx
    ON channel_bindings (transport, namespace, external_endpoint_key)
    WHERE installation_id IS NULL AND status IN ('active', 'paused');

-- One canonical deployment interface per channel per transport, so a retried or concurrent
-- channel creation cannot leave a channel with two email bindings.
CREATE UNIQUE INDEX channel_bindings_canonical_deployment_idx
    ON channel_bindings (company_id, channel_id, transport)
    WHERE installation_id IS NULL AND status IN ('active', 'paused');

CREATE INDEX channel_bindings_channel_idx
    ON channel_bindings (company_id, channel_id, transport, status);

CREATE INDEX channel_bindings_installation_idx
    ON channel_bindings (installation_id, status)
    WHERE installation_id IS NOT NULL;

-- Append-only lifecycle history. Linking a private provider conversation is a read grant to
-- everyone in it, so who did it, when, and what they were shown has to survive the binding being
-- paused, re-enabled and disabled again.
CREATE TABLE binding_audit_events (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    binding_id UUID NOT NULL,
    action TEXT NOT NULL,
    reason TEXT,
    actor JSONB NOT NULL,
    -- Safe identifiers plus the confirmed access-policy snapshot. Never a credential, never a full
    -- provider response; `ChannelBinding::audit_metadata` is the only thing that builds it.
    metadata JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT binding_audit_events_binding_fk
        FOREIGN KEY (company_id, binding_id)
        REFERENCES channel_bindings(company_id, id) ON DELETE CASCADE,
    CONSTRAINT binding_audit_events_action_check CHECK (
        action IN ('linked', 'endpoint_changed', 'enabled', 'paused', 'disabled',
                   'drift_detected', 'unlinked')
    ),
    CONSTRAINT binding_audit_events_reason_check
        CHECK (reason IS NULL OR valid_binding_change_reason(reason)),
    CONSTRAINT binding_audit_events_actor_check CHECK (valid_creation_provenance(actor)),
    CONSTRAINT binding_audit_events_metadata_check CHECK (
        jsonb_typeof(metadata) = 'object'
        AND metadata->'version' = '1'::JSONB
        AND jsonb_typeof(metadata->'transport') = 'string'
        AND octet_length(metadata::text) <= 4096
    )
);

CREATE INDEX binding_audit_events_binding_idx
    ON binding_audit_events (company_id, binding_id, created_at DESC, id DESC);

-- Append-only means append-only. Deleting a company or a channel still cascades the history away
-- with the rows it describes, but nothing may rewrite what an audit row said it saw.
CREATE FUNCTION reject_binding_audit_rewrite() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' OR pg_trigger_depth() <= 1 THEN
        RAISE EXCEPTION 'binding_audit_events is append-only' USING ERRCODE = '23514';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER binding_audit_events_append_only
BEFORE UPDATE OR DELETE ON binding_audit_events
FOR EACH ROW EXECUTE FUNCTION reject_binding_audit_rewrite();

-- The durable boundary between a fast authenticated webhook acknowledgement and canonical
-- ingestion. Exact provider bytes live here only for the short incident/retry window; tasks and
-- canonical messages carry identifiers and bounded normalized content instead.

CREATE FUNCTION valid_inbound_event_error_class(class TEXT) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN class IN (
    'decode', 'invalid_payload', 'routing', 'dependency', 'rate_limited', 'provider_fault',
    'deadline', 'internal', 'unsupported_transport', 'lease_expired'
);

CREATE FUNCTION valid_inbound_event_ignore_reason(reason TEXT) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN reason IN (
    'not_message', 'unsupported_event', 'unsupported_subtype', 'automated_sender',
    'empty_content', 'inactive_binding', 'delivery_confirmation'
);

-- Header selection is the authenticating adapter's responsibility. This function only guarantees
-- that the selected diagnostic facts stay small, printable, and structurally predictable.
CREATE FUNCTION valid_inbound_safe_header_facts(facts JSONB) RETURNS BOOLEAN
LANGUAGE SQL
IMMUTABLE
RETURN jsonb_typeof(facts) = 'object'
   AND (SELECT COUNT(*) <= 16 FROM jsonb_object_keys(facts))
   AND octet_length(facts::TEXT) <= 4096
   AND NOT EXISTS (
       SELECT 1
         FROM jsonb_each(facts) AS fact(name, value)
        WHERE fact.name !~ '^[a-z0-9_]{1,64}$'
           OR jsonb_typeof(fact.value) <> 'string'
           OR octet_length(fact.value #>> '{}') NOT BETWEEN 1 AND 256
           OR fact.value #>> '{}' ~ '[[:cntrl:]]'
   );

CREATE TABLE inbound_events (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    installation_id UUID,
    transport TEXT NOT NULL,
    external_event_key TEXT NOT NULL,
    correlation_id UUID NOT NULL,
    raw_payload BYTEA NOT NULL,
    content_type TEXT,
    content_hash BYTEA NOT NULL,
    safe_header_facts JSONB NOT NULL DEFAULT '{}'::JSONB,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_error_class TEXT,
    last_error_detail TEXT,
    ignore_reason TEXT,
    execution_id UUID,
    owner_worker_id UUID,
    locked_at TIMESTAMPTZ,
    lock_expires_at TIMESTAMPTZ,
    received_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT inbound_events_company_id_id_key UNIQUE (company_id, id),
    -- Slack event_id is globally unique. A future provider with tenant-local delivery ids must
    -- change this documented key to include installation_id before it can use this inbox.
    CONSTRAINT inbound_events_transport_external_event_key
        UNIQUE (transport, external_event_key),
    -- The three-column reference proves installation, company and discriminator agree. Its NULL
    -- behavior permits deployment transports; the check immediately below says exactly which
    -- transports are allowed to use that arm.
    CONSTRAINT inbound_events_installation_fk
        FOREIGN KEY (company_id, installation_id, transport)
        REFERENCES integration_installations(company_id, id, transport) ON DELETE CASCADE,
    CONSTRAINT inbound_events_installation_check CHECK (
        transport_requires_installation(transport) = (installation_id IS NOT NULL)
    ),
    CONSTRAINT inbound_events_transport_check CHECK (transport IN ('email', 'slack')),
    CONSTRAINT inbound_events_external_event_key_check CHECK (
        btrim(external_event_key) <> '' AND octet_length(external_event_key) <= 512
    ),
    -- This is the same 1 MiB boundary exported as MAX_INBOUND_EVENT_PAYLOAD_BYTES. The HTTP route
    -- rejects before allocation; this check prevents a second writer bypassing that guard.
    CONSTRAINT inbound_events_payload_check CHECK (
        octet_length(raw_payload) BETWEEN 1 AND 1048576
    ),
    CONSTRAINT inbound_events_content_type_check CHECK (
        content_type IS NULL
        OR (btrim(content_type) <> '' AND octet_length(content_type) <= 255
            AND content_type !~ '[[:cntrl:]]')
    ),
    CONSTRAINT inbound_events_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT inbound_events_safe_header_facts_check
        CHECK (valid_inbound_safe_header_facts(safe_header_facts)),
    CONSTRAINT inbound_events_status_check CHECK (status IN (
        'pending', 'processing', 'retryable', 'completed', 'ignored', 'dead_letter'
    )),
    CONSTRAINT inbound_events_attempt_check CHECK (
        attempt_count >= 0 AND max_attempts > 0 AND attempt_count <= max_attempts
    ),
    CONSTRAINT inbound_events_error_check CHECK (
        (last_error_class IS NULL OR valid_inbound_event_error_class(last_error_class))
        AND (last_error_detail IS NULL OR octet_length(last_error_detail) <= 512)
        AND (last_error_detail IS NULL OR last_error_class IS NOT NULL)
        AND ((status IN ('retryable', 'dead_letter')) = (last_error_class IS NOT NULL))
    ),
    CONSTRAINT inbound_events_ignore_check CHECK (
        (status = 'ignored') = (ignore_reason IS NOT NULL)
        AND (ignore_reason IS NULL OR valid_inbound_event_ignore_reason(ignore_reason))
    ),
    CONSTRAINT inbound_events_lease_check CHECK (
        (status = 'processing'
         AND execution_id IS NOT NULL
         AND owner_worker_id IS NOT NULL
         AND locked_at IS NOT NULL
         AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'processing'
         AND execution_id IS NULL
         AND owner_worker_id IS NULL
         AND locked_at IS NULL
         AND lock_expires_at IS NULL)
    ),
    CONSTRAINT inbound_events_processed_check CHECK (
        (status IN ('completed', 'ignored', 'dead_letter')) = (processed_at IS NOT NULL)
    )
);

CREATE INDEX inbound_events_claimable_idx
    ON inbound_events (available_at, received_at, id)
    WHERE status IN ('pending', 'retryable');
CREATE INDEX inbound_events_processing_lease_idx
    ON inbound_events (lock_expires_at, id) WHERE status = 'processing';
-- Retention walks terminal rows by status and processed time and deletes in bounded batches.
CREATE INDEX inbound_events_processed_retention_idx
    ON inbound_events (status, processed_at, id)
    WHERE status IN ('completed', 'ignored', 'dead_letter');
CREATE INDEX inbound_events_company_installation_created_idx
    ON inbound_events (company_id, installation_id, created_at DESC, id DESC);

-- A hint only. Polling and startup reconciliation remain authoritative, so a dropped notification
-- cannot lose work and a process on another machine can recover without receiving this payload.
CREATE FUNCTION notify_inbound_event_ready() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status IN ('pending', 'retryable')
       AND (TG_OP = 'INSERT' OR OLD.status IS DISTINCT FROM NEW.status
            OR OLD.available_at IS DISTINCT FROM NEW.available_at) THEN
        PERFORM pg_notify('inbound_event_ready', NEW.id::TEXT);
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER inbound_events_notify_ready
AFTER INSERT OR UPDATE OF status, available_at ON inbound_events
FOR EACH ROW EXECUTE FUNCTION notify_inbound_event_ready();

-- One conversation inside one business channel.
--
-- Deliberately carries no provider key of its own: a thread may be bound to email and to several
-- Slack conversations at once, so the mapping lives one-to-many in `external_threads`.
CREATE TABLE threads (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    subject TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT threads_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT threads_company_channel_id_key UNIQUE (company_id, channel_id, id),
    CONSTRAINT threads_channel_id_key UNIQUE (channel_id, id),
    CONSTRAINT threads_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE
);

CREATE INDEX threads_channel_updated_idx
    ON threads (channel_id, updated_at DESC, id DESC);

-- Who is a party to a thread, and in what capacity.  `role` carries the author/participant
-- distinction that an email array could only imply by position.
CREATE TABLE thread_principals (
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    role TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, channel_id, thread_id, principal_id, role),
    CONSTRAINT thread_principals_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT thread_principals_principal_fk
        FOREIGN KEY (company_id, principal_id)
        REFERENCES principals(company_id, id) ON DELETE CASCADE,
    CONSTRAINT thread_principals_role_check CHECK (role IN ('author', 'participant'))
);

CREATE INDEX thread_principals_principal_idx
    ON thread_principals (company_id, principal_id, thread_id, role);

-- The canonical payload of one message, whatever carried it: an email, a Slack post, a schedule's
-- prompt, an approval note, or an agent's answer.
--
-- Nothing here is email-shaped. The author is a principal, not an address; the subject is a plain
-- string; protocol headers and provider keys live in `email_message_metadata`, `external_messages`
-- and `external_threads`. A message stored once may be associated with several threads through
-- `thread_messages` and delivered through several bindings without a second copy of its body.
CREATE TABLE messages (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    -- Who said it. Always a principal, so "the same person over two transports" is one actor.
    author_principal_id UUID NOT NULL,
    -- Which of that principal's handles said it, when a transport named one. A schedule prompt
    -- and a system note have an author but no handle, which is why this is nullable.
    authored_identity_id UUID,
    subject TEXT NOT NULL,
    clean_text_body TEXT NOT NULL,
    attachments JSONB,
    direction TEXT NOT NULL,
    role TEXT NOT NULL,
    -- The chain this message belongs to, minted at ingress and shared with the task it causes.
    correlation_id UUID NOT NULL,
    -- Over one canonical payload, so a provider redelivering the same key with different content
    -- is a detectable collision rather than a silent rewrite. See `canonical_message_hash`.
    content_hash BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT messages_company_id_id_key UNIQUE (company_id, id),
    -- Composite, so a message can never name an author or handle from another company.
    CONSTRAINT messages_author_principal_fk
        FOREIGN KEY (company_id, author_principal_id)
        REFERENCES principals(company_id, id) ON DELETE CASCADE,
    CONSTRAINT messages_authored_identity_fk
        FOREIGN KEY (company_id, authored_identity_id)
        REFERENCES participant_identities(company_id, id)
        ON DELETE SET NULL (authored_identity_id),
    CONSTRAINT messages_authored_identity_author_fk
        FOREIGN KEY (company_id, author_principal_id, authored_identity_id)
        REFERENCES participant_identities(company_id, principal_id, id)
        ON DELETE SET NULL (authored_identity_id),
    CONSTRAINT messages_direction_check CHECK (direction IN ('inbound', 'outbound')),
    CONSTRAINT messages_role_check CHECK (role IN ('human', 'agent', 'system')),
    CONSTRAINT messages_content_hash_check CHECK (octet_length(content_hash) = 32),
    CONSTRAINT messages_subject_check CHECK (octet_length(subject) <= 2048),
    -- Attachment metadata arrives from outside and is decoded long after it was written, so it is
    -- stored as a versioned, discriminated, bounded envelope and decoded fallibly in Rust -- a
    -- structurally valid object may still carry a version a rolling deploy has not learned yet.
    CONSTRAINT messages_attachments_check CHECK (
        attachments IS NULL
        OR (
            jsonb_typeof(attachments) = 'object'
            -- The discriminator is a JSON *string* because `MessageAttachments` is an
            -- internally-tagged Rust enum, whose variant name is what serde writes here. A
            -- number would decode as a different shape entirely.
            AND attachments->'version' = '"1"'::JSONB
            AND jsonb_typeof(attachments->'items') = 'array'
            AND octet_length(attachments::text) <= 262144
        )
    )
);

CREATE INDEX messages_company_created_idx ON messages (company_id, created_at DESC, id DESC);
CREATE INDEX messages_author_idx ON messages (company_id, author_principal_id);

-- The sender/to/cc projection of a message, for the transports that have one.
--
-- `position` is what makes a rendered `To:` header reproducible: the order a message was addressed
-- in is data, not an accident of how a query happened to sort. A transport without recipient
-- vocabulary -- Slack, a schedule prompt -- simply writes no rows here.
CREATE TABLE message_participants (
    company_id UUID NOT NULL,
    message_id UUID NOT NULL,
    participant_identity_id UUID NOT NULL,
    kind TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (message_id, kind, position),
    CONSTRAINT message_participants_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT message_participants_identity_fk
        FOREIGN KEY (company_id, participant_identity_id)
        REFERENCES participant_identities(company_id, id) ON DELETE CASCADE,
    -- One handle appears at most once per role, so a duplicated `Cc` cannot become two rows that
    -- render the same address twice.
    CONSTRAINT message_participants_identity_kind_key
        UNIQUE (message_id, kind, participant_identity_id),
    CONSTRAINT message_participants_kind_check CHECK (kind IN ('sender', 'to', 'cc')),
    CONSTRAINT message_participants_position_check CHECK (position >= 0)
);

CREATE INDEX message_participants_identity_idx
    ON message_participants (company_id, participant_identity_id, message_id);

-- The email protocol extension of a canonical message: the headers and raw representations that
-- only mail has, kept out of `messages` so a Slack post needs none of them.
--
-- `rfc_message_id` is deliberately *not* unique per company. A Message-ID identifies a mail on the
-- wire, not a message this company holds: when one channel's agent mails another, the same
-- Message-ID is one outbound message on the sending channel's binding and one inbound message on
-- the receiving channel's, with different bodies, different directions and different threads. The
-- pre-canonical schema forced those into one row keyed by Message-ID and then had to demand that
-- both writers produce byte-identical content -- a coupling that silently broke every
-- inter-channel delegation the moment one side stored a raw body the other did not. Dedup belongs
-- to `external_messages (binding_id, external_message_key)`, which is the provider key qualified
-- by the interface that carried it.
--
-- Email authentication (SPF/DKIM/DMARC) and spam scoring are deliberately absent. They are ingress
-- guards, consumed before a message exists at all -- see `check_inbound_guards` -- and nothing
-- downstream reads them back. A field is retained here only because something reads it.
CREATE TABLE email_message_metadata (
    company_id UUID NOT NULL,
    message_id UUID NOT NULL,
    rfc_message_id TEXT NOT NULL,
    in_reply_to TEXT,
    references_list TEXT[] NOT NULL DEFAULT '{}',
    thread_index TEXT,
    raw_text_body TEXT,
    raw_html_body TEXT,
    PRIMARY KEY (message_id),
    CONSTRAINT email_message_metadata_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT email_message_metadata_rfc_message_id_check CHECK (
        btrim(rfc_message_id) <> '' AND octet_length(rfc_message_id) <= 998
    ),
    CONSTRAINT email_message_metadata_in_reply_to_check CHECK (
        in_reply_to IS NULL OR octet_length(in_reply_to) <= 998
    ),
    CONSTRAINT email_message_metadata_thread_index_check CHECK (
        thread_index IS NULL OR octet_length(thread_index) <= 998
    ),
    -- Bounded because the whole array is read into memory to build a threading lookup key.
    CONSTRAINT email_message_metadata_references_check CHECK (
        array_length(references_list, 1) IS NULL OR array_length(references_list, 1) <= 100
    )
);

-- Thread resolution and the ingress duplicate check both look a Message-ID up inside one company.
CREATE INDEX email_message_metadata_company_rfc_idx
    ON email_message_metadata (company_id, rfc_message_id);
CREATE INDEX email_message_metadata_in_reply_to_idx
    ON email_message_metadata (company_id, in_reply_to) WHERE in_reply_to IS NOT NULL;
CREATE INDEX email_message_metadata_thread_index_idx
    ON email_message_metadata (company_id, thread_index) WHERE thread_index IS NOT NULL;

-- Which provider conversation, on which binding, a canonical thread is reachable as.
--
-- One thread may have many rows: the same conversation can run over the channel's email binding
-- and over two Slack conversations at once. The key is opaque here -- the owning adapter decides
-- what a thread key is (`thread_ts.unwrap_or(ts)` for Slack, an RFC root for mail) -- so nothing
-- in the database or the application parses it.
CREATE TABLE external_threads (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    binding_id UUID NOT NULL,
    external_thread_key TEXT NOT NULL,
    thread_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT external_threads_company_id_id_key UNIQUE (company_id, id),
    -- One provider conversation resolves to exactly one internal thread; the same key in another
    -- binding is a different conversation and collides with nothing.
    CONSTRAINT external_threads_binding_key_key UNIQUE (binding_id, external_thread_key),
    CONSTRAINT external_threads_binding_fk
        FOREIGN KEY (company_id, binding_id)
        REFERENCES channel_bindings(company_id, id) ON DELETE CASCADE,
    CONSTRAINT external_threads_thread_fk
        FOREIGN KEY (company_id, thread_id)
        REFERENCES threads(company_id, id) ON DELETE CASCADE,
    CONSTRAINT external_threads_key_check CHECK (
        btrim(external_thread_key) <> '' AND octet_length(external_thread_key) <= 998
    )
);

CREATE INDEX external_threads_thread_idx
    ON external_threads (company_id, thread_id, binding_id);

-- Which provider message, on which binding, a canonical message was carried as.
--
-- This is the dedup key for redelivery: a provider replaying an event finds its own key here and
-- the existing canonical message is returned instead of a second one being written.
CREATE TABLE external_messages (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    binding_id UUID NOT NULL,
    external_message_key TEXT NOT NULL,
    message_id UUID NOT NULL,
    -- Which part of an outbound delivery produced this provider message. A long answer is sent as
    -- several provider messages, so the mapping is per part rather than per message: several rows
    -- here can point at one canonical message while each names the part that carried it.
    --
    -- `NULL` for an inbound mapping, which is every message that arrived from outside. The
    -- reference is added at the end of the file, after `message_delivery_parts` exists.
    delivery_part_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT external_messages_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT external_messages_binding_key_key UNIQUE (binding_id, external_message_key),
    CONSTRAINT external_messages_binding_fk
        FOREIGN KEY (company_id, binding_id)
        REFERENCES channel_bindings(company_id, id) ON DELETE CASCADE,
    CONSTRAINT external_messages_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT external_messages_key_check CHECK (
        btrim(external_message_key) <> '' AND octet_length(external_message_key) <= 998
    )
);

CREATE INDEX external_messages_message_idx
    ON external_messages (company_id, message_id, binding_id);

-- One canonical message's membership of one thread.
--
-- Carries no payload of its own: the body, role and direction belong to the message, and this row
-- says only that the message is part of this conversation. `id` is the association identity the
-- UI and `task_outreach_targets.response_association_id` name, so a message in two threads has two
-- addressable rows.
CREATE TABLE thread_messages (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    message_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT thread_messages_thread_message_key UNIQUE (thread_id, message_id),
    -- A message lands in at most one thread per channel: a second thread in the same channel would
    -- split one conversation in two for the same audience.
    CONSTRAINT thread_messages_channel_message_key UNIQUE (channel_id, message_id),
    -- All three tenancy columns are in the key, so a message cannot name a thread that belongs to
    -- another company or another channel than the one it recorded.
    CONSTRAINT thread_messages_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT thread_messages_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE CASCADE
);

CREATE INDEX thread_messages_thread_created_idx
    ON thread_messages (thread_id, created_at, id);
CREATE INDEX thread_messages_message_thread_idx
    ON thread_messages (message_id, thread_id);

CREATE FUNCTION delete_orphan_message() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM messages message
    WHERE message.id = OLD.message_id
      AND NOT EXISTS (
          SELECT 1 FROM thread_messages association
          WHERE association.message_id = OLD.message_id
      );
    RETURN NULL;
END;
$$;

CREATE TRIGGER thread_messages_delete_orphan_message
AFTER DELETE ON thread_messages
FOR EACH ROW EXECUTE FUNCTION delete_orphan_message();

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

-- INSERT only, and the association insert is `ON CONFLICT DO NOTHING`: a redelivered provider
-- message writes no second row here and so announces nothing, which is what stops a duplicate
-- bubble appearing in an open mailbox. The body lives on `messages` and is never rewritten by the
-- association, so there is nothing else here worth announcing.
CREATE TRIGGER thread_messages_notify
    AFTER INSERT ON thread_messages
    FOR EACH ROW
    EXECUTE FUNCTION notify_thread_message();

CREATE TABLE background_tasks (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL,
    thread_id UUID,
    -- The canonical message this task was queued for, when one caused it. Tenant-scoped and
    -- unique, so a redelivered provider message finds the task its first delivery created instead
    -- of starting a second run of the same work.
    source_message_uuid UUID,
    -- The same guarantee for the one source that is not a message: a schedule slot coming due.
    -- No foreign key, because `schedule_runs` names `background_tasks` and Postgres cannot create
    -- the pair in either order; `schedule_runs_schedule_slot_key` is what makes the slot unique in
    -- the first place, and this makes the task it materializes unique too.
    source_schedule_run_id UUID,
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
    CONSTRAINT background_tasks_company_source_message_key
        UNIQUE (company_id, source_message_uuid),
    CONSTRAINT background_tasks_company_source_schedule_run_key
        UNIQUE (company_id, source_schedule_run_id),
    -- A task names at most one source. Both set would make "which redelivery does this dedup
    -- against?" ambiguous, and the two unique keys above would each answer differently.
    CONSTRAINT background_tasks_single_source_check CHECK (
        source_message_uuid IS NULL OR source_schedule_run_id IS NULL
    ),
    CONSTRAINT background_tasks_source_message_fk
        FOREIGN KEY (company_id, source_message_uuid)
        REFERENCES messages(company_id, id) ON DELETE SET NULL (source_message_uuid),
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
    warnings JSONB NOT NULL DEFAULT '[]'::jsonb,
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
    -- Which worker run produced this attempt. `background_tasks.worker_id` is a lease and is
    -- nulled the moment the run ends, so the ledger is the only durable answer to "who ran this".
    worker_id UUID NOT NULL,
    -- Where that run executed: FLY_MACHINE_ID, or a per-boot `local-<uuid>` off Fly. Denormalized
    -- onto the attempt on purpose -- there is no worker registry to join to, and a ledger row is
    -- the record of what was true at the time, not a pointer to what is true now.
    machine_id TEXT NOT NULL,
    machine_region TEXT,
    CONSTRAINT task_attempts_task_attempt_key UNIQUE (task_id, attempt_number),
    CONSTRAINT task_attempts_status_check CHECK (status IN ('processing', 'completed', 'failed')),
    CONSTRAINT task_attempts_machine_id_check CHECK (length(trim(machine_id)) > 0),
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
    -- The conversation the approval concerns. Not nullable: the request is written as a system
    -- message in this thread and delivered from it, so an approval with no thread is one nobody
    -- could be told about.
    thread_id UUID NOT NULL,
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
    CONSTRAINT human_approvals_thread_step_key UNIQUE
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

-- One durable attempt to expose one canonical message through one protocol interface.
--
-- The generic queue avoids transport-shaped assumptions: one provider result per
-- row (`provider_message_id`), a single flat status vocabulary that could not tell "the provider
-- refused this" from "the connection dropped after the request went out", and a lease that lived
-- on the same row as the thing being sent. A chat provider splits one answer into several posts,
-- each with its own provider key, and its ambiguous outcomes must never be blind-retried.
--
-- So a delivery owns the lease and the retry budget; `message_delivery_parts` owns the provider
-- results. There is exactly one leased object per provider call chain, which is what keeps a
-- multi-part send from growing a second ownership state machine.
--
-- `transport` is stored rather than derived at read time so a claim does not have to join the
-- binding to know which adapter to hand the row to; `message_deliveries_transport_matches_binding`
-- proves the copy agrees with the binding it came from.
CREATE TABLE message_deliveries (
    id UUID PRIMARY KEY,
    company_id UUID REFERENCES companies(id) ON DELETE CASCADE,
    -- The business channel whose interface carries this. Canonical deliveries require it; the
    -- attribution check permits NULL only for a standalone rejection notification.
    channel_id UUID,
    -- The canonical message being exposed. Canonical deliveries require it. A pre-ingest
    -- rejection has no accepted message to invent, so the standalone notification arm omits it.
    message_id UUID,
    -- The interface the message came from, or that its producing channel speaks through. Recorded
    -- so fan-out can exclude it: delivering a message back to its own interface is an echo.
    source_binding_id UUID,
    -- The interface that actually carries this delivery. Deduplication is scoped to it.
    destination_binding_id UUID,
    -- The recipient named inside the destination interface's own namespace, when the destination
    -- is an address rather than the interface itself: an outreach recipient, the customer a reply
    -- answers. `NULL` means the interface *is* the destination, which is what a mirror is.
    external_destination TEXT,
    -- The task whose work produced this. Carries no lifecycle meaning -- it is the join the task
    -- view uses to show delivery state, and nothing writes back through it.
    task_id UUID,
    -- The delivery that has to land first. A chat mirror cannot post a reply until the root post
    -- it threads under exists, and the claim below refuses this row until that one is delivered.
    depends_on_delivery_id UUID,
    -- Inherited from whatever produced this. Unlike `task_id` it is never cleared, so a delivered
    -- message stays attached to its trail even after the task row is gone.
    correlation_id UUID NOT NULL,
    transport TEXT NOT NULL,
    purpose TEXT NOT NULL,
    -- Stable across every attempt at the same logical delivery, and derived from the purpose, the
    -- message and the destination rather than from the attempt. It is the lock that makes creation
    -- idempotent, and what the delivered provider key is derived from.
    idempotency_key TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL,
    available_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Why the last attempt ended, as a typed class plus a bounded detail. Typed because an
    -- operator alert has to tell a revoked credential from a rate limit, and because recovering
    -- that by matching substrings of a free-text error is not classification.
    last_error_class TEXT,
    last_error_detail TEXT,
    -- The fence. Minted fresh by every claim, and named in the `WHERE` clause of every renewal,
    -- part transition, completion and failure, so a superseded run cannot report a result over
    -- the execution that replaced it.
    execution_id UUID,
    owner_worker_id UUID,
    locked_at TIMESTAMPTZ,
    lock_expires_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT message_deliveries_company_id_id_key UNIQUE (company_id, id),
    -- One logical delivery per destination interface. The destination is already inside
    -- `idempotency_key`, so this absorbs a retried planning step rather than enqueuing a second
    -- send, while two outreach recipients on one message stay two rows.
    CONSTRAINT message_deliveries_destination_key_key
        UNIQUE (destination_binding_id, idempotency_key),
    -- Every composite reference proves the referenced row belongs to the same company, so a
    -- delivery cannot name another tenant's channel, message, interface or task.
    CONSTRAINT message_deliveries_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT message_deliveries_message_fk
        FOREIGN KEY (company_id, message_id)
        REFERENCES messages(company_id, id) ON DELETE CASCADE,
    CONSTRAINT message_deliveries_source_binding_fk
        FOREIGN KEY (company_id, source_binding_id)
        REFERENCES channel_bindings(company_id, id) ON DELETE CASCADE,
    -- Carried, not re-asserted: the pair proves the destination interface both belongs to this
    -- company *and* speaks the transport this row says it does, so a claim can trust the stored
    -- `transport` without joining.
    CONSTRAINT message_deliveries_destination_binding_fk
        FOREIGN KEY (company_id, destination_binding_id, transport)
        REFERENCES channel_bindings(company_id, id, transport) ON DELETE CASCADE,
    CONSTRAINT message_deliveries_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE SET NULL (task_id),
    -- Self-referential and same-company. A dependency in another company would let one tenant's
    -- stuck root hold another tenant's delivery closed for ever.
    CONSTRAINT message_deliveries_dependency_fk
        FOREIGN KEY (company_id, depends_on_delivery_id)
        REFERENCES message_deliveries(company_id, id) ON DELETE SET NULL (depends_on_delivery_id),
    CONSTRAINT message_deliveries_no_self_dependency_check
        CHECK (depends_on_delivery_id IS NULL OR depends_on_delivery_id <> id),
    CONSTRAINT message_deliveries_attribution_check CHECK (
        (company_id IS NOT NULL
         AND channel_id IS NOT NULL
         AND message_id IS NOT NULL
         AND source_binding_id IS NOT NULL
         AND destination_binding_id IS NOT NULL)
        OR
        (company_id IS NULL
         AND channel_id IS NULL
         AND message_id IS NULL
         AND source_binding_id IS NULL
         AND destination_binding_id IS NULL
         AND task_id IS NULL
         AND depends_on_delivery_id IS NULL
         AND external_destination IS NOT NULL
         AND purpose = 'notification')
    ),
    CONSTRAINT message_deliveries_transport_check CHECK (transport IN ('email', 'slack')),
    CONSTRAINT message_deliveries_purpose_check
        CHECK (purpose IN ('reply', 'mirror', 'outreach', 'notification')),
    CONSTRAINT message_deliveries_status_check CHECK (status IN (
        'pending', 'sending', 'retryable', 'delivered', 'outcome_unknown', 'dead_letter'
    )),
    CONSTRAINT message_deliveries_attempt_check
        CHECK (attempt_count >= 0 AND max_attempts > 0 AND attempt_count <= max_attempts),
    CONSTRAINT message_deliveries_idempotency_key_check CHECK (
        btrim(idempotency_key) <> '' AND octet_length(idempotency_key) <= 512
    ),
    CONSTRAINT message_deliveries_external_destination_check CHECK (
        external_destination IS NULL
        OR (btrim(external_destination) <> '' AND octet_length(external_destination) <= 998)
    ),
    CONSTRAINT message_deliveries_error_check CHECK (
        (last_error_class IS NULL OR valid_delivery_failure_class(last_error_class))
        AND (last_error_detail IS NULL OR octet_length(last_error_detail) <= 512)
        -- A detail with no class is an unclassified failure wearing a sentence, which is the shape
        -- `src/adapters/persistence/AGENTS.md` forbids for an audited transition.
        AND (last_error_detail IS NULL OR last_error_class IS NOT NULL)
    ),
    -- Lease metadata belongs to 'sending' and to nothing else. Without the second arm a terminal
    -- row keeps the worker id that last touched it, and a stale lease on a finished row reads as
    -- an in-flight delivery to anything sweeping for expired ones.
    CONSTRAINT message_deliveries_lease_check CHECK (
        (status = 'sending'
         AND execution_id IS NOT NULL
         AND owner_worker_id IS NOT NULL
         AND locked_at IS NOT NULL
         AND lock_expires_at IS NOT NULL
         AND lock_expires_at > locked_at)
        OR
        (status <> 'sending'
         AND execution_id IS NULL
         AND owner_worker_id IS NULL
         AND locked_at IS NULL
         AND lock_expires_at IS NULL)
    ),
    -- Only a delivered row has a delivery time, and it must have one.
    CONSTRAINT message_deliveries_delivered_at_check
        CHECK ((status = 'delivered') = (delivered_at IS NOT NULL))
);

-- The claim's own index: `status IN ('pending','retryable') AND available_at <= now`, ordered by
-- `(available_at, id)`. Both claimable statuses share one partial index because the claim takes
-- them together -- a row that failed and backed off is the same work as one that never ran.
CREATE INDEX message_deliveries_claimable_idx
    ON message_deliveries (available_at, id)
    WHERE status IN ('pending', 'retryable');
CREATE INDEX message_deliveries_sending_lease_idx
    ON message_deliveries (lock_expires_at, id) WHERE status = 'sending';
CREATE INDEX message_deliveries_company_created_idx
    ON message_deliveries (company_id, created_at DESC, id DESC);
CREATE INDEX message_deliveries_company_channel_created_idx
    ON message_deliveries (company_id, channel_id, created_at DESC, id DESC);
CREATE INDEX message_deliveries_correlation_idx
    ON message_deliveries (correlation_id, created_at);
CREATE INDEX message_deliveries_task_idx
    ON message_deliveries (task_id) WHERE task_id IS NOT NULL;
-- The board's delivery-side recency arm and its unfinished arm; see
-- `background_tasks_company_updated_idx`. Both are needed for the board's
-- `status IN (...) OR updated_at >= cutoff` disjunction to come out as a BitmapOr of two index
-- scans rather than a sequential scan.
CREATE INDEX message_deliveries_company_updated_idx
    ON message_deliveries (company_id, updated_at DESC);
CREATE INDEX message_deliveries_company_status_idx
    ON message_deliveries (company_id, status);
CREATE INDEX message_deliveries_message_idx
    ON message_deliveries (company_id, message_id);
-- Read by the claim (per candidate row) and by the sweep that dead-letters descendants of a
-- dependency that can never be delivered.
CREATE INDEX message_deliveries_dependency_idx
    ON message_deliveries (depends_on_delivery_id)
    WHERE depends_on_delivery_id IS NOT NULL;
CREATE UNIQUE INDEX message_deliveries_standalone_key_key
    ON message_deliveries (transport, idempotency_key)
    WHERE destination_binding_id IS NULL;

-- One frozen piece of a delivery, and what its provider said about it.
--
-- Parts are rendered and written before the first provider call, so a retry sends the bytes that
-- were frozen rather than re-rendering against a display name or a policy that has since changed.
-- They own no lease: every transition here is fenced on the parent's live `execution_id`, which is
-- why `begin_part`/`complete_part` take the parent's execution rather than a claim of their own.
CREATE TABLE message_delivery_parts (
    id UUID PRIMARY KEY,
    company_id UUID,
    delivery_id UUID NOT NULL,
    part_index INTEGER NOT NULL,
    -- Stable across re-renders of the same delivery, and derived from the delivery's idempotency
    -- key rather than from its id: whoever froze these parts computed the key before the row
    -- existed, and an outbound RFC Message-ID is derived from it so a queuer can record the
    -- message it will send under before it is sent.
    part_key TEXT NOT NULL,
    -- The rendered wire payload, in the owning adapter's own shape. Versioned and transport-tagged
    -- inside the object and decoded fallibly, so a payload written by a newer renderer is an error
    -- at the seam rather than a misread field halfway through a provider call. Never a credential
    -- and never an authorization header.
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'prepared',
    -- The provider's own key for what it stored: an RFC Message-ID, a chat timestamp. One per
    -- part, because a long answer is several provider messages.
    provider_message_key TEXT,
    -- What a reconciliation lookup compares against when a provider outcome was ambiguous. Derived
    -- from the rendered body alone, so it is safe to carry in provider metadata.
    content_digest TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error_class TEXT,
    last_error_detail TEXT,
    -- Committed immediately before the external call, and the whole reason a crash can be
    -- classified. A part whose lease lapsed without this set never reached the provider and is
    -- retryable; one with it set may have been accepted and becomes `outcome_unknown`.
    request_started_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT message_delivery_parts_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT message_delivery_parts_delivery_index_key UNIQUE (delivery_id, part_index),
    CONSTRAINT message_delivery_parts_delivery_key_key UNIQUE (delivery_id, part_key),
    CONSTRAINT message_delivery_parts_delivery_fk
        FOREIGN KEY (company_id, delivery_id)
        REFERENCES message_deliveries(company_id, id) ON DELETE CASCADE,
    CONSTRAINT message_delivery_parts_delivery_id_fk
        FOREIGN KEY (delivery_id) REFERENCES message_deliveries(id) ON DELETE CASCADE,
    CONSTRAINT message_delivery_parts_index_check
        CHECK (part_index >= 0 AND part_index < 50),
    CONSTRAINT message_delivery_parts_status_check CHECK (status IN (
        'prepared', 'sending', 'delivered', 'outcome_unknown', 'retryable', 'dead'
    )),
    CONSTRAINT message_delivery_parts_key_check CHECK (
        btrim(part_key) <> '' AND octet_length(part_key) <= 200
    ),
    CONSTRAINT message_delivery_parts_digest_check CHECK (
        btrim(content_digest) <> '' AND octet_length(content_digest) <= 128
    ),
    CONSTRAINT message_delivery_parts_provider_key_check CHECK (
        provider_message_key IS NULL
        OR (btrim(provider_message_key) <> '' AND octet_length(provider_message_key) <= 998)
    ),
    -- Bounded here as well as in Rust: the payload is read back into memory by whichever instance
    -- claims the row, and a bound only the writer enforces is not a bound.
    CONSTRAINT message_delivery_parts_payload_check CHECK (
        jsonb_typeof(payload) = 'object'
        AND jsonb_typeof(payload->'transport') = 'string'
        AND jsonb_typeof(payload->'version') = 'number'
        AND octet_length(payload::text) <= 262144
    ),
    CONSTRAINT message_delivery_parts_attempt_check CHECK (attempt_count >= 0),
    CONSTRAINT message_delivery_parts_error_check CHECK (
        (last_error_class IS NULL OR valid_delivery_failure_class(last_error_class))
        AND (last_error_detail IS NULL OR octet_length(last_error_detail) <= 512)
        AND (last_error_detail IS NULL OR last_error_class IS NOT NULL)
    ),
    -- A delivered part has a delivery time and nothing else does; and a part cannot claim the
    -- provider accepted it without having started the request that carried it.
    CONSTRAINT message_delivery_parts_delivered_at_check
        CHECK ((status = 'delivered') = (delivered_at IS NOT NULL)),
    CONSTRAINT message_delivery_parts_started_check CHECK (
        status <> 'delivered' OR request_started_at IS NOT NULL
    )
);

-- Delivery resumes at the first unfinished part, in order.
CREATE INDEX message_delivery_parts_delivery_idx
    ON message_delivery_parts (delivery_id, part_index);
CREATE INDEX message_delivery_parts_unfinished_idx
    ON message_delivery_parts (delivery_id, part_index)
    WHERE status IN ('prepared', 'retryable');
-- The reply guard matches a third party's `References:` against the provider key an outreach went
-- out under, so this is read per candidate rather than scanned.
CREATE UNIQUE INDEX message_delivery_parts_provider_key_idx
    ON message_delivery_parts (company_id, provider_message_key)
    WHERE provider_message_key IS NOT NULL;

-- Declared here rather than inside `external_messages`, which is created several hundred lines
-- earlier: the mapping table has to exist before the ingress path can write an inbound row, and
-- the part table has to exist before this reference can be made. Composite, so a provider mapping
-- cannot name another tenant's delivery part, and `SET NULL` so retiring a delivery leaves the
-- provider mapping that proves the message went out.
ALTER TABLE external_messages
    ADD CONSTRAINT external_messages_delivery_part_fk
    FOREIGN KEY (company_id, delivery_part_id)
    REFERENCES message_delivery_parts(company_id, id) ON DELETE SET NULL (delivery_part_id);

CREATE INDEX external_messages_delivery_part_idx
    ON external_messages (delivery_part_id) WHERE delivery_part_id IS NOT NULL;

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
    -- The *association* the reply landed on -- `thread_messages.id`, not `messages.id`. Named for
    -- the table it points at, because its sibling `request_message_id` points at the canonical row
    -- and two columns called `*_message_id` referencing different tables is a join waiting to be
    -- written the wrong way round. A reply is a turn in one thread; the question is a canonical
    -- message that may appear in several.
    response_association_id UUID,
    -- The delivery that carried this outreach's question. `SET NULL` so closing a company's
    -- deliveries does not erase the record that this target was asked.
    delivery_id UUID REFERENCES message_deliveries(id) ON DELETE SET NULL,
    -- The canonical message this outreach *asked* with.
    --
    -- Recorded so that "is this outbound message the agent's answer, or the agent asking somebody
    -- else a question?" is answered by a canonical relation. The reply guard used to answer it by
    -- joining a delivery's provider key back to an RFC `Message-ID` on the message, which made a
    -- purely internal decision depend on an SMTP header -- and gave the wrong answer for any
    -- transport that has none.
    request_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (outreach_id, email),
    CONSTRAINT task_outreach_targets_response_association_fk
        FOREIGN KEY (response_association_id) REFERENCES thread_messages(id) ON DELETE SET NULL,
    CONSTRAINT task_outreach_targets_response_check CHECK (
        response_association_id IS NULL OR responded_at IS NOT NULL
    )
);

CREATE INDEX task_outreach_targets_email_waiting_idx
    ON task_outreach_targets (email, outreach_id) WHERE responded_at IS NULL;
CREATE INDEX task_outreach_targets_response_association_idx
    ON task_outreach_targets (response_association_id)
    WHERE response_association_id IS NOT NULL;
-- The reply guard's `NOT EXISTS` runs per candidate outbound message, so it reads this rather than
-- the table.
CREATE INDEX task_outreach_targets_request_message_idx
    ON task_outreach_targets (request_message_id)
    WHERE request_message_id IS NOT NULL;
CREATE UNIQUE INDEX task_outreach_targets_delivery_idx
    ON task_outreach_targets (delivery_id) WHERE delivery_id IS NOT NULL;

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
    -- `pending -> sending -> delivered` is three material changes and still emits three
    -- notifications,
    -- which the stream coalesces on its own. The checks are per table because these are the only
    -- notifying tables that have a `status` column at all.
    IF TG_OP = 'UPDATE' THEN
        IF TG_TABLE_NAME = 'message_deliveries' THEN
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
    ELSIF TG_TABLE_NAME = 'message_deliveries' THEN
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

CREATE TRIGGER message_deliveries_notify_chain
AFTER INSERT OR UPDATE OF status ON message_deliveries
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
    -- The team member a run acts as: its prompt is attributed to their address and user-scoped
    -- memory is recalled and written as theirs. NULL is a run that belongs to nobody, which is
    -- what every schedule was before the attribution existed. Team membership itself is not
    -- constrained here, because it can be revoked after the fact -- the run re-checks it and
    -- refuses rather than acting as somebody who has left; a deleted account leaves the schedule
    -- running unattributed rather than erroring forever.
    run_as_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
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
