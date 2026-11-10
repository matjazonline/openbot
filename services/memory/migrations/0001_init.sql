-- Memory service schema (plan/.archive/memory-service implementation-decisions D2).
-- No backward compatibility: this single init migration is edited in place; a checksum mismatch
-- means resetting the service database.
--
-- pgvector is not a trusted extension, so provisioning (scripts/provision-db.sh) creates it as a
-- superuser. The owning role only checks that it is present and new enough for iterative scans.
DO $$
DECLARE
    installed text;
BEGIN
    SELECT extversion INTO installed FROM pg_extension WHERE extname = 'vector';
    IF installed IS NULL THEN
        RAISE EXCEPTION 'pgvector is not installed in this database; run scripts/provision-db.sh';
    END IF;
    IF string_to_array(installed, '.')::int[] < ARRAY[0, 8, 0] THEN
        RAISE EXCEPTION 'pgvector % is older than 0.8.0 (iterative index scans required)', installed;
    END IF;
END $$;

CREATE DOMAIN access_level AS smallint CHECK (VALUE BETWEEN 0 AND 9);

CREATE TABLE service_settings (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    embedding_model_id text,
    vector_dims integer NOT NULL DEFAULT 384 CHECK (vector_dims = 384),
    created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO service_settings DEFAULT VALUES;

CREATE TABLE admin_credentials (
    id uuid PRIMARY KEY,
    scope text NOT NULL CHECK (scope IN ('operator', 'provisioner')),
    label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 128),
    verifier bytea NOT NULL CHECK (octet_length(verifier) = 32),
    expires_at timestamptz,
    revoked_at timestamptz,
    created_by uuid REFERENCES admin_credentials (id),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Tenant rows are never deleted: a deleted row is the tenant's permanent barrier. Its content is
-- removed by the maintenance sweep in bounded batches; `content_removed_at` marks the end (06A).
CREATE TABLE tenants (
    id uuid PRIMARY KEY,
    external_id text NOT NULL UNIQUE CHECK (external_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    state text NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'disabled', 'deleted')),
    generation_daily_token_cap bigint NOT NULL DEFAULT 0 CHECK (generation_daily_token_cap >= 0),
    managed_by uuid REFERENCES admin_credentials (id),
    disabled_by uuid REFERENCES admin_credentials (id),
    disabled_by_scope text CHECK (disabled_by_scope IN ('operator', 'provisioner')),
    created_at timestamptz NOT NULL DEFAULT now(),
    state_changed_at timestamptz NOT NULL DEFAULT now(),
    content_removed_at timestamptz,
    CHECK (content_removed_at IS NULL OR state = 'deleted'),
    CHECK ((state = 'disabled') = (disabled_by_scope IS NOT NULL)),
    CHECK ((disabled_by IS NULL) OR (disabled_by_scope IS NOT NULL))
);
-- Reassignment moves a manager's tenants in id order (06A A1-5).
CREATE INDEX tenants_managed_by ON tenants (managed_by, id);

-- The fairness cursor lives off the tenant row so claims never conflict with writers' FOR SHARE.
CREATE TABLE tenant_generation_schedule (
    tenant_id uuid PRIMARY KEY REFERENCES tenants (id),
    last_claimed_at timestamptz
);

-- Namespace rows are never deleted: the tombstone is the barrier against recreation. A deleted
-- namespace's content is removed once none of its operations is unfinished (06A).
CREATE TABLE namespaces (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    id uuid NOT NULL,
    external_id text NOT NULL CHECK (external_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    state text NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'deleted')),
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    content_removed_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, external_id),
    CHECK ((state = 'deleted') = (deleted_at IS NOT NULL)),
    CHECK (content_removed_at IS NULL OR state = 'deleted')
);
CREATE INDEX namespaces_pending_removal ON namespaces (deleted_at, tenant_id, id)
    WHERE state = 'deleted' AND content_removed_at IS NULL;

