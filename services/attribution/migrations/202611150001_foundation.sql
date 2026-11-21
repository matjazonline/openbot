-- No backward compatibility: incompatible schemas require an operator-managed reset.
CREATE TABLE attribution_schema_identity (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    application TEXT NOT NULL CHECK (application = 'attribution_service')
);
INSERT INTO attribution_schema_identity (singleton, application)
VALUES (TRUE, 'attribution_service');

CREATE TABLE attribution_login_transactions (
    id uuid PRIMARY KEY,
    state_verifier bytea NOT NULL UNIQUE CHECK (octet_length(state_verifier) = 32),
    browser_verifier bytea NOT NULL CHECK (octet_length(browser_verifier) = 32),
    encrypted_pkce bytea NOT NULL CHECK (octet_length(encrypted_pkce) BETWEEN 44 AND 256),
    return_path text NOT NULL CHECK (length(return_path) BETWEEN 1 AND 2048),
    expires_at timestamptz NOT NULL DEFAULT (clock_timestamp() + interval '10 minutes')
);
CREATE INDEX attribution_login_expiry ON attribution_login_transactions (expires_at);
CREATE TABLE attribution_sessions (
    id uuid PRIMARY KEY,
    verifier bytea NOT NULL UNIQUE CHECK (octet_length(verifier) = 32),
    user_id uuid NOT NULL,
    encrypted_secrets bytea NOT NULL CHECK (octet_length(encrypted_secrets) BETWEEN 44 AND 2048),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    absolute_expires_at timestamptz NOT NULL,
    idle_expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CHECK (absolute_expires_at > created_at AND idle_expires_at <= absolute_expires_at)
);
CREATE INDEX attribution_session_expiry ON attribution_sessions (absolute_expires_at);
CREATE SEQUENCE attribution_observation_order;
CREATE TABLE attribution_tenants (
    company_id uuid PRIMARY KEY,
    display_name text CHECK (length(display_name) BETWEEN 1 AND 120),
    version bigint NOT NULL CHECK (version > 0),
    state text NOT NULL CHECK (state IN ('active', 'suspended', 'deleted')),
    changed_at timestamptz NOT NULL,
    observation_number bigint NOT NULL DEFAULT 0 CHECK (observation_number >= 0),
    status_valid_until timestamptz,
    purge_requested boolean NOT NULL DEFAULT false,
    reconcile_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    reconcile_execution uuid,
    reconcile_lease_until timestamptz,
    reconcile_failures integer NOT NULL DEFAULT 0 CHECK (reconcile_failures BETWEEN 0 AND 12),
    last_reconcile_failure timestamptz,
    maintenance_due_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    maintenance_failure_at timestamptz,
    purged_at timestamptz,
    CHECK ((reconcile_execution IS NULL) = (reconcile_lease_until IS NULL)),
    CHECK (state <> 'deleted' OR (display_name IS NULL AND purge_requested))
);
CREATE INDEX attribution_tenants_reconcile ON attribution_tenants (reconcile_at, company_id)
    WHERE state <> 'deleted';

CREATE INDEX attribution_tenants_maintenance ON attribution_tenants(maintenance_due_at,company_id) WHERE purged_at IS NULL;

CREATE TABLE attribution_stores (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL REFERENCES attribution_tenants(company_id),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    timezone text NOT NULL CHECK (length(timezone) BETWEEN 1 AND 64),
    enabled boolean NOT NULL DEFAULT true,
    UNIQUE(company_id, id)
);
CREATE TABLE attribution_domains (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    host text NOT NULL CHECK (octet_length(host) BETWEEN 3 AND 240),
    challenge text NOT NULL CHECK (length(challenge) = 43),
    verified_until timestamptz,
    revoked_at timestamptz,
    FOREIGN KEY(company_id, store_id) REFERENCES attribution_stores(company_id, id),
    UNIQUE(company_id, store_id, id)
);
CREATE UNIQUE INDEX attribution_domain_live_host ON attribution_domains(company_id, store_id, host)
    WHERE revoked_at IS NULL;
