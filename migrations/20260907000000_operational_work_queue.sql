-- Business urgency is deliberately independent of worker scheduling. `run_at` remains the only
-- queue scheduling timestamp; these columns describe when a person expects the work completed.
ALTER TABLE background_tasks
    ADD COLUMN business_priority TEXT NOT NULL DEFAULT 'normal',
    ADD COLUMN business_due_at TIMESTAMPTZ,
    ADD COLUMN attention_version BIGINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT background_tasks_business_priority_check CHECK (
        business_priority IN ('normal', 'high', 'urgent')
    ),
    ADD CONSTRAINT background_tasks_attention_version_check CHECK (attention_version > 0);

-- A standalone handoff is human work that is not a task execution. It has its own narrow
-- lifecycle rather than borrowing background-task states or introducing a generic workflow.
CREATE TABLE manual_handoffs (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID,
    correlation_id UUID,
    title TEXT NOT NULL,
    next_action TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    responsible_principal_id UUID,
    business_priority TEXT NOT NULL DEFAULT 'normal',
    business_due_at TIMESTAMPTZ,
    version BIGINT NOT NULL DEFAULT 1,
    created_by_principal_id UUID NOT NULL,
    resolved_by_principal_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at TIMESTAMPTZ,
    CONSTRAINT manual_handoffs_company_id_id_key UNIQUE (company_id, id),
    CONSTRAINT manual_handoffs_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT manual_handoffs_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT manual_handoffs_responsible_fk
        FOREIGN KEY (company_id, responsible_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT manual_handoffs_creator_fk
        FOREIGN KEY (company_id, created_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT manual_handoffs_resolver_fk
        FOREIGN KEY (company_id, resolved_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT manual_handoffs_title_check CHECK (
        btrim(title) <> '' AND octet_length(title) <= 512
    ),
    CONSTRAINT manual_handoffs_next_action_check CHECK (
        btrim(next_action) <> '' AND octet_length(next_action) <= 2048
    ),
    CONSTRAINT manual_handoffs_status_check CHECK (status IN ('open', 'resolved', 'withdrawn')),
    CONSTRAINT manual_handoffs_priority_check CHECK (
        business_priority IN ('normal', 'high', 'urgent')
    ),
    CONSTRAINT manual_handoffs_version_check CHECK (version > 0),
    CONSTRAINT manual_handoffs_resolution_check CHECK (
        (status = 'open' AND resolved_at IS NULL AND resolved_by_principal_id IS NULL)
        OR (status <> 'open' AND resolved_at IS NOT NULL AND resolved_by_principal_id IS NOT NULL)
    )
);

-- Immutable audit for business-only mutations. Task ownership retains its richer existing ledger;
-- handoff reassignment is represented here because it changes the handoff itself.
CREATE TABLE attention_source_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    source_kind TEXT NOT NULL,
    source_id UUID NOT NULL,
    command_id UUID NOT NULL,
    command_fingerprint TEXT NOT NULL,
    operation TEXT NOT NULL,
    actor_principal_id UUID NOT NULL,
    from_version BIGINT NOT NULL,
    to_version BIGINT NOT NULL,
    previous_priority TEXT,
    new_priority TEXT,
    previous_due_at TIMESTAMPTZ,
    new_due_at TIMESTAMPTZ,
    previous_responsible_principal_id UUID,
    new_responsible_principal_id UUID,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT attention_source_events_source_command_key
        UNIQUE (source_kind, source_id, command_id),
    CONSTRAINT attention_source_events_actor_fk
        FOREIGN KEY (company_id, actor_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT attention_source_events_previous_responsible_fk
        FOREIGN KEY (company_id, previous_responsible_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT attention_source_events_new_responsible_fk
        FOREIGN KEY (company_id, new_responsible_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT attention_source_events_source_kind_check CHECK (
        source_kind IN ('task', 'handoff')
    ),
    CONSTRAINT attention_source_events_operation_check CHECK (
        operation IN ('created', 'attributes_changed', 'reassigned', 'resolved', 'withdrawn')
    ),
    CONSTRAINT attention_source_events_version_check CHECK (
        from_version >= 0 AND to_version = from_version + 1
    ),
    CONSTRAINT attention_source_events_priority_check CHECK (
        (previous_priority IS NULL OR previous_priority IN ('normal', 'high', 'urgent'))
        AND (new_priority IS NULL OR new_priority IN ('normal', 'high', 'urgent'))
    )
);

-- Identifier-only wakeups. SSE is only a hint; every client re-runs the scoped projection.
CREATE FUNCTION notify_attention_changed() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    source_id UUID;
    company UUID;
BEGIN
    company := NULLIF(row_data->>'company_id', '')::UUID;
    source_id := COALESCE(
        NULLIF(row_data->>'id', '')::UUID,
        NULLIF(row_data->>'draft_id', '')::UUID
    );
    IF company IS NOT NULL AND source_id IS NOT NULL THEN
        PERFORM pg_notify(
            'attention_changed',
            json_build_object(
                'company_id', company,
                'source_kind', TG_ARGV[0],
                'source_id', source_id
            )::TEXT
        );
    END IF;
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER background_tasks_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status, owner_principal_id, owner_principal_kind,
    business_priority, business_due_at, attention_version
ON background_tasks FOR EACH ROW EXECUTE FUNCTION notify_attention_changed('task');

CREATE TRIGGER manual_handoffs_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status, responsible_principal_id, business_priority,
    business_due_at, version
ON manual_handoffs FOR EACH ROW EXECUTE FUNCTION notify_attention_changed('handoff');

CREATE TRIGGER response_reviews_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status, reviewer_principal_id, expires_at
ON response_reviews FOR EACH ROW EXECUTE FUNCTION notify_attention_changed('response_review');

CREATE TRIGGER task_outreaches_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status, expires_at, version
ON task_outreaches FOR EACH ROW EXECUTE FUNCTION notify_attention_changed('delegation');

CREATE TRIGGER message_deliveries_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status
ON message_deliveries FOR EACH ROW EXECUTE FUNCTION notify_attention_changed('delivery');
