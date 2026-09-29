-- Budget identity is immutable. Mutable consumption must never reference a run:
-- repeated counter UPDATEs would otherwise acquire the root run's FK lock.
CREATE TABLE workflow_root_budgets (
    company_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    provenance text NOT NULL CHECK (provenance IN ('frozen_v2','sealed_legacy')),
    activations integer,
    model_calls integer,
    repetitions integer,
    PRIMARY KEY (company_id,root_run_id),
    FOREIGN KEY (company_id,root_run_id) REFERENCES workflow_runs(company_id,id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CHECK ((provenance='sealed_legacy' AND activations IS NULL AND model_calls IS NULL AND repetitions IS NULL)
        OR (provenance='frozen_v2' AND activations IS NOT NULL AND activations BETWEEN 1 AND 100000
            AND model_calls IS NOT NULL AND model_calls BETWEEN 1 AND 1000
            AND repetitions IS NOT NULL AND repetitions BETWEEN 1 AND 1000))
);

ALTER TABLE workflow_runs ADD CONSTRAINT workflow_run_budget_parent_identity UNIQUE (company_id,id,parent_run_id);
CREATE TABLE workflow_run_budgets (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    parent_run_id uuid,
    PRIMARY KEY (company_id,run_id),
    UNIQUE (company_id,run_id,root_run_id),
    FOREIGN KEY (company_id,run_id) REFERENCES workflow_runs(company_id,id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (company_id,run_id,parent_run_id) REFERENCES workflow_runs(company_id,id,parent_run_id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (company_id,root_run_id) REFERENCES workflow_root_budgets(company_id,root_run_id),
    FOREIGN KEY (company_id,parent_run_id,root_run_id)
        REFERENCES workflow_run_budgets(company_id,run_id,root_run_id),
    CHECK ((parent_run_id IS NULL AND run_id=root_run_id)
        OR (parent_run_id IS NOT NULL AND run_id<>root_run_id AND run_id<>parent_run_id))
);

CREATE TABLE workflow_root_budget_usage (
    company_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    activations bigint NOT NULL DEFAULT 0 CHECK (activations>=0),
    model_calls bigint NOT NULL DEFAULT 0 CHECK (model_calls>=0),
    repetitions bigint NOT NULL DEFAULT 0 CHECK (repetitions>=0),
    PRIMARY KEY (company_id,root_run_id),
    FOREIGN KEY (company_id,root_run_id) REFERENCES workflow_root_budgets(company_id,root_run_id) ON DELETE CASCADE
);

CREATE TABLE workflow_budget_receipts (
    company_id uuid NOT NULL,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    root_run_id uuid NOT NULL,
    resource text NOT NULL CHECK (resource IN ('activation','model_call','repetition')),
    reservation_key text NOT NULL CHECK (octet_length(reservation_key) BETWEEN 1 AND 128
        AND reservation_key ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]*$'),
    quantity integer NOT NULL CHECK (quantity>0 AND quantity<=CASE WHEN resource='activation' THEN 100000 ELSE 1000 END),
    disposition text NOT NULL CHECK (disposition IN ('granted','exhausted')),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (company_id,run_id,execution_id,resource,reservation_key),
    FOREIGN KEY (company_id,run_id,execution_id) REFERENCES workflow_executions(company_id,run_id,id) ON DELETE CASCADE,
    FOREIGN KEY (company_id,run_id,root_run_id) REFERENCES workflow_run_budgets(company_id,run_id,root_run_id),
    CHECK (resource<>'activation' OR (quantity=1 AND reservation_key='activation'))
);

-- Retained runs have no frozen v2 ceilings. Preserve exact historical activation
-- facts without inventing an allowance. Prior workflow model/repetition consumers
-- did not exist; their provable historical usage is zero. Sealed roots reject all
-- fresh reservations, irrespective of counters or the unreadable v1 bundle.
INSERT INTO workflow_root_budgets(company_id,root_run_id,provenance)
    SELECT company_id,id,'sealed_legacy' FROM workflow_runs WHERE parent_run_id IS NULL;
WITH RECURSIVE lineage AS (
    SELECT company_id,id AS run_id,id AS root_run_id,parent_run_id FROM workflow_runs WHERE parent_run_id IS NULL
    UNION ALL
    SELECT child.company_id,child.id,lineage.root_run_id,child.parent_run_id
    FROM workflow_runs AS child JOIN lineage ON lineage.company_id=child.company_id AND lineage.run_id=child.parent_run_id
)
INSERT INTO workflow_run_budgets(company_id,run_id,root_run_id,parent_run_id)
    SELECT company_id,run_id,root_run_id,parent_run_id FROM lineage;
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM workflow_runs AS run LEFT JOIN workflow_run_budgets AS link
        ON link.company_id=run.company_id AND link.run_id=run.id WHERE link.run_id IS NULL) THEN
        RAISE EXCEPTION 'Cannot prove retained workflow budget lineage';
    END IF;
END $$;
INSERT INTO workflow_budget_receipts(company_id,run_id,execution_id,root_run_id,resource,reservation_key,quantity,disposition,created_at)
    SELECT execution.company_id,execution.run_id,execution.id,link.root_run_id,'activation','activation',1,'granted',execution.activated_at
    FROM workflow_executions AS execution JOIN workflow_run_budgets AS link
        ON link.company_id=execution.company_id AND link.run_id=execution.run_id WHERE execution.activated_at IS NOT NULL;
INSERT INTO workflow_root_budget_usage(company_id,root_run_id,activations)
    SELECT budget.company_id,budget.root_run_id,count(receipt.execution_id)
    FROM workflow_root_budgets AS budget LEFT JOIN workflow_budget_receipts AS receipt
        ON receipt.company_id=budget.company_id AND receipt.root_run_id=budget.root_run_id
    GROUP BY budget.company_id,budget.root_run_id;

CREATE FUNCTION insert_workflow_budget_link() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE root uuid; stored jsonb; limits jsonb;
BEGIN
    IF NEW.parent_run_id IS NOT NULL THEN
        SELECT root_run_id INTO STRICT root FROM workflow_run_budgets
            WHERE company_id=NEW.company_id AND run_id=NEW.parent_run_id;
    ELSE
        root=NEW.id;
        -- Invalid/old raw schema fixtures may remain as sealed records. They do
        -- not become executable or receive defaults through this SQL boundary.
        BEGIN
            stored=convert_from(NEW.bundle,'UTF8')::jsonb;
            IF stored->>'compiler_revision'='2' THEN
                SELECT version->'compiled'->'root_budget' INTO limits
                    FROM jsonb_array_elements(stored->'versions') AS version
                    WHERE version->>'version'=stored->>'root';
            END IF;
        EXCEPTION WHEN data_exception THEN
            limits=NULL;
        END;
        INSERT INTO workflow_root_budgets(company_id,root_run_id,provenance,activations,model_calls,repetitions)
            VALUES(NEW.company_id,root,CASE WHEN limits IS NULL THEN 'sealed_legacy' ELSE 'frozen_v2' END,
                (limits->>'activations')::integer,(limits->>'model_calls')::integer,(limits->>'repetitions')::integer);
        INSERT INTO workflow_root_budget_usage(company_id,root_run_id) VALUES(NEW.company_id,root);
    END IF;
    INSERT INTO workflow_run_budgets(company_id,run_id,root_run_id,parent_run_id)
        VALUES(NEW.company_id,NEW.id,root,NEW.parent_run_id);
    RETURN NEW;
END $$;
-- Run the established native parent-execution FK trigger first, preserving its
-- scoped rejection (including foreign parents) before looking up the budget.
CREATE TRIGGER workflow_root_budget_link BEFORE INSERT ON workflow_runs
    FOR EACH ROW EXECUTE FUNCTION insert_workflow_budget_link();

CREATE FUNCTION preserve_workflow_budget_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id
            AND id=CASE WHEN TG_TABLE_NAME='workflow_run_budgets' THEN (to_jsonb(OLD)->>'run_id')::uuid ELSE OLD.root_run_id END) THEN
            RAISE EXCEPTION 'Workflow budget identity cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow budget identity is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_root_budget_immutable BEFORE UPDATE OR DELETE ON workflow_root_budgets
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_budget_identity();
CREATE TRIGGER workflow_run_budget_immutable BEFORE UPDATE OR DELETE ON workflow_run_budgets
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_budget_identity();

CREATE FUNCTION preserve_workflow_budget_receipt() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_runs WHERE company_id=OLD.company_id AND id=OLD.run_id) THEN
            RAISE EXCEPTION 'Workflow budget receipt cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF NEW IS DISTINCT FROM OLD THEN
        RAISE EXCEPTION 'Workflow budget receipt is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_budget_receipt_immutable BEFORE UPDATE OR DELETE ON workflow_budget_receipts
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_budget_receipt();

