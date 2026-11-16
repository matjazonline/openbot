\set ON_ERROR_STOP on
BEGIN;
CREATE TEMP TABLE teardown_company(id integer PRIMARY KEY);
CREATE TEMP TABLE teardown_agent(id integer PRIMARY KEY, company_id integer REFERENCES teardown_company(id) ON DELETE CASCADE);
CREATE TEMP TABLE teardown_principal(id integer PRIMARY KEY, company_id integer REFERENCES teardown_company(id) DEFERRABLE INITIALLY DEFERRED,
    agent_id integer REFERENCES teardown_agent(id) DEFERRABLE INITIALLY DEFERRED);
CREATE TEMP TABLE teardown_department(id integer PRIMARY KEY, company_id integer REFERENCES teardown_company(id) DEFERRABLE INITIALLY DEFERRED,
    parent_id integer REFERENCES teardown_department(id));
CREATE TEMP TABLE teardown_channel(id integer PRIMARY KEY, company_id integer REFERENCES teardown_company(id) DEFERRABLE INITIALLY DEFERRED,
    department_id integer REFERENCES teardown_department(id) ON DELETE RESTRICT);
CREATE TEMP TABLE teardown_history(id integer PRIMARY KEY, company_id integer REFERENCES teardown_company(id) ON DELETE CASCADE,
    channel_id integer REFERENCES teardown_channel(id));
CREATE TEMP TABLE teardown_order(sequence integer GENERATED ALWAYS AS IDENTITY, fact text);
CREATE FUNCTION pg_temp.teardown_history_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM teardown_company WHERE id=OLD.company_id) THEN
        RAISE EXCEPTION 'history immutable while company exists' USING ERRCODE='23514';
    END IF;
    INSERT INTO teardown_order(fact) VALUES('history');
    RETURN OLD;
END $$;
CREATE TRIGGER history_guard BEFORE DELETE ON teardown_history FOR EACH ROW EXECUTE FUNCTION pg_temp.teardown_history_guard();
CREATE FUNCTION pg_temp.teardown_after_statement() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM teardown_channel WHERE company_id IN (SELECT id FROM removed_companies);
    INSERT INTO teardown_order(fact) VALUES('channel');
    DELETE FROM teardown_department WHERE company_id IN (SELECT id FROM removed_companies);
    INSERT INTO teardown_order(fact) VALUES('department');
    DELETE FROM teardown_principal WHERE company_id IN (SELECT id FROM removed_companies);
    INSERT INTO teardown_order(fact) VALUES('principal');
    RETURN NULL;
END $$;
CREATE TRIGGER company_tree_cleanup AFTER DELETE ON teardown_company REFERENCING OLD TABLE AS removed_companies
    FOR EACH STATEMENT EXECUTE FUNCTION pg_temp.teardown_after_statement();
INSERT INTO teardown_company VALUES(1);
INSERT INTO teardown_agent VALUES(10,1);
INSERT INTO teardown_principal VALUES(11,1,10);
INSERT INTO teardown_department VALUES(20,1,NULL),(21,1,20),(22,1,21);
INSERT INTO teardown_channel VALUES(30,1,22);
INSERT INTO teardown_history VALUES(40,1,30);
DO $$ BEGIN
    BEGIN
        DELETE FROM teardown_history WHERE id=40;
        RAISE EXCEPTION 'negative control unexpectedly accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
END $$;
DELETE FROM teardown_company WHERE id=1;
SET CONSTRAINTS ALL IMMEDIATE;
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM teardown_company) OR EXISTS (SELECT 1 FROM teardown_agent)
      OR EXISTS (SELECT 1 FROM teardown_principal) OR EXISTS (SELECT 1 FROM teardown_department)
      OR EXISTS (SELECT 1 FROM teardown_channel) OR EXISTS (SELECT 1 FROM teardown_history) THEN
        RAISE EXCEPTION 'teardown leaked a row';
    END IF;
    IF (SELECT array_agg(fact ORDER BY sequence) FROM teardown_order) <> ARRAY['history','channel','department','principal'] THEN
        RAISE EXCEPTION 'unexpected teardown order';
    END IF;
END $$;
SELECT 'PASS: parent-absence immutability, tenant cascades before transition-table statement cleanup, channel->department->principal, deferred FKs checked' AS result;
ROLLBACK;
