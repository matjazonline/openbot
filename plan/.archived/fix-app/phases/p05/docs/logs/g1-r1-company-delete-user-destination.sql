-- Reviewer B, FIXAPP-P05-G1-R1: company deletion with a user-kind workflow destination (with an
-- email delivery), after D4's thread_messages FK replaces the message/thread FKs. Rolled back.
\set ON_ERROR_STOP 0
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO principals(id, company_id, kind, display_label) VALUES ('00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000c1','system','sys');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-0000000000c1','ch2','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO threads(id, company_id, channel_id, subject) VALUES ('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','s');
INSERT INTO messages(id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) VALUES ('00000000-0000-0000-0000-000000000021','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','agent',gen_random_uuid(),decode(repeat('00',32),'hex'),'external_conversation');
INSERT INTO thread_messages(id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','conversation');
INSERT INTO message_deliveries(id, company_id, channel_id, message_id, source_binding_id, destination_binding_id, correlation_id, transport, purpose, idempotency_key, max_attempts, message_audience)
  VALUES ('00000000-0000-0000-0000-0000000000d1','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000021',gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'email','workflow_send','k',3,'external_conversation');
INSERT INTO workflow_runs(company_id,id,binding_id,binding_revision,workflow_id,version_id,actor_id,trigger_id,correlation_id,bundle,input,params,resources,max_steps,max_context_bytes,deadline)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),1,gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),gen_random_uuid(),'\x7b7d','{}','{}','{}',10,1000,now()+interval '1 day');
INSERT INTO workflow_message_destinations(company_id, run_id, invocation_id, ordinal, kind, destination_id, thread_id, message_id, delivery_id, principal_id, recipient_address)
  VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000e1',gen_random_uuid(),0,'user','00000000-0000-0000-0000-0000000000a1','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','00000000-0000-0000-0000-0000000000d1','00000000-0000-0000-0000-0000000000b1','u@example.test');
ALTER TABLE workflow_message_destinations ADD COLUMN channel_id uuid;
UPDATE workflow_message_destinations SET channel_id='00000000-0000-0000-0000-0000000000f2';
SET LOCAL session_replication_role = origin;
ALTER TABLE workflow_message_destinations DROP CONSTRAINT workflow_message_destination_message, DROP CONSTRAINT workflow_message_destination_thread;
ALTER TABLE workflow_message_destinations ADD CONSTRAINT d4_thread_message FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id);
\echo '=== D4 thread_messages FK in place; pre-existing principal + delivery FKs unchanged'
SAVEPOINT a;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT a;
\echo '=== same, delivery FK dropped (principal FK remains)'
ALTER TABLE workflow_message_destinations DROP CONSTRAINT workflow_message_destination_delivery;
SAVEPOINT b;
DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1';
ROLLBACK TO SAVEPOINT b;
ROLLBACK;