-- The receipt is the sole debit writer. Check replay before touching usage:
-- BEFORE INSERT also executes for ON CONFLICT DO NOTHING/UPDATE.
CREATE FUNCTION debit_workflow_budget_receipt() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE usage workflow_root_budget_usage%ROWTYPE; budget workflow_root_budgets%ROWTYPE;
    receipt workflow_budget_receipts%ROWTYPE; consumed bigint; ceiling integer;
BEGIN
    PERFORM 1 FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id FOR UPDATE;
    SELECT * INTO receipt FROM workflow_budget_receipts WHERE company_id=NEW.company_id
        AND run_id=NEW.run_id AND execution_id=NEW.execution_id AND resource=NEW.resource AND reservation_key=NEW.reservation_key;
    IF FOUND THEN
        IF (NEW.root_run_id,NEW.quantity) IS DISTINCT FROM (receipt.root_run_id,receipt.quantity) THEN
            RAISE EXCEPTION 'Workflow budget reservation payload conflicts' USING ERRCODE='23514';
        END IF;
        NEW.disposition=receipt.disposition;
        RETURN NEW;
    END IF;
    SELECT * INTO STRICT usage FROM workflow_root_budget_usage
        WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id FOR UPDATE;
    SELECT * INTO STRICT budget FROM workflow_root_budgets
        WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id;
    consumed=CASE NEW.resource WHEN 'activation' THEN usage.activations WHEN 'model_call' THEN usage.model_calls ELSE usage.repetitions END;
    ceiling=CASE NEW.resource WHEN 'activation' THEN budget.activations WHEN 'model_call' THEN budget.model_calls ELSE budget.repetitions END;
    NEW.disposition=CASE WHEN budget.provenance='frozen_v2' AND NEW.quantity>0
        AND consumed<=ceiling::bigint-NEW.quantity THEN 'granted' ELSE 'exhausted' END;
    IF NEW.disposition='granted' THEN
        UPDATE workflow_root_budget_usage SET
            activations=activations+CASE WHEN NEW.resource='activation' THEN NEW.quantity ELSE 0 END,
            model_calls=model_calls+CASE WHEN NEW.resource='model_call' THEN NEW.quantity ELSE 0 END,
            repetitions=repetitions+CASE WHEN NEW.resource='repetition' THEN NEW.quantity ELSE 0 END
            WHERE company_id=NEW.company_id AND root_run_id=NEW.root_run_id;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_budget_receipt_debit BEFORE INSERT ON workflow_budget_receipts
    FOR EACH ROW EXECUTE FUNCTION debit_workflow_budget_receipt();

