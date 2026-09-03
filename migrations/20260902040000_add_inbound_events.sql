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
