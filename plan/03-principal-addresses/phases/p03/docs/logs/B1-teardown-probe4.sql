BEGIN;
ALTER TABLE threads ADD CONSTRAINT pa03_probe_thread_company_fk FOREIGN KEY(company_id) REFERENCES companies(id) ON DELETE CASCADE;
ALTER TABLE external_threads ALTER CONSTRAINT external_threads_target_principal_fk DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE threads ALTER CONSTRAINT threads_target_principal_fk DEFERRABLE INITIALLY DEFERRED;
CREATE TEMP TABLE pa03_probe_thread AS SELECT gen_random_uuid() AS id;
INSERT INTO threads(id,company_id,principal_id,subject) SELECT id,'6f16cdb7-3bbb-476a-9736-b6f900963267','0ac636d0-1338-4951-888e-a92620851fc8','Rollback-only principal teardown probe' FROM pa03_probe_thread;
INSERT INTO external_threads(id,company_id,principal_id,binding_id,external_thread_key,thread_id)
 SELECT gen_random_uuid(),'6f16cdb7-3bbb-476a-9736-b6f900963267','0ac636d0-1338-4951-888e-a92620851fc8','b9c3a13d-c46a-4756-b9b2-01f393b955f3',gen_random_uuid()::text,id FROM pa03_probe_thread;
DELETE FROM companies WHERE id='6f16cdb7-3bbb-476a-9736-b6f900963267';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT count(*) AS company_remaining FROM companies WHERE id='6f16cdb7-3bbb-476a-9736-b6f900963267';
ROLLBACK;