CREATE TABLE attribution_installations (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    domain_id uuid NOT NULL,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    enabled boolean NOT NULL DEFAULT true,
    FOREIGN KEY(company_id, store_id, domain_id) REFERENCES attribution_domains(company_id, store_id, id),
    UNIQUE(company_id, store_id, id)
);
CREATE TABLE attribution_credentials (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    label text NOT NULL CHECK (length(label) BETWEEN 1 AND 80),
    verifier bytea NOT NULL UNIQUE CHECK (octet_length(verifier) = 32),
    scopes text[] NOT NULL CHECK (cardinality(scopes) BETWEEN 1 AND 5 AND
        scopes <@ ARRAY['conversions:write','checkouts:write','links:read','links:write','analytics:read']::text[]),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz,
    revoked_at timestamptz,
    last_used_at timestamptz,
    rotated_to uuid,
    FOREIGN KEY(company_id, store_id) REFERENCES attribution_stores(company_id, id),
    UNIQUE(company_id, store_id, id),
    FOREIGN KEY(company_id, store_id, rotated_to) REFERENCES attribution_credentials(company_id, store_id, id)
);
CREATE TABLE attribution_management_receipts (
    company_id uuid NOT NULL REFERENCES attribution_tenants(company_id),
    principal_kind text NOT NULL CHECK (principal_kind IN ('user', 'merchant')),
    principal_id uuid NOT NULL,
    operation text NOT NULL CHECK (length(operation) BETWEEN 1 AND 256),
    key uuid NOT NULL,
    original_time text NOT NULL CHECK (length(original_time) BETWEEN 1 AND 40),
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint) = 32),
    result jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id, principal_kind, principal_id, operation, key)
);
CREATE INDEX attribution_receipt_retention ON attribution_management_receipts(created_at);
CREATE TABLE attribution_audit (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL REFERENCES attribution_tenants(company_id),
    actor_kind text NOT NULL CHECK (actor_kind IN ('user', 'merchant')),
    actor_id uuid NOT NULL,
    action text NOT NULL CHECK (length(action) BETWEEN 1 AND 64),
    resource_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX attribution_audit_company ON attribution_audit(company_id, created_at, id);


CREATE TABLE attribution_campaigns (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    enabled boolean NOT NULL DEFAULT true,
    FOREIGN KEY(company_id,store_id) REFERENCES attribution_stores(company_id,id),
    UNIQUE(company_id,store_id,id)
);
CREATE TABLE attribution_ads (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    campaign_id uuid NOT NULL,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    advertised_product_id text CHECK (length(advertised_product_id) BETWEEN 1 AND 128),
    enabled boolean NOT NULL DEFAULT true,
    FOREIGN KEY(company_id,store_id,campaign_id) REFERENCES attribution_campaigns(company_id,store_id,id),
    UNIQUE(company_id,store_id,id)
);
CREATE TABLE attribution_links (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    ad_id uuid NOT NULL,
    code text NOT NULL UNIQUE CHECK (code ~ '^[A-Za-z0-9_-]{24}$'),
    destination text NOT NULL CHECK (octet_length(destination) BETWEEN 1 AND 2048),
    placement text NOT NULL CHECK (length(placement) BETWEEN 1 AND 120),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    enabled boolean NOT NULL DEFAULT true,
    FOREIGN KEY(company_id,store_id,ad_id) REFERENCES attribution_ads(company_id,store_id,id),
    UNIQUE(company_id,store_id,id)
);
CREATE TABLE attribution_clicks (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    link_id uuid NOT NULL,
    clicked_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    dimensions jsonb NOT NULL CHECK (jsonb_typeof(dimensions)='object' AND octet_length(dimensions::text)<=16384),
    is_bot boolean NOT NULL,
    is_prefetch boolean NOT NULL,
    classifier_version smallint NOT NULL CHECK (classifier_version > 0),
    FOREIGN KEY(company_id,store_id,link_id) REFERENCES attribution_links(company_id,store_id,id),
    UNIQUE(company_id,store_id,id)
);
CREATE INDEX attribution_clicks_store_time ON attribution_clicks(company_id,store_id,clicked_at,id);


CREATE TABLE attribution_observations (
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    installation_id uuid NOT NULL,
    event_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('image_load','landing_page_load','visible_placement')),
    click_id uuid,
    link_id uuid,
    observed_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    dimensions jsonb NOT NULL CHECK (jsonb_typeof(dimensions)='object' AND octet_length(dimensions::text)<=16384),
    visibility_percent smallint,
    visibility_millis integer,
    observation_version smallint NOT NULL DEFAULT 1 CHECK (observation_version=1),
    PRIMARY KEY(installation_id,event_id),
    FOREIGN KEY(company_id,store_id,installation_id) REFERENCES attribution_installations(company_id,store_id,id),
    CHECK (click_id IS NOT NULL OR link_id IS NOT NULL),
    CHECK ((kind='visible_placement' AND visibility_percent IS NOT NULL AND visibility_millis IS NOT NULL AND visibility_percent=50 AND visibility_millis=1000) OR (kind<>'visible_placement' AND visibility_percent IS NULL AND visibility_millis IS NULL))
);
CREATE INDEX attribution_observations_store_time ON attribution_observations(company_id,store_id,observed_at,event_id);
CREATE INDEX attribution_observations_retention ON attribution_observations(received_at,installation_id,event_id);
CREATE TABLE attribution_checkouts (
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    checkout_id text NOT NULL CHECK (length(checkout_id) BETWEEN 1 AND 128),
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    -- Canonical text preserves the attested instant's nanoseconds for equality
    -- and exact attribution bounds; received_at is the database retention clock.
    order_created_at text NOT NULL CHECK (length(order_created_at) BETWEEN 1 AND 40),
    click_id uuid,
    reason text NOT NULL CHECK (reason IN ('attributed','missing','no_consent','expired','future','bot','foreign_or_missing','evidence_unavailable')),
    policy text NOT NULL CHECK (policy='last-eligible-click-30d-v1'),
    evidence jsonb CHECK (jsonb_typeof(evidence)='object' AND octet_length(evidence::text)<=16384),
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(company_id,store_id,checkout_id),
    FOREIGN KEY(company_id,store_id) REFERENCES attribution_stores(company_id,id),
    CHECK ((reason='attributed' AND click_id IS NOT NULL AND evidence IS NOT NULL) OR (reason<>'attributed' AND click_id IS NULL AND evidence IS NULL))
);
CREATE INDEX attribution_checkouts_retention ON attribution_checkouts(received_at,company_id,store_id,checkout_id);


