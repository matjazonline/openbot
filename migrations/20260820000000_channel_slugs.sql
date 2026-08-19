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

INSERT INTO channel_slugs (company_id, channel_id, slug, is_primary)
    SELECT company_id, id, slug, TRUE FROM channels;

ALTER TABLE channels DROP CONSTRAINT channels_company_slug_key;
ALTER TABLE channels DROP CONSTRAINT channels_slug_format;
ALTER TABLE channels DROP COLUMN slug;
