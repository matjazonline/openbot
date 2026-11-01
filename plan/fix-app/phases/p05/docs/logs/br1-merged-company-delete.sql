-- Reviewer B, FIXAPP-P05-BR1: p05 worktree schema (digest 2a3cb838) + p01 4d940da migration hunks,
-- applied to a scratch DB. One company holding p01's history paths (person, agent and system
-- principals authoring messages; a participant identity on a message; owner member row) and p05's
-- rows (reviewer notice, notification with email notice, destinations of all three kinds, internal
-- note). Rows inserted with triggers off; DELETE with triggers on; SET CONSTRAINTS ALL IMMEDIATE
-- emulates COMMIT. Everything rolled back.
\set ON_ERROR_STOP 0
\set c '''00000000-0000-0000-0000-0000000000c1'''
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x'),('00000000-0000-0000-0000-0000000000a2','member_b','member_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES (:c,'00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO company_members(id, company_id, user_id, role) VALUES (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000a1','owner'),(gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000a2','member');
INSERT INTO agents(id, company_id, name, slug, created_by) VALUES ('00000000-0000-0000-0000-0000000000a7',:c,'Agent','agent','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO principals(id, company_id, kind, user_id, agent_id, display_label) VALUES
 ('00000000-0000-0000-0000-0000000000b1',:c,'system',NULL,NULL,'sys'),
 ('00000000-0000-0000-0000-0000000000b0',:c,'person','00000000-0000-0000-0000-0000000000a1',NULL,'owner'),
 ('00000000-0000-0000-0000-0000000000b2',:c,'person','00000000-0000-0000-0000-0000000000a2',NULL,'member'),
 ('00000000-0000-0000-0000-0000000000b3',:c,'agent',NULL,'00000000-0000-0000-0000-0000000000a7','agent');
INSERT INTO participant_identities(id, company_id, principal_id, transport, namespace, subject, status, claim_metadata, provenance)
  VALUES ('00000000-0000-0000-0000-0000000000a8',:c,'00000000-0000-0000-0000-0000000000b2','email','email','member_b@example.test','verified','{"version":1,"kind":"account"}','account');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f1',:c,'run','{"actor_type":"system","actor_id":null,"actor_name":"s"}'),('00000000-0000-0000-0000-0000000000f2',:c,'dest','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO threads(id, company_id, channel_id, subject) VALUES ('00000000-0000-0000-0000-000000000011',:c,'00000000-0000-0000-0000-0000000000f2','s'),('00000000-0000-0000-0000-000000000012',:c,'00000000-0000-0000-0000-0000000000f1','s'),('00000000-0000-0000-0000-000000000013',:c,'00000000-0000-0000-0000-0000000000f1','s');
-- m1 channel post by the agent, m2 user email by system, m3 inbound by the member, m4 reply by the agent, m5 note by the member
INSERT INTO messages(id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) VALUES
 ('00000000-0000-0000-0000-000000000021',:c,'00000000-0000-0000-0000-0000000000b3','s','b','outbound','agent',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000022',:c,'00000000-0000-0000-0000-0000000000b1','s','b','outbound','system',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000023',:c,'00000000-0000-0000-0000-0000000000b2','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000024',:c,'00000000-0000-0000-0000-0000000000b3','s','b','outbound','agent',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation'),
 ('00000000-0000-0000-0000-000000000025',:c,'00000000-0000-0000-0000-0000000000b2','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),'internal_only');
INSERT INTO message_participants(company_id, message_id, participant_identity_id, kind, "position") VALUES (:c,'00000000-0000-0000-0000-000000000023','00000000-0000-0000-0000-0000000000a8','sender',0);
INSERT INTO thread_messages(id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES
 (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','conversation'),
 (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000022','conversation'),
 (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000023','conversation'),
 (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000024','conversation'),
 (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000025','note');
INSERT INTO internal_notes(id, company_id, channel_id, thread_id, message_id, command_id, command_fingerprint, author_principal_id, provenance, message_audience)
  VALUES (gen_random_uuid(),:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000025',gen_random_uuid(),'f','00000000-0000-0000-0000-0000000000b2','api','internal_only');
INSERT INTO message_deliveries(id, company_id, channel_id, message_id, source_binding_id, destination_binding_id, correlation_id, transport, purpose, idempotency_key, max_attempts, message_audience) VALUES
 ('00000000-0000-0000-0000-0000000000d2',:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000022',gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'email','workflow_send','k2',3,'external_conversation'),
 ('00000000-0000-0000-0000-0000000000d4',:c,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000024',gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'email','workflow_reply','k4',3,'external_conversation');
INSERT INTO message_deliveries(id, company_id, external_destination, correlation_id, transport, purpose, idempotency_key, max_attempts) VALUES
 ('00000000-0000-0000-0000-0000000000dd',:c,'member_b@example.test',gen_random_uuid(),'email','notification','notification:workflow-decision:x',3),
 ('00000000-0000-0000-0000-0000000000de',:c,'member_b@example.test',gen_random_uuid(),'email','notification','notification:delivery-failure:y',3);
INSERT INTO workflow_runs(company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline,channel_id,state,waiting_reason)
  VALUES (:c,'00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),1,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'\x7b7d','{}','{}','{}',10,1000,now()+interval '1 day','00000000-0000-0000-0000-0000000000f1','waiting','decision');
INSERT INTO workflow_executions(company_id,run_id,id,step_id,activation) VALUES (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','start',1);
INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline) VALUES (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e3','decision',now()+interval '1 day');
INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest) VALUES (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000e2','00000000-0000-0000-0000-0000000000e4','00000000-0000-0000-0000-0000000000e3','business',repeat('a',64));
INSERT INTO workflow_decision_reviewers(company_id,user_id,decision_id,ordinal,principal_id,notice_delivery_id) VALUES (:c,'00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000e4',0,'00000000-0000-0000-0000-0000000000b2','00000000-0000-0000-0000-0000000000dd');
INSERT INTO workflow_message_destinations(company_id, run_id, invocation_id, ordinal, kind, destination_channel_id, destination_user_id, replied_message_id, channel_id, thread_id, message_id, delivery_id, principal_id, recipient_address) VALUES
 (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000a9',0,'channel','00000000-0000-0000-0000-0000000000f2',NULL,NULL,'00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021',NULL,NULL,NULL),
 (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000a9',1,'user',NULL,'00000000-0000-0000-0000-0000000000a2',NULL,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000012','00000000-0000-0000-0000-000000000022','00000000-0000-0000-0000-0000000000d2','00000000-0000-0000-0000-0000000000b2','member_b@example.test'),
 (:c,'00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000aa',0,'reply',NULL,NULL,'00000000-0000-0000-0000-000000000023','00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-000000000013','00000000-0000-0000-0000-000000000024','00000000-0000-0000-0000-0000000000d4',NULL,'s@example.test');
INSERT INTO notification_events(id, company_id, source_kind, source_id, action_kind, source_generation) VALUES ('00000000-0000-0000-0000-0000000000ab',:c,'delivery',gen_random_uuid(),'delivery_failure',1);
INSERT INTO notifications(company_id, recipient_user_id, recipient_principal_id, event_id, source_kind, source_id, action_kind, source_generation, channel_id, email_delivery_id)
  SELECT company_id,'00000000-0000-0000-0000-0000000000a2','00000000-0000-0000-0000-0000000000b2',id,source_kind,source_id,action_kind,source_generation,'00000000-0000-0000-0000-0000000000f1','00000000-0000-0000-0000-0000000000de' FROM notification_events WHERE id='00000000-0000-0000-0000-0000000000ab';
SET LOCAL session_replication_role = origin;
SELECT 'fixture', (SELECT count(*) FROM messages) AS messages, (SELECT count(*) FROM principals) AS principals, (SELECT count(*) FROM message_participants) AS participants, (SELECT count(*) FROM message_deliveries) AS deliveries, (SELECT count(*) FROM workflow_message_destinations) AS destinations, (SELECT count(*) FROM notifications) AS notifications;

\echo '=== A. merged schema: whole-company delete, commit emulated'
SAVEPOINT a;
DELETE FROM companies WHERE id=:c;
SET CONSTRAINTS ALL IMMEDIATE;
SELECT 'A left', (SELECT count(*) FROM messages) AS messages, (SELECT count(*) FROM principals) AS principals, (SELECT count(*) FROM message_participants) AS participants, (SELECT count(*) FROM message_deliveries) AS deliveries, (SELECT count(*) FROM workflow_message_destinations) AS destinations, (SELECT count(*) FROM workflow_decision_reviewers) AS reviewers, (SELECT count(*) FROM internal_notes) AS notes, (SELECT count(*) FROM notifications) AS notifications;
ROLLBACK TO SAVEPOINT a;

\echo '=== B. merged schema: p05 remove_member sequence for the member (demote, lock, delete), commit emulated'
SAVEPOINT b;
UPDATE principals SET kind='external', user_id=NULL WHERE company_id=:c AND user_id='00000000-0000-0000-0000-0000000000a2'
  AND EXISTS (SELECT 1 FROM company_members WHERE company_id=:c AND user_id='00000000-0000-0000-0000-0000000000a2' AND role<>'owner');
SELECT id FROM company_members WHERE company_id=:c AND user_id='00000000-0000-0000-0000-0000000000a2' AND role<>'owner' FOR UPDATE;
DELETE FROM company_members WHERE company_id=:c AND user_id='00000000-0000-0000-0000-0000000000a2' AND role<>'owner';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT 'B left', (SELECT count(*) FROM messages) AS messages, (SELECT kind FROM principals WHERE id='00000000-0000-0000-0000-0000000000b2') AS member_principal, (SELECT count(*) FROM notifications) AS notifications, (SELECT count(*) FROM workflow_decision_reviewers) AS reviewers, (SELECT count(*) FROM workflow_message_destinations) AS destinations, (SELECT count(*) FROM message_participants) AS participants;
ROLLBACK TO SAVEPOINT b;
ROLLBACK;
