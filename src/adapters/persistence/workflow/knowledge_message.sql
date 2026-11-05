-- A complete destination group has one MVCC snapshot. Concurrent CC/address/level edits
-- after this statement are ordered after this freeze, without a new thread lock protocol.
WITH requested AS (
    SELECT item.kind, item.id, item.recipient, item.frozen FROM jsonb_to_recordset($3::jsonb) AS item(kind text, id uuid, recipient text, frozen boolean)
), run AS (
    SELECT channel_id, thread_id, delivery_policy FROM workflow_runs
    WHERE company_id = $1 AND id = $2
), resolved AS (
    SELECT requested.kind, requested.id, requested.recipient, requested.frozen, run.delivery_policy,
        CASE requested.kind
            WHEN 'channel' THEN EXISTS (SELECT 1 FROM channels WHERE company_id = $1 AND id = requested.id)
            WHEN 'user' THEN EXISTS (SELECT 1 FROM company_members WHERE company_id = $1 AND user_id = requested.id)
            WHEN 'reply' THEN source.associations = 1
            ELSE false
        END AS valid,
        source.channel_id, source.thread_id, source.audience, source.sender_identity
    FROM requested CROSS JOIN run
    LEFT JOIN LATERAL (
        SELECT association.channel_id, association.thread_id, message.audience,
            count(*) OVER () AS associations,
            (SELECT participant.participant_identity_id FROM message_participants AS participant
             WHERE participant.company_id = $1 AND participant.message_id = message.id
               AND participant.kind = 'sender' ORDER BY participant.position LIMIT 1) AS sender_identity
        FROM thread_messages AS association JOIN messages AS message
            ON (message.company_id, message.id) = (association.company_id, association.message_id)
        WHERE requested.kind = 'reply' AND association.company_id = $1
          AND association.message_id = requested.id
          AND (run.channel_id IS NULL OR association.channel_id = run.channel_id)
          AND (run.thread_id IS NULL OR association.thread_id = run.thread_id)
        LIMIT 1
    ) AS source ON true
), receivers AS (
    SELECT COALESCE(valid, false) AS valid, 'internal'::text AS clearance,
        NULL::uuid AS user_id, NULL::uuid AS identity_id,
        NULL::uuid AS channel_id, NULL::uuid AS thread_id, frozen, recipient
    FROM resolved
    WHERE kind = 'channel' OR delivery_policy = 'in_app_only' OR (kind = 'reply' AND audience = 'internal_only')
    UNION ALL
    SELECT COALESCE(valid, false), 'email', CASE WHEN kind = 'user' THEN id END,
        CASE WHEN kind = 'reply' THEN sender_identity END, NULL, NULL, frozen, recipient
    FROM resolved
    WHERE kind IN ('user', 'reply') AND delivery_policy <> 'in_app_only'
      AND (kind = 'user' OR audience <> 'internal_only')
    UNION ALL
    SELECT COALESCE(valid, false), 'thread', NULL, NULL, channel_id, thread_id, frozen, recipient
    FROM resolved
    WHERE kind = 'reply' AND audience <> 'internal_only' AND delivery_policy <> 'in_app_only'
), facts AS (
    SELECT receiver.valid, receiver.clearance, principal.kind, member.role AS membership,
        member.access_level::smallint AS access_level,
        COALESCE(company.user_id = principal.user_id, false) AS is_owner
    FROM receivers AS receiver
    LEFT JOIN LATERAL (
        SELECT DISTINCT principal.kind, principal.user_id
        FROM principals AS principal
        JOIN thread_principals AS party ON (party.company_id, party.principal_id) = (principal.company_id, principal.id)
        WHERE receiver.clearance = 'thread' AND party.company_id = $1
          AND party.channel_id = receiver.channel_id AND party.thread_id = receiver.thread_id
          AND principal.kind IN ('person', 'external')
        UNION
        SELECT principal.kind, principal.user_id
        FROM principals AS principal
        JOIN users AS account ON account.id = principal.user_id
        JOIN participant_identities AS identity ON identity.company_id = principal.company_id
            AND identity.principal_id = principal.id
        WHERE receiver.clearance = 'email' AND principal.company_id = $1
          AND account.email_verified_at IS NOT NULL AND identity.transport = 'email'
          AND identity.status = 'verified' AND lower(identity.subject) = lower(account.email)
          AND (NOT receiver.frozen OR lower(identity.subject) = lower(receiver.recipient))
          AND ((receiver.user_id IS NOT NULL AND principal.user_id = receiver.user_id)
            OR (receiver.identity_id IS NOT NULL AND identity.id = receiver.identity_id))
    ) AS principal ON true
    LEFT JOIN company_members AS member ON member.company_id = $1 AND member.user_id = principal.user_id
    JOIN companies AS company ON company.id = $1
)
SELECT DISTINCT valid, clearance, kind, membership, access_level, is_owner FROM facts LIMIT $4
