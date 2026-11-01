BEGIN;
CREATE FUNCTION g1_case_probe() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE checked uuid := CASE WHEN TG_TABLE_NAME = 'companies' THEN NEW.id
                             ELSE COALESCE(NEW.company_id, OLD.company_id) END;
BEGIN RETURN NULL; END $$;
CREATE TRIGGER g1_case_probe AFTER INSERT ON companies FOR EACH ROW EXECUTE FUNCTION g1_case_probe();
INSERT INTO users(id,username,email,password_hash) VALUES ('00000000-0000-0000-0000-0000000000a9','probe','probe@example.com','x');
INSERT INTO companies(id,user_id,name,slug) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000a9','P','probe');
ROLLBACK;