-- Scoped immutable commerce facts. Pending order placeholders are never sales.
CREATE TABLE attribution_orders (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    external_order_id text NOT NULL CHECK (char_length(external_order_id) BETWEEN 1 AND 128),
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    cancelled_at timestamptz,
    retired_at timestamptz,
    paid_fingerprint bytea CHECK (paid_fingerprint IS NULL OR octet_length(paid_fingerprint)=32),
    order_created_at text,
    paid_original text,
    paid_at timestamptz,
    currency text CHECK (currency IN ('EUR','USD','GBP','JPY')),
    merchandise_amount bigint CHECK (merchandise_amount BETWEEN 0 AND 9000000000000),
    tax_amount bigint CHECK (tax_amount BETWEEN 0 AND 9000000000000),
    shipping_amount bigint CHECK (shipping_amount BETWEEN 0 AND 9000000000000),
    refunded_merchandise_amount bigint NOT NULL DEFAULT 0 CHECK (refunded_merchandise_amount>=0),
    refunded_tax_amount bigint NOT NULL DEFAULT 0 CHECK (refunded_tax_amount>=0),
    refunded_shipping_amount bigint NOT NULL DEFAULT 0 CHECK (refunded_shipping_amount>=0),
    returned_units bigint NOT NULL DEFAULT 0 CHECK (returned_units BETWEEN 0 AND 100000000),
    click_id uuid,
    ad_id uuid,
    attribution_reason text CHECK (attribution_reason IN ('attributed','missing','no_consent','expired','future','bot','foreign_or_missing','evidence_unavailable')),
    policy text CHECK (policy='last-eligible-click-30d-v1'),
    evidence jsonb,
    dimensions jsonb,
    FOREIGN KEY(company_id,store_id) REFERENCES attribution_stores(company_id,id) ON DELETE CASCADE,
    UNIQUE(company_id,store_id,external_order_id),
    UNIQUE(company_id,store_id,id),
    CHECK ((paid_fingerprint IS NULL AND paid_at IS NULL AND currency IS NULL AND merchandise_amount IS NULL AND tax_amount IS NULL AND shipping_amount IS NULL AND order_created_at IS NULL AND paid_original IS NULL AND attribution_reason IS NULL AND policy IS NULL)
        OR (paid_fingerprint IS NOT NULL AND paid_at IS NOT NULL AND currency IS NOT NULL AND merchandise_amount IS NOT NULL AND tax_amount IS NOT NULL AND shipping_amount IS NOT NULL AND order_created_at IS NOT NULL AND paid_original IS NOT NULL AND attribution_reason IS NOT NULL AND policy IS NOT NULL)),
    CHECK (merchandise_amount+tax_amount+shipping_amount<=9000000000000),
    CHECK (refunded_merchandise_amount<=COALESCE(merchandise_amount,0) AND refunded_tax_amount<=COALESCE(tax_amount,0) AND refunded_shipping_amount<=COALESCE(shipping_amount,0)),
    CHECK ((attribution_reason='attributed' AND click_id IS NOT NULL AND ad_id IS NOT NULL AND evidence IS NOT NULL AND dimensions IS NOT NULL)
        OR (attribution_reason IS DISTINCT FROM 'attributed' AND click_id IS NULL AND ad_id IS NULL AND evidence IS NULL AND dimensions IS NULL))
);
CREATE INDEX attribution_orders_paid ON attribution_orders(company_id,paid_at,id) WHERE paid_at IS NOT NULL;
CREATE INDEX attribution_orders_retention ON attribution_orders(company_id,retired_at,received_at,id);
CREATE TABLE attribution_order_lines (
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    order_id uuid NOT NULL,
    line_id text NOT NULL CHECK (char_length(line_id) BETWEEN 1 AND 128),
    product_id text NOT NULL CHECK (char_length(product_id) BETWEEN 1 AND 128),
    quantity bigint NOT NULL CHECK (quantity BETWEEN 1 AND 1000000),
    merchandise_amount bigint NOT NULL CHECK (merchandise_amount BETWEEN 0 AND 9000000000000),
    returned_quantity bigint NOT NULL DEFAULT 0 CHECK (returned_quantity BETWEEN 0 AND quantity),
    refunded_amount bigint NOT NULL DEFAULT 0 CHECK (refunded_amount BETWEEN 0 AND merchandise_amount),
    PRIMARY KEY(company_id,store_id,order_id,line_id),
    FOREIGN KEY(company_id,store_id,order_id) REFERENCES attribution_orders(company_id,store_id,id) ON DELETE CASCADE
);
CREATE INDEX attribution_order_lines_product ON attribution_order_lines(company_id,product_id,order_id);
CREATE TABLE attribution_refunds (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    order_id uuid NOT NULL,
    external_refund_id text NOT NULL CHECK (char_length(external_refund_id) BETWEEN 1 AND 128),
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    envelope jsonb NOT NULL CHECK (jsonb_typeof(envelope)='object'),
    occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    applied_at timestamptz,
    currency text NOT NULL CHECK (currency IN ('EUR','USD','GBP','JPY')),
    merchandise_amount bigint NOT NULL CHECK (merchandise_amount BETWEEN 0 AND 9000000000000),
    tax_amount bigint NOT NULL CHECK (tax_amount BETWEEN 0 AND 9000000000000),
    shipping_amount bigint NOT NULL CHECK (shipping_amount BETWEEN 0 AND 9000000000000),
    FOREIGN KEY(company_id,store_id,order_id) REFERENCES attribution_orders(company_id,store_id,id) ON DELETE CASCADE,
    UNIQUE(company_id,store_id,external_refund_id),
    UNIQUE(company_id,store_id,order_id,id)
);
CREATE INDEX attribution_refunds_occurred ON attribution_refunds(company_id,occurred_at,id) WHERE applied_at IS NOT NULL;
CREATE TABLE attribution_refund_lines (
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    order_id uuid NOT NULL,
    refund_id uuid NOT NULL,
    line_id text NOT NULL,
    returned_quantity bigint NOT NULL CHECK (returned_quantity BETWEEN 0 AND 1000000),
    merchandise_amount bigint NOT NULL CHECK (merchandise_amount BETWEEN 0 AND 9000000000000),
    PRIMARY KEY(refund_id,line_id),
    FOREIGN KEY(company_id,store_id,order_id,refund_id) REFERENCES attribution_refunds(company_id,store_id,order_id,id) ON DELETE CASCADE,
    FOREIGN KEY(company_id,store_id,order_id,line_id) REFERENCES attribution_order_lines(company_id,store_id,order_id,line_id)
);
CREATE TABLE attribution_merchant_events (
    id uuid PRIMARY KEY,
    company_id uuid NOT NULL,
    store_id uuid NOT NULL,
    order_id uuid NOT NULL,
    external_event_id text NOT NULL CHECK (char_length(external_event_id) BETWEEN 1 AND 128),
    kind text NOT NULL CHECK (kind IN ('paid','refund','cancelled')),
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    envelope jsonb NOT NULL CHECK (jsonb_typeof(envelope)='object'),
    occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    state text NOT NULL CHECK (state IN ('accepted','pending','conflict','unresolved')),
    reason text CHECK (char_length(reason) BETWEEN 1 AND 64),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 20),
    next_attempt_at timestamptz,
    deadline_at timestamptz NOT NULL DEFAULT (clock_timestamp()+interval '7 days'),
    execution uuid,
    lease_until timestamptz,
    FOREIGN KEY(company_id,store_id,order_id) REFERENCES attribution_orders(company_id,store_id,id) ON DELETE CASCADE,
    UNIQUE(company_id,store_id,external_event_id),
    CHECK ((execution IS NULL)=(lease_until IS NULL)),
    CHECK ((state='pending')=(next_attempt_at IS NOT NULL)),
    CHECK (state='pending' OR execution IS NULL)
);
CREATE INDEX attribution_merchant_events_due ON attribution_merchant_events(next_attempt_at,company_id,id) WHERE state='pending';
CREATE INDEX attribution_merchant_events_company ON attribution_merchant_events(company_id,id);
