-- Make the content boundary and each thread association's purpose explicit. The fail-closed
-- legacy value is intentional: only rows whose existing provenance proves external publication
-- are promoted during the backfill.
ALTER TABLE messages ADD COLUMN audience TEXT NOT NULL DEFAULT 'legacy_unclassified';

UPDATE messages AS message
   SET audience = 'external_conversation'
 WHERE (
           message.direction = 'inbound'
           AND EXISTS (
               SELECT 1
                 FROM principals AS author
                WHERE (author.company_id, author.id) =
                      (message.company_id, message.author_principal_id)
                  AND author.kind = 'external'
           )
       )
    OR (
           message.direction = 'outbound'
           AND EXISTS (
               SELECT 1
                 FROM message_deliveries AS delivery
                WHERE (delivery.company_id, delivery.message_id) =
                      (message.company_id, message.id)
           )
       );

UPDATE messages AS message
   SET audience = 'internal_only'
 WHERE audience = 'legacy_unclassified'
   AND EXISTS (
       SELECT 1
         FROM principals AS author
        WHERE (author.company_id, author.id) =
              (message.company_id, message.author_principal_id)
          AND author.kind IN ('person', 'agent', 'system')
   );

ALTER TABLE messages
    ADD CONSTRAINT messages_audience_check CHECK (
        audience IN ('external_conversation', 'internal_only', 'legacy_unclassified')
    ),
    ADD CONSTRAINT messages_company_id_id_audience_key UNIQUE (company_id, id, audience);

ALTER TABLE thread_messages ADD COLUMN entry_kind TEXT;

UPDATE thread_messages AS association
   SET entry_kind = CASE
       WHEN message.role = 'system' THEN 'system_event'
       WHEN message.audience = 'internal_only' AND message.role = 'agent' THEN 'delegation'
       WHEN message.audience = 'internal_only' THEN 'note'
       ELSE 'conversation'
   END
  FROM messages AS message
 WHERE (message.company_id, message.id) = (association.company_id, association.message_id);

ALTER TABLE thread_messages
    ALTER COLUMN entry_kind SET NOT NULL,
    ADD CONSTRAINT thread_messages_entry_kind_check CHECK (
        entry_kind IN ('conversation', 'note', 'delegation', 'system_event')
    );

-- Carry the audience discriminator into the delivery row and prove it against the canonical
-- message. This is the final fail-closed guard: no application path can queue private or
-- unclassified content by forgetting a policy check.
ALTER TABLE message_deliveries ADD COLUMN message_audience TEXT;

UPDATE message_deliveries
   SET message_audience = 'external_conversation'
 WHERE message_id IS NOT NULL;

ALTER TABLE message_deliveries
    DROP CONSTRAINT message_deliveries_message_fk,
    ADD CONSTRAINT message_deliveries_message_audience_check CHECK (
        (message_id IS NULL AND message_audience IS NULL)
        OR
        (message_id IS NOT NULL AND message_audience = 'external_conversation')
    ),
    ADD CONSTRAINT message_deliveries_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience) ON DELETE CASCADE;

-- Draft versions retain their content forever. Status is workflow metadata; revising or
-- publishing updates it but never rewrites a version's author, body, recipients, or scope.
CREATE TABLE response_drafts (
    id UUID NOT NULL,
    version INTEGER NOT NULL,
    company_id UUID NOT NULL,
    channel_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    task_id UUID,
    author_principal_id UUID NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    recipient_snapshot JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id, version),
    CONSTRAINT response_drafts_company_id_id_version_key
        UNIQUE (company_id, id, version),
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
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT response_drafts_version_check CHECK (version > 0),
    CONSTRAINT response_drafts_status_check CHECK (status IN ('active', 'superseded', 'published')),
    CONSTRAINT response_drafts_subject_check CHECK (octet_length(subject) <= 2048),
    CONSTRAINT response_drafts_body_check CHECK (
        btrim(body) <> '' AND octet_length(body) <= 262144
    ),
    CONSTRAINT response_drafts_recipient_snapshot_check CHECK (
        jsonb_typeof(recipient_snapshot) = 'object'
        AND recipient_snapshot->'version' = '"1"'::jsonb
        AND jsonb_typeof(recipient_snapshot->'to') = 'array'
        AND jsonb_typeof(recipient_snapshot->'cc') = 'array'
        AND octet_length(recipient_snapshot::text) <= 32768
    )
);

CREATE UNIQUE INDEX response_drafts_one_active_version_idx
    ON response_drafts (id) WHERE status = 'active';
CREATE INDEX response_drafts_scope_idx
    ON response_drafts (company_id, channel_id, thread_id, id, version DESC);

CREATE TABLE response_draft_publications (
    company_id UUID NOT NULL,
    draft_id UUID NOT NULL,
    draft_version INTEGER NOT NULL,
    message_id UUID NOT NULL,
    -- Carried into the message FK so the relation itself proves this publication is external.
    message_audience TEXT NOT NULL DEFAULT 'external_conversation',
    delivery_id UUID NOT NULL,
    published_by_principal_id UUID NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (draft_id),
    CONSTRAINT response_draft_publications_draft_fk
        FOREIGN KEY (company_id, draft_id, draft_version)
        REFERENCES response_drafts(company_id, id, version) ON DELETE RESTRICT,
    CONSTRAINT response_draft_publications_message_fk
        FOREIGN KEY (company_id, message_id, message_audience)
        REFERENCES messages(company_id, id, audience) ON DELETE RESTRICT,
    CONSTRAINT response_draft_publications_delivery_fk
        FOREIGN KEY (company_id, delivery_id)
        REFERENCES message_deliveries(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT response_draft_publications_publisher_fk
        FOREIGN KEY (company_id, published_by_principal_id)
        REFERENCES principals(company_id, id) ON DELETE RESTRICT,
    CONSTRAINT response_draft_publications_audience_check
        CHECK (message_audience = 'external_conversation')
);