CREATE TABLE credentials (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    id uuid NOT NULL UNIQUE,
    label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 128),
    verifier bytea NOT NULL CHECK (octet_length(verifier) = 32),
    expires_at timestamptz,
    revoked_at timestamptz,
    issued_by uuid NOT NULL REFERENCES admin_credentials (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, id)
);
-- Credential listing by label (06A, paged by id).
CREATE INDEX credentials_by_label ON credentials (tenant_id, label, id);

CREATE TABLE credential_grants (
    tenant_id uuid NOT NULL,
    credential_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    read_max access_level,
    write_max access_level,
    operations text[] NOT NULL CHECK (
        cardinality(operations) > 0
        AND operations <@ ARRAY['save', 'search', 'timeline', 'fetch', 'correct', 'delete',
                                'context', 'sources', 'sessions', 'generate',
                                'generation_status', 'purge']::text[]
    ),
    PRIMARY KEY (tenant_id, credential_id, namespace_id),
    FOREIGN KEY (tenant_id, credential_id) REFERENCES credentials (tenant_id, id),
    FOREIGN KEY (tenant_id, namespace_id) REFERENCES namespaces (tenant_id, id)
);

CREATE TABLE sessions (
    tenant_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    id uuid NOT NULL,
    external_id text NOT NULL CHECK (external_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    state text NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'closed')),
    opened_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, namespace_id, id),
    UNIQUE (tenant_id, namespace_id, external_id),
    FOREIGN KEY (tenant_id, namespace_id) REFERENCES namespaces (tenant_id, id)
);

-- Immutable evidence revisions. Content is only ever nulled by retention.
CREATE TABLE sources (
    tenant_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    id uuid NOT NULL,
    external_ref text NOT NULL CHECK (octet_length(external_ref) BETWEEN 1 AND 512),
    revision text NOT NULL CHECK (octet_length(revision) BETWEEN 1 AND 128),
    access_level access_level NOT NULL,
    session_id uuid,
    content text CHECK (octet_length(content) <= 262144),
    content_sha256 bytea NOT NULL CHECK (octet_length(content_sha256) = 32),
    redaction_version integer NOT NULL DEFAULT 1,
    occurred_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz,
    content_nulled_at timestamptz,
    -- The client credential that recorded the revision. A revision above a caller's read
    -- grant is reused only by the credential that recorded it (D3); NULL for none.
    recorded_by uuid,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, namespace_id, id),
    FOREIGN KEY (tenant_id, recorded_by) REFERENCES credentials (tenant_id, id),
    -- Target of observation_links' label FK: a link records its source's real level.
    UNIQUE (tenant_id, id, access_level),
    UNIQUE (tenant_id, namespace_id, external_ref, revision),
    FOREIGN KEY (tenant_id, namespace_id) REFERENCES namespaces (tenant_id, id),
    FOREIGN KEY (tenant_id, namespace_id, session_id) REFERENCES sessions (tenant_id, namespace_id, id),
    CHECK ((content IS NULL) = (content_nulled_at IS NOT NULL))
);
-- Retention candidates (D9): content is nulled 30 days after it was recorded or after the last
-- operation using it finished, or at its own expiry.
CREATE INDEX sources_retention ON sources (recorded_at) WHERE content IS NOT NULL;
CREATE INDEX sources_expiry ON sources (expires_at) WHERE content IS NOT NULL AND expires_at IS NOT NULL;
CREATE INDEX sources_removal ON sources (content_nulled_at) WHERE content IS NULL;

CREATE FUNCTION sources_are_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- Everything but the retention null-out (content, content_nulled_at) is frozen, including
    -- the session a revision belongs to, its redaction version and its expiry.
    IF (NEW.tenant_id, NEW.namespace_id, NEW.id, NEW.external_ref, NEW.revision, NEW.access_level,
        NEW.session_id, NEW.content_sha256, NEW.redaction_version, NEW.occurred_at,
        NEW.recorded_at, NEW.expires_at, NEW.recorded_by)
       IS DISTINCT FROM
       (OLD.tenant_id, OLD.namespace_id, OLD.id, OLD.external_ref, OLD.revision, OLD.access_level,
        OLD.session_id, OLD.content_sha256, OLD.redaction_version, OLD.occurred_at,
        OLD.recorded_at, OLD.expires_at, OLD.recorded_by)
       OR (NEW.content IS NOT NULL AND NEW.content IS DISTINCT FROM OLD.content) THEN
        RAISE EXCEPTION 'source revisions are immutable; only retention may null their content';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER sources_immutable BEFORE UPDATE ON sources
    FOR EACH ROW EXECUTE FUNCTION sources_are_immutable();

