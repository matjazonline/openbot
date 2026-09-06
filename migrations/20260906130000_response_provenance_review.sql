-- Response review is a clean replacement for the minimal draft scaffold. This repository resets
-- its database for this release, so retaining partially populated scaffold rows would only weaken
-- the immutable-version and exact-publication constraints below.
DROP TABLE response_draft_publications;
DROP TABLE response_drafts;

ALTER TABLE companies
    ADD COLUMN external_response_review TEXT NOT NULL DEFAULT 'autonomous',
    ADD CONSTRAINT companies_external_response_review_check CHECK (
        external_response_review IN ('autonomous', 'review_all_external')
    );

ALTER TABLE channels
    ADD COLUMN external_response_review_override TEXT,
    ADD COLUMN preferred_reviewer_principal_id UUID,
    ADD CONSTRAINT channels_external_response_review_override_check CHECK (
        external_response_review_override IS NULL
        OR external_response_review_override IN ('autonomous', 'review_all_external')
    ),
    ADD CONSTRAINT channels_preferred_reviewer_fk
        FOREIGN KEY (company_id, preferred_reviewer_principal_id)
        REFERENCES principals(company_id, id) ON DELETE SET NULL (preferred_reviewer_principal_id);

CREATE TABLE response_drafts (
    id UUID NOT NULL,
    version INTEGER NOT NULL,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    task_id UUID,
    source_handoff_generation UUID,
    author_principal_id UUID NOT NULL,
    reviewer_principal_id UUID NOT NULL,
    proposed_message_id UUID NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    attachment_snapshot JSONB NOT NULL,
    recipient_snapshot JSONB NOT NULL,
    transport_snapshot JSONB NOT NULL,
    -- The exact unpersisted MessageWrite and frozen NewDelivery. It contains outbound material,
    -- never evidence content, and is decoded fallibly before publication.
    publication_snapshot JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending_review',
    created_by_principal_id UUID NOT NULL,
    updated_by_principal_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id, version),
    CONSTRAINT response_drafts_company_id_id_version_key UNIQUE (company_id, id, version),
    CONSTRAINT response_drafts_company_message_key UNIQUE (company_id, proposed_message_id),
    CONSTRAINT response_drafts_channel_fk
        FOREIGN KEY (company_id, channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE,
    CONSTRAINT response_drafts_thread_fk
        FOREIGN KEY (company_id, channel_id, thread_id)
        REFERENCES threads(company_id, channel_id, id) ON DELETE CASCADE,
    CONSTRAINT response_drafts_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE SET NULL (task_id),
    CONSTRAINT response_drafts_author_fk
        FOREIGN KEY (company_id, author_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_drafts_reviewer_fk
        FOREIGN KEY (company_id, reviewer_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_drafts_created_by_fk
        FOREIGN KEY (company_id, created_by_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_drafts_updated_by_fk
        FOREIGN KEY (company_id, updated_by_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_drafts_version_check CHECK (version > 0),
    CONSTRAINT response_drafts_status_check CHECK (
        status IN ('pending_review', 'rejected', 'expired', 'superseded', 'published')
    ),
    CONSTRAINT response_drafts_subject_check CHECK (octet_length(subject) <= 2048),
    CONSTRAINT response_drafts_body_check CHECK (
        btrim(body) <> '' AND octet_length(body) <= 262144
    ),
    CONSTRAINT response_drafts_attachment_snapshot_check CHECK (
        jsonb_typeof(attachment_snapshot) = 'object'
        AND attachment_snapshot->'version' = '"1"'::jsonb
        AND jsonb_typeof(attachment_snapshot->'items') = 'array'
        AND octet_length(attachment_snapshot::text) <= 262144
    ),
    CONSTRAINT response_drafts_recipient_snapshot_check CHECK (
        jsonb_typeof(recipient_snapshot) = 'object'
        AND recipient_snapshot->'version' = '"1"'::jsonb
        AND jsonb_typeof(recipient_snapshot->'to') = 'array'
        AND jsonb_typeof(recipient_snapshot->'cc') = 'array'
        AND octet_length(recipient_snapshot::text) <= 32768
    ),
    CONSTRAINT response_drafts_transport_snapshot_check CHECK (
        jsonb_typeof(transport_snapshot) = 'object'
        AND transport_snapshot->'version' = '"1"'::jsonb
        AND octet_length(transport_snapshot::text) <= 8192
    ),
    CONSTRAINT response_drafts_publication_snapshot_check CHECK (
        jsonb_typeof(publication_snapshot) = 'object'
        AND publication_snapshot->'version' = '"1"'::jsonb
        AND octet_length(publication_snapshot::text) <= 16777216
    )
);

CREATE UNIQUE INDEX response_drafts_one_pending_version_idx
    ON response_drafts (id) WHERE status = 'pending_review';
CREATE INDEX response_drafts_scope_idx
    ON response_drafts (company_id, channel_id, thread_id, id, version DESC);
CREATE INDEX response_drafts_reviewer_pending_idx
    ON response_drafts (company_id, reviewer_principal_id, created_at, id)
    WHERE status = 'pending_review';

CREATE TABLE response_reviews (
    company_id UUID NOT NULL,
    draft_id UUID NOT NULL,
    draft_version INTEGER NOT NULL,
    reviewer_principal_id UUID NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    feedback TEXT,
    reviewer_rationale TEXT,
    decided_by_principal_id UUID,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (draft_id, draft_version),
    CONSTRAINT response_reviews_company_id_draft_key
        UNIQUE (company_id, draft_id, draft_version),
    CONSTRAINT response_reviews_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE CASCADE,
    CONSTRAINT response_reviews_reviewer_fk
        FOREIGN KEY (company_id, reviewer_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_reviews_decider_fk
        FOREIGN KEY (company_id, decided_by_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_reviews_status_check CHECK (
        status IN ('pending', 'rejected', 'expired', 'superseded', 'published')
    ),
    CONSTRAINT response_reviews_expiry_check CHECK (expires_at > created_at),
    CONSTRAINT response_reviews_feedback_check CHECK (
        feedback IS NULL OR (btrim(feedback) <> '' AND octet_length(feedback) <= 8192)
    ),
    CONSTRAINT response_reviews_rationale_check CHECK (
        reviewer_rationale IS NULL
        OR (btrim(reviewer_rationale) <> '' AND octet_length(reviewer_rationale) <= 2048)
    ),
    CONSTRAINT response_reviews_decision_shape_check CHECK (
        (status = 'pending' AND decided_by_principal_id IS NULL AND feedback IS NULL
            AND reviewer_rationale IS NULL)
        OR (status = 'rejected' AND decided_by_principal_id IS NOT NULL AND feedback IS NOT NULL)
        OR (status = 'published' AND decided_by_principal_id IS NOT NULL)
        OR status IN ('expired', 'superseded')
    )
);

CREATE INDEX response_reviews_pending_expiry_idx
    ON response_reviews (expires_at, draft_id) WHERE status = 'pending';

CREATE TABLE response_draft_evidence (
    company_id UUID NOT NULL,
    draft_id UUID NOT NULL,
    draft_version INTEGER NOT NULL,
    id UUID NOT NULL,
    position INTEGER NOT NULL,
    source_reference JSONB NOT NULL,
    source_version TEXT NOT NULL,
    content_digest TEXT NOT NULL,
    audience TEXT NOT NULL,
    support TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (draft_id, draft_version, id),
    CONSTRAINT response_draft_evidence_position_key UNIQUE (draft_id, draft_version, position),
    CONSTRAINT response_draft_evidence_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE CASCADE,
    CONSTRAINT response_draft_evidence_position_check CHECK (position >= 0 AND position < 128),
    CONSTRAINT response_draft_evidence_source_check CHECK (
        jsonb_typeof(source_reference) = 'object'
        AND jsonb_typeof(source_reference->'kind') = 'string'
        AND source_reference->>'kind' IN (
            'message', 'note', 'attachment', 'delegated_result', 'retained_tool_result',
            'external_url'
        )
        AND octet_length(source_reference::text) <= 8192
    ),
    CONSTRAINT response_draft_evidence_version_check CHECK (
        btrim(source_version) <> '' AND octet_length(source_version) <= 256
    ),
    CONSTRAINT response_draft_evidence_digest_check CHECK (
        btrim(content_digest) <> '' AND octet_length(content_digest) <= 128
    ),
    CONSTRAINT response_draft_evidence_audience_check CHECK (
        audience IN ('external_conversation', 'internal_only', 'company_restricted')
    ),
    CONSTRAINT response_draft_evidence_support_check CHECK (
        support IN ('direct_evidence', 'inference')
    )
);

CREATE OR REPLACE FUNCTION enforce_response_draft_evidence_immutability() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF EXISTS (
            SELECT 1 FROM response_reviews
            WHERE company_id = NEW.company_id AND draft_id = NEW.draft_id
              AND draft_version = NEW.draft_version
        ) THEN
            RAISE EXCEPTION 'response draft evidence is already sealed' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' AND NOT EXISTS (
        SELECT 1 FROM response_drafts
        WHERE company_id = OLD.company_id AND id = OLD.draft_id AND version = OLD.draft_version
    ) THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'response draft evidence is immutable' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER response_draft_evidence_immutable
BEFORE INSERT OR UPDATE OR DELETE ON response_draft_evidence
FOR EACH ROW EXECUTE FUNCTION enforce_response_draft_evidence_immutability();

CREATE TABLE response_draft_publications (
    company_id UUID NOT NULL,
    draft_id UUID NOT NULL,
    draft_version INTEGER NOT NULL,
    message_id UUID NOT NULL,
    message_audience TEXT NOT NULL DEFAULT 'external_conversation',
    delivery_id UUID NOT NULL,
    published_by_principal_id UUID NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (draft_id),
    CONSTRAINT response_draft_publications_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_draft_publications_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_draft_publications_delivery_fk
        FOREIGN KEY (company_id, delivery_id)
        REFERENCES message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_draft_publications_publisher_fk
        FOREIGN KEY (company_id, published_by_principal_id)
        REFERENCES principals(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_draft_publications_audience_check
        CHECK (message_audience = 'external_conversation')
);

CREATE TABLE response_review_commands (
    company_id UUID NOT NULL,
    command_id UUID NOT NULL,
    draft_id UUID NOT NULL,
    expected_draft_version INTEGER NOT NULL,
    action TEXT NOT NULL,
    command_fingerprint TEXT NOT NULL,
    resulting_draft_version INTEGER NOT NULL,
    published_message_id UUID,
    published_delivery_id UUID,
    published_delivery_created BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (company_id, command_id),
    CONSTRAINT response_review_commands_draft_fk
        FOREIGN KEY (company_id, draft_id, expected_draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE CASCADE,
    CONSTRAINT response_review_commands_delivery_fk
        FOREIGN KEY (company_id, published_delivery_id)
        REFERENCES message_deliveries(company_id, id) DEFERRABLE INITIALLY DEFERRED,
    CONSTRAINT response_review_commands_publication_result_check CHECK (
        (published_delivery_id IS NULL AND published_delivery_created IS NULL)
        OR (published_delivery_id IS NOT NULL AND published_delivery_created IS NOT NULL)
    ),
    CONSTRAINT response_review_commands_action_check CHECK (
        action IN ('approve', 'edit', 'reject', 'reassign')
    ),
    CONSTRAINT response_review_commands_fingerprint_check CHECK (
        btrim(command_fingerprint) <> '' AND octet_length(command_fingerprint) <= 128
    ),
    CONSTRAINT response_review_commands_version_check CHECK (
        expected_draft_version > 0 AND resulting_draft_version > 0
    )
);

CREATE OR REPLACE FUNCTION enforce_response_draft_update() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF (OLD.id, OLD.version, OLD.company_id, OLD.channel_id, OLD.thread_id,
        OLD.author_principal_id, OLD.proposed_message_id,
        OLD.subject, OLD.body, OLD.attachment_snapshot, OLD.recipient_snapshot,
        OLD.transport_snapshot, OLD.publication_snapshot, OLD.created_by_principal_id,
        OLD.created_at, OLD.source_handoff_generation) IS DISTINCT FROM
       (NEW.id, NEW.version, NEW.company_id, NEW.channel_id, NEW.thread_id,
        NEW.author_principal_id, NEW.proposed_message_id,
        NEW.subject, NEW.body, NEW.attachment_snapshot, NEW.recipient_snapshot,
        NEW.transport_snapshot, NEW.publication_snapshot, NEW.created_by_principal_id,
        NEW.created_at, NEW.source_handoff_generation) THEN
        RAISE EXCEPTION 'response draft versions are immutable' USING ERRCODE = '23514';
    END IF;
    IF OLD.task_id IS DISTINCT FROM NEW.task_id
       AND NOT (OLD.task_id IS NOT NULL AND NEW.task_id IS NULL) THEN
        RAISE EXCEPTION 'a response draft task source cannot be replaced' USING ERRCODE = '23514';
    END IF;
    IF OLD.status <> NEW.status AND NOT (
        OLD.status = 'pending_review'
        AND NEW.status IN ('rejected', 'expired', 'superseded', 'published')
    ) THEN
        RAISE EXCEPTION 'invalid response draft status transition' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER response_drafts_immutable_versions
BEFORE UPDATE ON response_drafts
FOR EACH ROW EXECUTE FUNCTION enforce_response_draft_update();
