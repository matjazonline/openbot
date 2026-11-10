-- Reviewer B, FIXAPP-P05-BR1: p05's fairness_tests stale-demand fixture shape on the merged
-- (p05 + p01 4d940da) schema: a company INSERT and its owner member INSERT as two autocommit
-- statements, as `execute(p.pool())` runs them.
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_f','owner_f@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c9','00000000-0000-0000-0000-0000000000a1','Stale demand','stale-demand-x');
INSERT INTO company_members(id, company_id, user_id, role) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c9','00000000-0000-0000-0000-0000000000a1','owner');
\echo '--- control: both in one transaction'
BEGIN;
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000ca','00000000-0000-0000-0000-0000000000a1','Stale demand','stale-demand-y');
INSERT INTO company_members(id, company_id, user_id, role) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000ca','00000000-0000-0000-0000-0000000000a1','owner');
COMMIT;
SELECT slug FROM companies ORDER BY slug;
DELETE FROM companies; DELETE FROM users;
