CREATE TABLE workflow_http_connections (
    company_id uuid NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    id uuid NOT NULL,
    origin text NOT NULL CHECK (octet_length(origin) BETWEEN 8 AND 2048),
    enabled boolean NOT NULL DEFAULT true,
    bearer boolean NOT NULL,
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id, id)
);
CREATE TABLE workflow_http_credentials (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    envelope text NOT NULL CHECK (octet_length(envelope) BETWEEN 1 AND 16384 AND envelope LIKE 'enc:v2:%'),
    PRIMARY KEY (company_id, connection_id),
    FOREIGN KEY (company_id, connection_id) REFERENCES workflow_http_connections(company_id, id) ON DELETE CASCADE
);
CREATE TABLE workflow_http_operations (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    name text NOT NULL CHECK (octet_length(name) BETWEEN 1 AND 128),
    snapshot jsonb NOT NULL CHECK (octet_length(snapshot::text) <= 65536 AND jsonb_typeof(snapshot) = 'object'),
    approval_required boolean NOT NULL,
    PRIMARY KEY (company_id, connection_id, name),
    FOREIGN KEY (company_id, connection_id) REFERENCES workflow_http_connections(company_id, id) ON DELETE CASCADE,
    CHECK (snapshot->>'company_id' = company_id::text),
    CHECK (snapshot->'contract'->>'name' = name),
    CHECK (snapshot->'contract'->'connection' = 'null'::jsonb),
    CHECK (snapshot->'policy'->>'recovery' = 'reconcile')
);
CREATE FUNCTION workflow_http_identity_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.origin, NEW.bearer, NEW.created_at)
        IS DISTINCT FROM (OLD.company_id, OLD.id, OLD.origin, OLD.bearer, OLD.created_at) THEN
        RAISE EXCEPTION 'HTTP connection identity is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER workflow_http_identity_immutable BEFORE UPDATE ON workflow_http_connections
FOR EACH ROW EXECUTE FUNCTION workflow_http_identity_immutable();
