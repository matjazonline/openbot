-- Reviewer B, FIXAPP-P05-G1-R1: company/thread/channel deletion with an internal note under D4's
-- internal_notes_thread_message_fk (NO ACTION, not deferred). Rolled back.
\set ON_ERROR_STOP 0
BEGIN;
SET LOCAL session_replication_role = replica;
INSERT INTO users(id, username, email, password_hash) VALUES ('00000000-0000-0000-0000-0000000000a1','owner_b','owner_b@example.test','x');
INSERT INTO companies(id, user_id, name, slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','Co','co-b');
INSERT INTO principals(id, company_id, kind, display_label) VALUES ('00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000c1','system','sys');
INSERT INTO channels(id, company_id, name, created_by) VALUES ('00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-0000000000c1','ch2','{"actor_type":"system","actor_id":null,"actor_name":"s"}');
INSERT INTO threads(id, company_id, channel_id, subject) VALUES ('00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','s');
INSERT INTO messages(id, company_id, author_principal_id, subject, clean_text_body, direction, role, correlation_id, content_hash, audience) VALUES ('00000000-0000-0000-0000-000000000021','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','s','b','outbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'),'internal_only');
INSERT INTO thread_messages(id, company_id, channel_id, thread_id, message_id, entry_kind) VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021','note');
INSERT INTO internal_notes(id, company_id, channel_id, thread_id, message_id, command_id, command_fingerprint, author_principal_id, provenance, message_audience)
  VALUES (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000f2','00000000-0000-0000-0000-000000000011','00000000-0000-0000-0000-000000000021',gen_random_uuid(),'f','00000000-0000-0000-0000-0000000000b1','api','internal_only');
SET LOCAL session_replication_role = origin;
ALTER TABLE internal_notes ADD CONSTRAINT internal_notes_thread_message_fk FOREIGN KEY (company_id, channel_id, thread_id, message_id) REFERENCES thread_messages(company_id, channel_id, thread_id, message_id);
\echo '=== company deletion'
SAVEPOINT a; DELETE FROM companies WHERE id='00000000-0000-0000-0000-0000000000c1'; SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK TO SAVEPOINT a;
\echo '=== channel deletion (raw SQL; the app refuses a channel with threads)'
SAVEPOINT b; DELETE FROM channels WHERE id='00000000-0000-0000-0000-0000000000f2'; SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK TO SAVEPOINT b;
\echo '=== thread deletion (raw SQL)'
SAVEPOINT c; DELETE FROM threads WHERE id='00000000-0000-0000-0000-000000000011'; SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK TO SAVEPOINT c;
\echo '=== message deletion stays blocked'
SAVEPOINT d; DELETE FROM messages WHERE id='00000000-0000-0000-0000-000000000021'; SET CONSTRAINTS ALL IMMEDIATE; ROLLBACK TO SAVEPOINT d;
ROLLBACK;