-- Generation operations (D9). The immutable request holds the window's event list; the
-- window's text lives once, in `sources` (linked below), and retention nulls both.
CREATE TABLE generation_operations (
    tenant_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    id uuid NOT NULL,
    credential_id uuid NOT NULL,
    idempotency_key text NOT NULL,
    -- The accepting request's `x-correlation-id` (06A A2-2), carried by every worker log line.
    correlation_id text CHECK (correlation_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    session_id uuid,
    summary boolean NOT NULL DEFAULT false,
    state text NOT NULL DEFAULT 'accepted' CHECK (state IN (
        'accepted', 'claimed', 'dispatched', 'completed', 'failed', 'cancelled', 'expired',
        'unknown')),
    -- A bounded code (saved, nothing_to_save, a failure or cancellation reason); never
    -- provider text.
    outcome_code text CHECK (outcome_code ~ '^[a-z_]{1,64}$'),
    request jsonb CHECK (octet_length(request::text) <= 1048576),
    -- Set when retention nulled the request (and released the window's content to retention).
    inputs_nulled_at timestamptz,
    producing_ceiling access_level NOT NULL,
    watermark_kind text NOT NULL CHECK (watermark_kind IN ('exact', 'ceiling_fallback')),
    watermark_level access_level NOT NULL,
    source_max_level access_level NOT NULL DEFAULT 0,
    input_level access_level NOT NULL,
    allowance_tokens bigint NOT NULL CHECK (allowance_tokens > 0),
    max_output_tokens integer NOT NULL CHECK (max_output_tokens BETWEEN 1 AND 8192),
    -- The spend each dispatch reserves: 2 × prompt bytes + 4,096 + max_output_tokens (D9).
    reservation bigint NOT NULL DEFAULT 0 CHECK (reservation >= 0),
    -- What is reserved right now (claimed or dispatched), on `spend_day`.
    reserved_tokens bigint NOT NULL DEFAULT 0 CHECK (reserved_tokens >= 0),
    used_tokens bigint NOT NULL DEFAULT 0 CHECK (used_tokens >= 0),
    spend_day date,
    fence bigint NOT NULL DEFAULT 0,
    lease_owner uuid,
    lease_until timestamptz,
    cancel_requested_at timestamptz,
    -- Dispatches so far: a definite rejection goes back to `accepted` until the third.
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    dispatched_at timestamptz,
    finished_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, namespace_id, id),
    -- One key gives at most one operation while it exists, even after its idempotency record
    -- was swept (06A A1-4): a reuse with a new deadline is `idempotency_conflict`.
    CONSTRAINT generation_operations_one_per_key UNIQUE (tenant_id, idempotency_key),
    FOREIGN KEY (tenant_id, namespace_id) REFERENCES namespaces (tenant_id, id),
    FOREIGN KEY (tenant_id, credential_id) REFERENCES credentials (tenant_id, id),
    FOREIGN KEY (tenant_id, namespace_id, session_id) REFERENCES sessions (tenant_id, namespace_id, id),
    CHECK (watermark_level <= producing_ceiling),
    CHECK (watermark_kind = 'exact' OR watermark_level = producing_ceiling),
    CHECK (source_max_level <= producing_ceiling),
    CHECK (input_level = GREATEST(watermark_level, source_max_level)),
    CHECK (NOT summary OR session_id IS NOT NULL),
    CHECK (reservation <= allowance_tokens),
    CHECK ((request IS NULL) = (inputs_nulled_at IS NOT NULL)),
    CHECK ((state IN ('claimed', 'dispatched')) = (lease_until IS NOT NULL)),
    CHECK ((state IN ('completed', 'failed', 'cancelled', 'expired', 'unknown'))
           = (finished_at IS NOT NULL)),
    CHECK (request IS NOT NULL OR finished_at IS NOT NULL)
);
-- At most one claimed or dispatched operation per tenant, enforced by the database.
CREATE UNIQUE INDEX generation_one_in_flight_per_tenant
    ON generation_operations (tenant_id) WHERE state IN ('claimed', 'dispatched');
CREATE INDEX generation_due ON generation_operations (next_attempt_at) WHERE state = 'accepted';
CREATE INDEX generation_pending_expiry ON generation_operations (expires_at)
    WHERE state = 'accepted';
CREATE INDEX generation_leases ON generation_operations (lease_until)
    WHERE state IN ('claimed', 'dispatched');
CREATE INDEX generation_by_credential ON generation_operations (tenant_id, credential_id)
    WHERE state IN ('accepted', 'claimed', 'dispatched');
CREATE INDEX generation_by_namespace ON generation_operations (tenant_id, namespace_id);
CREATE INDEX generation_retention ON generation_operations (finished_at)
    WHERE request IS NOT NULL AND finished_at IS NOT NULL;

-- The identity, label and request of an operation never change; retention may only null the
-- request, and a finished operation is never revived (D9: nothing revives old inference).
CREATE FUNCTION generation_requests_are_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.tenant_id, NEW.namespace_id, NEW.id, NEW.credential_id, NEW.idempotency_key,
        NEW.correlation_id, NEW.session_id, NEW.summary, NEW.producing_ceiling, NEW.watermark_kind,
        NEW.watermark_level, NEW.source_max_level, NEW.input_level, NEW.allowance_tokens,
        NEW.max_output_tokens, NEW.reservation, NEW.expires_at, NEW.created_at)
       IS DISTINCT FROM
       (OLD.tenant_id, OLD.namespace_id, OLD.id, OLD.credential_id, OLD.idempotency_key,
        OLD.correlation_id, OLD.session_id, OLD.summary, OLD.producing_ceiling, OLD.watermark_kind,
        OLD.watermark_level, OLD.source_max_level, OLD.input_level, OLD.allowance_tokens,
        OLD.max_output_tokens, OLD.reservation, OLD.expires_at, OLD.created_at)
       OR (NEW.request IS NOT NULL AND NEW.request IS DISTINCT FROM OLD.request) THEN
        RAISE EXCEPTION 'generation requests and labels are immutable; only retention may null the request';
    END IF;
    IF OLD.finished_at IS NOT NULL AND (NEW.state, NEW.outcome_code, NEW.finished_at)
        IS DISTINCT FROM (OLD.state, OLD.outcome_code, OLD.finished_at) THEN
        RAISE EXCEPTION 'a finished generation operation is never revived';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER generation_requests_immutable BEFORE UPDATE ON generation_operations
    FOR EACH ROW EXECUTE FUNCTION generation_requests_are_immutable();

