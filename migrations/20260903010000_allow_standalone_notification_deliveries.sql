-- Rejection bounces are provider deliveries, but deliberately have no canonical message: the
-- inbound bytes were refused before persistence. An address under an unknown company has no
-- tenant, channel, or binding either. Let that one explicit delivery arm use the same durable
-- lease/part state machine without inventing tenant identifiers or leaving it on a detached SMTP
-- send. Canonical deliveries retain their all-or-none attribution and existing composite FKs.
ALTER TABLE message_deliveries
    ALTER COLUMN company_id DROP NOT NULL,
    ALTER COLUMN channel_id DROP NOT NULL,
    ALTER COLUMN message_id DROP NOT NULL,
    ALTER COLUMN source_binding_id DROP NOT NULL,
    ALTER COLUMN destination_binding_id DROP NOT NULL;

ALTER TABLE message_deliveries
    ADD CONSTRAINT message_deliveries_attribution_check CHECK (
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
    );

-- The canonical uniqueness constraint remains unchanged. NULL binding IDs do not conflict under
-- it, so standalone rows need their own stable-key lock.
CREATE UNIQUE INDEX message_deliveries_standalone_key_key
    ON message_deliveries (transport, idempotency_key)
    WHERE destination_binding_id IS NULL;

ALTER TABLE message_delivery_parts
    ALTER COLUMN company_id DROP NOT NULL;

-- A NULL in the existing tenant-composite FK intentionally bypasses that FK for standalone rows;
-- this direct parent FK still makes an orphaned part impossible.
ALTER TABLE message_delivery_parts
    ADD CONSTRAINT message_delivery_parts_delivery_id_fk
    FOREIGN KEY (delivery_id) REFERENCES message_deliveries(id) ON DELETE CASCADE;
