-- 04.10: a company MCP connection id always names one endpoint. Replacing an endpoint
-- creates a new connection, so a frozen workflow action cannot reach another service.
CREATE FUNCTION company_mcp_endpoint_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.company_id, NEW.id, NEW.endpoint_url, NEW.transport)
        IS DISTINCT FROM (OLD.company_id, OLD.id, OLD.endpoint_url, OLD.transport) THEN
        RAISE EXCEPTION 'MCP connection endpoint is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER company_mcp_endpoint_immutable BEFORE UPDATE ON company_mcp_connections
FOR EACH ROW EXECUTE FUNCTION company_mcp_endpoint_immutable();

-- Trusted workflow policy for one granted MCP tool. The grant foreign key makes grant
-- revocation (including refresh drops) revoke the workflow approval in the same commit.
-- `version` is a fresh random token per write, never reissued after deletion.
CREATE TABLE workflow_mcp_operations (
    company_id uuid NOT NULL,
    connection_id uuid NOT NULL,
    name text NOT NULL CHECK (octet_length(name) BETWEEN 1 AND 128),
    snapshot jsonb NOT NULL CHECK (octet_length(snapshot::text) <= 65536 AND jsonb_typeof(snapshot) = 'object'),
    approval_required boolean NOT NULL,
    version uuid NOT NULL,
    PRIMARY KEY (company_id, connection_id, name),
    FOREIGN KEY (company_id, connection_id) REFERENCES company_mcp_connections(company_id, id) ON DELETE CASCADE,
    FOREIGN KEY (company_id, connection_id, name)
        REFERENCES company_mcp_tool_grants(company_id, connection_id, tool_name) ON DELETE CASCADE,
    CHECK (snapshot->>'company_id' = company_id::text),
    CHECK (snapshot->'contract'->>'name' = name),
    CHECK (snapshot->'contract'->'connection' = 'null'::jsonb),
    CHECK (snapshot->'policy'->>'recovery' = 'reconcile')
);
