-- Delegation targets have a stable, transport-neutral identity. The delivery address remains an
-- adapter detail used to correlate email replies; it is no longer the target exposed by the
-- application read model.

ALTER TABLE task_outreaches ADD COLUMN company_id UUID;

UPDATE task_outreaches AS outreach
SET company_id = task.company_id
FROM background_tasks AS task
WHERE task.id = outreach.task_id;

ALTER TABLE task_outreaches
    ALTER COLUMN company_id SET NOT NULL,
    DROP CONSTRAINT task_outreaches_task_id_fkey,
    ADD CONSTRAINT task_outreaches_company_id_id_key UNIQUE (company_id, id),
    ADD CONSTRAINT task_outreaches_task_fk
        FOREIGN KEY (company_id, task_id)
        REFERENCES background_tasks(company_id, id) ON DELETE CASCADE;

ALTER TABLE task_outreach_targets
    ADD COLUMN id UUID NOT NULL DEFAULT gen_random_uuid(),
    ADD COLUMN company_id UUID,
    ADD COLUMN target_kind TEXT NOT NULL DEFAULT 'external',
    ADD COLUMN internal_channel_id UUID,
    ADD COLUMN external_transport TEXT,
    ADD COLUMN external_namespace TEXT,
    ADD COLUMN external_subject TEXT;

-- Existing deployments only recorded an email address and therefore cannot prove that a target
-- was an internal channel. Preserve the truthful interpretation during rollout. Fresh writes
-- carry the internal channel id when one was resolved before the outreach was created.
UPDATE task_outreach_targets AS target
SET company_id = outreach.company_id,
    external_transport = 'email',
    external_namespace = 'email',
    external_subject = lower(target.email::text)
FROM task_outreaches AS outreach
WHERE outreach.id = target.outreach_id;

ALTER TABLE task_outreach_targets
    ALTER COLUMN company_id SET NOT NULL,
    ADD CONSTRAINT task_outreach_targets_company_id_id_key UNIQUE (company_id, id),
    ADD CONSTRAINT task_outreach_targets_kind_check
        CHECK (target_kind IN ('internal_channel', 'external')),
    ADD CONSTRAINT task_outreach_targets_transport_check
        CHECK (external_transport IS NULL OR external_transport IN ('email', 'slack')),
    ADD CONSTRAINT task_outreach_targets_identity_check CHECK (
        (target_kind = 'internal_channel'
         AND internal_channel_id IS NOT NULL
         AND external_transport IS NULL
         AND external_namespace IS NULL
         AND external_subject IS NULL)
        OR
        (target_kind = 'external'
         AND internal_channel_id IS NULL
         AND external_transport IS NOT NULL
         AND external_namespace IS NOT NULL
         AND external_subject IS NOT NULL
         AND btrim(external_namespace) <> ''
         AND btrim(external_subject) <> '')
    ),
    DROP CONSTRAINT task_outreach_targets_outreach_id_fkey,
    ADD CONSTRAINT task_outreach_targets_outreach_fk
        FOREIGN KEY (company_id, outreach_id)
        REFERENCES task_outreaches(company_id, id) ON DELETE CASCADE,
    ADD CONSTRAINT task_outreach_targets_internal_channel_fk
        FOREIGN KEY (company_id, internal_channel_id)
        REFERENCES channels(company_id, id) ON DELETE CASCADE;
