SELECT id AS selected_id, company_id AS selected_company FROM channels LIMIT 1 \gset
EXPLAIN (ANALYZE, BUFFERS)

    SELECT ch.id, ch.company_id, ch.name, ch.description,
           (SELECT cs.slug::text FROM channel_slugs cs
            WHERE cs.channel_id = ch.id AND cs.is_primary) AS slug,
           COALESCE(
               (SELECT array_agg(cs.slug::text ORDER BY cs.slug::text)
                FROM channel_slugs cs
                WHERE cs.channel_id = ch.id AND NOT cs.is_primary),
               ARRAY[]::text[]) AS alias_slugs,
           CASE ch.access_mode
               WHEN 'public' THEN ARRAY['@public']::text[] || COALESCE(
                   (SELECT array_agg(identity.subject ORDER BY identity.subject)
                    FROM channel_principal_grants AS channel_grant
                    JOIN participant_identities AS identity
                      ON (identity.company_id, identity.principal_id) =
                         (channel_grant.company_id, channel_grant.principal_id)
                    WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id
                      AND channel_grant.capability = 'participate'
                      AND channel_grant.provenance = 'configured_allowlist'
                      AND identity.transport = 'email' AND identity.status <> 'disabled'),
                   ARRAY[]::text[])
               WHEN 'allowlist' THEN COALESCE(
                   (SELECT array_agg(identity.subject ORDER BY identity.subject)
                    FROM channel_principal_grants AS channel_grant
                    JOIN participant_identities AS identity
                      ON (identity.company_id, identity.principal_id) =
                         (channel_grant.company_id, channel_grant.principal_id)
                    WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id
                      AND channel_grant.capability = 'participate'
                      AND channel_grant.provenance = 'configured_allowlist'
                      AND identity.transport = 'email' AND identity.status <> 'disabled'),
                   ARRAY[]::text[])
               ELSE NULL::text[]
           END AS participant_emails,
           ch.access_mode,
           COALESCE(
               (SELECT jsonb_agg(jsonb_build_object(
                           'principal_id', channel_grant.principal_id,
                           'capability', channel_grant.capability,
                           'provenance', channel_grant.provenance,
                           'created_at', channel_grant.created_at)
                       ORDER BY channel_grant.created_at, channel_grant.principal_id, channel_grant.capability)
                FROM channel_principal_grants AS channel_grant
                WHERE channel_grant.company_id = ch.company_id AND channel_grant.channel_id = ch.id),
               '[]'::jsonb) AS principal_grants,
           ch.enabled, ch.add_3rd_party, ch.settings_revision, ch.created_by, ch.created_at
    FROM channels ch
 WHERE ch.company_id=:'selected_company'::uuid AND ch.id=ANY(ARRAY[:'selected_id'::uuid]);
