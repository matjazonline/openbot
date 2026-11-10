\set VERBOSITY verbose
\echo '=== S1 D2: autocommit demote of owner row (expect 23514 companies_owner_member_agreement)'
UPDATE company_members SET role='member' WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND user_id='00000000-0000-0000-0000-0000000000a1';
\echo '=== S2 D2: autocommit change of companies.user_id alone to member u2 (expect 23514)'
UPDATE companies SET user_id='00000000-0000-0000-0000-0000000000a2' WHERE id='00000000-0000-0000-0000-0000000000c1';
\echo '=== S3 D2: companies.user_id to non-member u3 (expect 23503 companies_owner_member_fk or 23514)'
UPDATE companies SET user_id='00000000-0000-0000-0000-0000000000a3' WHERE id='00000000-0000-0000-0000-0000000000c1';
\echo '=== S4 D2: delete owner member row (expect 23503 companies_owner_member_fk)'
DELETE FROM company_members WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND user_id='00000000-0000-0000-0000-0000000000a1';
\echo '=== S5 D2: second owner row insert (expect 23505 company_members_one_owner_idx)'
UPDATE company_members SET role='owner' WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND user_id='00000000-0000-0000-0000-0000000000a2';
\echo '=== S6 D5: invite acceptance upsert for owner with WHERE role<>owner leaves owner'
INSERT INTO company_members(id,company_id,user_id,role) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','member')
  ON CONFLICT (company_id,user_id) DO UPDATE SET role = EXCLUDED.role WHERE company_members.role <> 'owner' RETURNING company_id;
SELECT role FROM company_members WHERE user_id='00000000-0000-0000-0000-0000000000a1';
\echo '=== S7 D2: owner moved to another company via UPDATE OF company_id (expect 23503 via FK on old company)'
UPDATE company_members SET company_id = company_id WHERE false;
\echo '=== S8 D1 forward: separate statements (expect 23503 messages_authored_identity_author_fk)'
BEGIN;
INSERT INTO principals(id,company_id,kind,display_label) VALUES ('00000000-0000-0000-0000-0000000000b9','00000000-0000-0000-0000-0000000000c1','external','old@example.com');
UPDATE participant_identities SET principal_id='00000000-0000-0000-0000-0000000000b9' WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND id='00000000-0000-0000-0000-0000000000d1';
ROLLBACK;
\echo '=== S9 D1 forward: one data-modifying CTE (expect success)'
BEGIN;
INSERT INTO principals(id,company_id,kind,display_label) VALUES ('00000000-0000-0000-0000-0000000000b9','00000000-0000-0000-0000-0000000000c1','external','old@example.com');
WITH moved AS (
  UPDATE participant_identities SET principal_id='00000000-0000-0000-0000-0000000000b9', status='observed',
         provenance='transport_ingress', claim_metadata='{"kind":"observation","version":1}', display_label=NULL
   WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND id='00000000-0000-0000-0000-0000000000d1'
     AND principal_id='00000000-0000-0000-0000-0000000000b1' RETURNING id),
 authored AS (
  UPDATE messages SET author_principal_id='00000000-0000-0000-0000-0000000000b9'
   WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND authored_identity_id IN (SELECT id FROM moved)
     AND author_principal_id='00000000-0000-0000-0000-0000000000b1' RETURNING id)
SELECT (SELECT count(*) FROM moved) AS moved, (SELECT count(*) FROM authored) AS authored;
\echo '--- S9b D1 reverse: adopt outsider new@ into person b1 (ON CONFLICT upsert + authorship move in one statement)'
WITH upserted AS (
  INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,display_label,status,claim_metadata,provenance)
  VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','email','default','new@example.com','u1','verified','{"kind":"account","version":1}','account')
  ON CONFLICT (company_id,transport,namespace,subject) DO UPDATE SET principal_id=EXCLUDED.principal_id, display_label=EXCLUDED.display_label,
     status='verified', claim_metadata=EXCLUDED.claim_metadata, provenance='account', updated_at=CURRENT_TIMESTAMP
  RETURNING id),
 authored AS (
  UPDATE messages SET author_principal_id='00000000-0000-0000-0000-0000000000b1'
   WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND authored_identity_id IN (SELECT id FROM upserted)
     AND author_principal_id='00000000-0000-0000-0000-0000000000bf' RETURNING id)
SELECT (SELECT count(*) FROM upserted) AS upserted, (SELECT count(*) FROM authored) AS authored;
SELECT id, author_principal_id, authored_identity_id FROM messages ORDER BY id;
SELECT id, principal_id, subject, status, provenance FROM participant_identities ORDER BY id;
COMMIT;
\echo '=== S10 D4: plain agent delete with principal (expect 23503 principals_company_agent_fk)'
DELETE FROM agents WHERE id='00000000-0000-0000-0000-0000000000e1';
\echo '=== S11 D4: retire (lock, demote, delete) in one tx (expect success; m2 kept, author external)'
BEGIN;
SELECT company_id FROM agents WHERE id='00000000-0000-0000-0000-0000000000e1' FOR UPDATE;
UPDATE principals SET kind='external', agent_id=NULL, updated_at=now() WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND agent_id='00000000-0000-0000-0000-0000000000e1';
DELETE FROM agents WHERE id='00000000-0000-0000-0000-0000000000e1';
COMMIT;
SELECT m.id, p.kind FROM messages m JOIN principals p ON (p.company_id,p.id)=(m.company_id,m.author_principal_id) ORDER BY m.id;
\echo '=== S12 D3: delete a principal that authored messages (expect 23503 messages_author_principal_fk)'
DELETE FROM principals WHERE id='00000000-0000-0000-0000-0000000000b9';
\echo '=== S13 D3: delete principal whose identity is only a participant (bf now has no identity; b2 identity d2 unused) -> b2 delete cascades member? delete member row u2'
DELETE FROM company_members WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND user_id='00000000-0000-0000-0000-0000000000a2';
\echo '=== S14 D2 transfer helper in one tx: ensure u3 member, demote u1, set user_id, promote u3 (expect success)'
BEGIN;
INSERT INTO company_members(id,company_id,user_id,role) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a3','member') ON CONFLICT (company_id,user_id) DO NOTHING;
UPDATE company_members SET role='member' WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND role='owner';
UPDATE companies SET user_id='00000000-0000-0000-0000-0000000000a3' WHERE id='00000000-0000-0000-0000-0000000000c1';
UPDATE company_members SET role='owner' WHERE company_id='00000000-0000-0000-0000-0000000000c1' AND user_id='00000000-0000-0000-0000-0000000000a3';
COMMIT;
SELECT user_id, role FROM company_members ORDER BY role, user_id;
\echo '=== S15 company delete with retired agent principal, persons, externals, messages, participants (expect success, all gone)'
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
SELECT (SELECT count(*) FROM principals) p, (SELECT count(*) FROM participant_identities) i, (SELECT count(*) FROM messages) m,
       (SELECT count(*) FROM message_participants) mp, (SELECT count(*) FROM company_members) cm, (SELECT count(*) FROM agents) a;
