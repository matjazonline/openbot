-- Reviewer B, FIXAPP-P05-G1-R1: DDL validity of the D3/D4 generated-column designs on PG 18.
\set ON_ERROR_STOP 0
SELECT version();
BEGIN;
-- D4: generated destination_id, per-kind columns, unique key including destination_id.
CREATE TEMP TABLE d4 (
  company_id uuid NOT NULL, invocation_id uuid NOT NULL, kind text NOT NULL,
  destination_channel_id uuid, destination_user_id uuid, replied_message_id uuid,
  destination_id uuid GENERATED ALWAYS AS (COALESCE(destination_channel_id, destination_user_id, replied_message_id)) STORED NOT NULL,
  UNIQUE (company_id, invocation_id, kind, destination_id));
CREATE FUNCTION pg_temp.peek() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE NOTICE 'BEFORE INSERT sees destination_id=%', NEW.destination_id; RETURN NEW; END $$;
CREATE TRIGGER peek BEFORE INSERT ON d4 FOR EACH ROW EXECUTE FUNCTION pg_temp.peek();
INSERT INTO d4(company_id, invocation_id, kind, destination_user_id) VALUES (gen_random_uuid(), gen_random_uuid(), 'user', gen_random_uuid()) RETURNING destination_id IS NOT NULL AS generated_ok;
SAVEPOINT s1;
INSERT INTO d4(company_id, invocation_id, kind, destination_id) VALUES (gen_random_uuid(), gen_random_uuid(), 'user', gen_random_uuid());
ROLLBACK TO SAVEPOINT s1;
SAVEPOINT s2;
INSERT INTO d4(company_id, invocation_id, kind) VALUES (gen_random_uuid(), gen_random_uuid(), 'user');
ROLLBACK TO SAVEPOINT s2;
-- D3: generated booleans in a referenced UNIQUE and a referencing FK with ON DELETE CASCADE.
CREATE TEMP TABLE wb (company_id uuid NOT NULL, id uuid NOT NULL, channel_id uuid,
  company_wide boolean GENERATED ALWAYS AS (channel_id IS NULL) STORED,
  PRIMARY KEY (company_id, id), UNIQUE (company_id, channel_id, id), UNIQUE (company_id, id, company_wide));
CREATE TEMP TABLE cs (id uuid PRIMARY KEY, company_id uuid NOT NULL, channel_id uuid NOT NULL, binding_id uuid NOT NULL,
  binding_channel_id uuid CHECK (binding_channel_id IS NULL OR binding_channel_id = channel_id),
  binding_company_wide boolean GENERATED ALWAYS AS (binding_channel_id IS NULL) STORED,
  FOREIGN KEY (company_id, binding_id, binding_company_wide) REFERENCES wb(company_id, id, company_wide) ON DELETE CASCADE,
  FOREIGN KEY (company_id, binding_channel_id, binding_id) REFERENCES wb(company_id, channel_id, id) ON DELETE CASCADE);
INSERT INTO wb(company_id, id, channel_id) VALUES ('00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000b1',NULL),('00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000b2','00000000-0000-0000-0000-0000000000a1');
\echo 'company binding, claimed company-wide (ok):'
INSERT INTO cs VALUES (gen_random_uuid(),'00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000b1',NULL);
\echo 'channel binding (a1), schedule on a1 (ok):'
INSERT INTO cs VALUES (gen_random_uuid(),'00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000a1','00000000-0000-0000-0000-0000000000b2','00000000-0000-0000-0000-0000000000a1');
SAVEPOINT s3;
\echo 'channel binding (a1), schedule on a2 claiming company-wide (must fail):'
INSERT INTO cs VALUES (gen_random_uuid(),'00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000b2',NULL);
ROLLBACK TO SAVEPOINT s3;
SAVEPOINT s4;
\echo 'channel binding (a1), schedule on a2 claiming a2 (must fail):'
INSERT INTO cs VALUES (gen_random_uuid(),'00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000b2','00000000-0000-0000-0000-0000000000a2');
ROLLBACK TO SAVEPOINT s4;
SAVEPOINT s5;
\echo 'company binding, schedule claiming channel a2 (must fail):'
INSERT INTO cs VALUES (gen_random_uuid(),'00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000a2');
ROLLBACK TO SAVEPOINT s5;
\echo 'delete channel binding cascades its schedule:'
DELETE FROM wb WHERE id='00000000-0000-0000-0000-0000000000b2';
SELECT count(*) AS schedules_left FROM cs;
ROLLBACK;