-- The recorded source revisions of an operation's window, all in its namespace.
CREATE TABLE generation_operation_sources (
    tenant_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    source_id uuid NOT NULL,
    PRIMARY KEY (tenant_id, operation_id, source_id),
    FOREIGN KEY (tenant_id, namespace_id, operation_id)
        REFERENCES generation_operations (tenant_id, namespace_id, id),
    FOREIGN KEY (tenant_id, namespace_id, source_id) REFERENCES sources (tenant_id, namespace_id, id)
);
CREATE INDEX generation_operation_sources_by_source
    ON generation_operation_sources (tenant_id, source_id);

CREATE TABLE observations (
    tenant_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    id uuid NOT NULL,
    session_id uuid,
    kind text NOT NULL CHECK (kind IN ('fact', 'decision', 'discovery', 'fix', 'outcome', 'note',
                                       'summary')),
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 256 AND octet_length(title) <= 1024),
    body text NOT NULL CHECK (char_length(body) <= 16384 AND octet_length(body) <= 65536),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
    access_level access_level NOT NULL,
    producing_ceiling access_level NOT NULL,
    watermark_kind text NOT NULL CHECK (watermark_kind IN ('exact', 'ceiling_fallback')),
    watermark_level access_level NOT NULL,
    source_max_level access_level NOT NULL,
    -- A correction never lowers the label: when the prior level exceeds what the correction's
    -- own evidence derives, it is carried here, so the evidence columns still explain the label.
    carried_level access_level,
    origin text NOT NULL CHECK (origin IN ('explicit', 'generated')),
    generation_operation_id uuid,
    output_ordinal integer,
    occurred_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz,
    search_tsv tsvector GENERATED ALWAYS AS (to_tsvector('simple', title || ' ' || body)) STORED,
    -- Semantic indexing (D7): pending → indexed | failed. Keyword recall never waits for it.
    -- Attempts count claims; after 8 a failed row stays terminal until `admin reindex-failed`.
    index_state text NOT NULL DEFAULT 'pending' CHECK (index_state IN ('pending', 'indexed', 'failed')),
    index_attempts integer NOT NULL DEFAULT 0 CHECK (index_attempts BETWEEN 0 AND 8),
    next_index_at timestamptz NOT NULL DEFAULT now(),
    index_lease_until timestamptz,
    index_fence bigint NOT NULL DEFAULT 0,
    -- A bounded failure code for operators; never content.
    index_error text CHECK (index_error IN ('embedding_unavailable', 'invalid_vector',
                                            'part_over_limit', 'too_many_parts',
                                            'lease_expired')),
    CHECK ((index_state = 'failed') = (index_error IS NOT NULL)),
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, id, namespace_id, access_level, revision),
    UNIQUE (tenant_id, generation_operation_id, output_ordinal),
    FOREIGN KEY (tenant_id, namespace_id) REFERENCES namespaces (tenant_id, id),
    FOREIGN KEY (tenant_id, namespace_id, session_id) REFERENCES sessions (tenant_id, namespace_id, id),
    -- A generated observation lives in its operation's namespace.
    FOREIGN KEY (tenant_id, namespace_id, generation_operation_id)
        REFERENCES generation_operations (tenant_id, namespace_id, id),
    CHECK (access_level >= watermark_level),
    CHECK (access_level >= source_max_level),
    CHECK (access_level = GREATEST(watermark_level, source_max_level, COALESCE(carried_level, 0))),
    CHECK (carried_level IS NULL OR carried_level > GREATEST(watermark_level, source_max_level)),
    CHECK (watermark_level <= producing_ceiling),
    CHECK (watermark_kind = 'exact' OR watermark_level = producing_ceiling),
    CHECK ((origin = 'generated') = (generation_operation_id IS NOT NULL)),
    CHECK ((generation_operation_id IS NULL) = (output_ordinal IS NULL))
);
CREATE INDEX observations_search ON observations USING gin (search_tsv);
CREATE INDEX observations_scope ON observations (tenant_id, namespace_id, access_level);
CREATE INDEX observations_timeline ON observations (tenant_id, namespace_id, occurred_at, id);
CREATE INDEX observations_index_due ON observations (next_index_at)
    WHERE index_state IN ('pending', 'failed');