CREATE FUNCTION preserve_workflow_budget_usage() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='INSERT' THEN
        IF NEW.activations<>0 OR NEW.model_calls<>0 OR NEW.repetitions<>0 THEN
            RAISE EXCEPTION 'New workflow budget usage starts at zero' USING ERRCODE='23514';
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP='DELETE' THEN
        IF EXISTS (SELECT 1 FROM workflow_root_budgets WHERE company_id=OLD.company_id AND root_run_id=OLD.root_run_id) THEN
            RAISE EXCEPTION 'Workflow budget usage cannot be removed while its owner exists' USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF (NEW.company_id,NEW.root_run_id) IS DISTINCT FROM (OLD.company_id,OLD.root_run_id)
        OR NEW.activations<OLD.activations OR NEW.model_calls<OLD.model_calls OR NEW.repetitions<OLD.repetitions
        OR (NEW IS DISTINCT FROM OLD AND pg_trigger_depth()<2) THEN
        RAISE EXCEPTION 'Workflow budget usage changes require a receipt' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER workflow_budget_usage_guard BEFORE INSERT OR UPDATE OR DELETE ON workflow_root_budget_usage
    FOR EACH ROW EXECUTE FUNCTION preserve_workflow_budget_usage();

COMMENT ON TABLE workflow_budget_receipts IS 'Append-only accounting facts; never provider dispatch authority, execution ownership, or results.';