CREATE INDEX observations_expiry ON observations (expires_at) WHERE expires_at IS NOT NULL;
-- The metrics scrape counts failed indexing without scanning the table (06A A2-1).
CREATE INDEX observations_index_failed ON observations (tenant_id) WHERE index_state = 'failed';

CREATE TABLE observation_links (
    tenant_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    ordinal integer NOT NULL,
    source_id uuid,
    parent_observation_id uuid,
    -- The parent's revision the derivation read; a copy names an exact revision (D4).
    parent_revision bigint,
    source_level access_level NOT NULL,
    PRIMARY KEY (tenant_id, observation_id, ordinal),
    FOREIGN KEY (tenant_id, observation_id) REFERENCES observations (tenant_id, id) ON DELETE CASCADE,
    -- A source link carries the source's real (immutable) level, so it cannot under-label.
    -- A parent-observation link records the parent's level at derivation time (MATCH SIMPLE:
    -- this FK does not apply when source_id is NULL).
    FOREIGN KEY (tenant_id, source_id, source_level) REFERENCES sources (tenant_id, id, access_level),
    FOREIGN KEY (tenant_id, parent_observation_id) REFERENCES observations (tenant_id, id)
        ON DELETE CASCADE,
    CHECK ((source_id IS NULL) <> (parent_observation_id IS NULL)),
    CHECK ((parent_observation_id IS NULL) = (parent_revision IS NULL))
);
CREATE INDEX observation_links_parent ON observation_links (tenant_id, parent_observation_id)
    WHERE parent_observation_id IS NOT NULL;
-- Source removal (06A) checks for links by source; this also serves the FK check on delete.
CREATE INDEX observation_links_source ON observation_links (tenant_id, source_id)
    WHERE source_id IS NOT NULL;

CREATE TABLE observation_parts (
    tenant_id uuid NOT NULL,
    observation_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    access_level access_level NOT NULL,
    revision bigint NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal BETWEEN 0 AND 127),
    byte_start integer NOT NULL CHECK (byte_start >= 0),
    byte_end integer NOT NULL,
    -- The identity of the model that produced the vector; search compares only vectors of the
    -- configured model, and startup refuses another model (reset policy).
    model_id text NOT NULL CHECK (char_length(model_id) BETWEEN 1 AND 256),
    embedding vector(384) NOT NULL,
    PRIMARY KEY (tenant_id, observation_id, ordinal),
    -- A part cannot carry a different label, namespace or a stale revision.
    FOREIGN KEY (tenant_id, observation_id, namespace_id, access_level, revision)
        REFERENCES observations (tenant_id, id, namespace_id, access_level, revision)
        ON DELETE CASCADE,
    -- Parts are non-empty byte ranges of the body; an empty body has one title-only part 0..0.
    CHECK (byte_end > byte_start OR (byte_start = 0 AND byte_end = 0 AND ordinal = 0))
);
CREATE INDEX observation_parts_scope ON observation_parts (tenant_id, namespace_id, access_level);
CREATE INDEX observation_parts_embedding ON observation_parts
    USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);

CREATE TABLE tenant_spend_days (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    day date NOT NULL,
    reserved bigint NOT NULL DEFAULT 0 CHECK (reserved >= 0),
    used bigint NOT NULL DEFAULT 0 CHECK (used >= 0),
    PRIMARY KEY (tenant_id, day)
);
CREATE INDEX tenant_spend_retention ON tenant_spend_days (day);

CREATE TABLE idempotency_records (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    kind text NOT NULL,
    key text NOT NULL CHECK (key ~ '^[A-Za-z0-9._:-]{1,128}$'),
    request_sha256 bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    -- The credential that executed it; a replay by another credential re-checks its authority.
    credential_id uuid NOT NULL,
    result jsonb NOT NULL,
    request_expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, kind, key),
    FOREIGN KEY (tenant_id, credential_id) REFERENCES credentials (tenant_id, id)
);
CREATE INDEX idempotency_retention ON idempotency_records (created_at);

CREATE TABLE audit_events (
    id uuid PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    tenant_id uuid,
    actor_kind text NOT NULL CHECK (actor_kind IN ('operator', 'provisioner', 'client', 'system')),
    actor_id uuid,
    action text NOT NULL,
    target_kind text NOT NULL,
    target_id text NOT NULL,
    outcome text NOT NULL,
    -- The HTTP request id; NULL for CLI actions such as `admin bootstrap`.
    request_id uuid,
    -- The client's `x-correlation-id`, when it sent one (06A A2-2): opaque, never authority.
    correlation_id text CHECK (correlation_id ~ '^[A-Za-z0-9._:-]{1,128}$'),
    details jsonb NOT NULL DEFAULT '{}'::jsonb CHECK (octet_length(details::text) <= 4096)
);
CREATE INDEX audit_retention ON audit_events (at);

CREATE FUNCTION audit_is_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit events are append-only';
END $$;
CREATE TRIGGER audit_append_only BEFORE UPDATE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_is_append_only();

-- Retention is enforced here too (06A): only an event older than 400 days may be deleted.
CREATE FUNCTION audit_is_retained() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.at > now() - interval '400 days' THEN
        RAISE EXCEPTION 'audit events are kept for 400 days';
    END IF;
    RETURN OLD;
END $$;
CREATE TRIGGER audit_retained BEFORE DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_is_retained();
