-- Fixture: users u1 (owner), u2 (member); company c1; agent a1 with principal pa; person principals p1, p2;
-- external x1; identities; messages authored by person-with-identity, agent, external; participants.
\set ON_ERROR_STOP 1
BEGIN;
INSERT INTO users(id,username,email,password_hash) VALUES
 ('00000000-0000-0000-0000-0000000000a1','u1','old@example.com','x'),
 ('00000000-0000-0000-0000-0000000000a2','u2','u2@example.com','x'),
 ('00000000-0000-0000-0000-0000000000a3','u3','u3@example.com','x');
INSERT INTO companies(id,user_id,name,slug) VALUES ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','C1','c1');
INSERT INTO company_members(id,company_id,user_id,role) VALUES
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a1','owner'),
 (gen_random_uuid(),'00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000a2','member');
INSERT INTO agents(id,company_id,name,slug,created_by) VALUES
 ('00000000-0000-0000-0000-0000000000e1','00000000-0000-0000-0000-0000000000c1','Agent','agent','{"actor_type":"system","actor_id":null,"actor_name":"sys"}');
INSERT INTO principals(id,company_id,kind,user_id,agent_id,display_label) VALUES
 ('00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000c1','person','00000000-0000-0000-0000-0000000000a1',NULL,'u1'),
 ('00000000-0000-0000-0000-0000000000b2','00000000-0000-0000-0000-0000000000c1','person','00000000-0000-0000-0000-0000000000a2',NULL,'u2'),
 ('00000000-0000-0000-0000-0000000000be','00000000-0000-0000-0000-0000000000c1','agent',NULL,'00000000-0000-0000-0000-0000000000e1','Agent'),
 ('00000000-0000-0000-0000-0000000000bf','00000000-0000-0000-0000-0000000000c1','external',NULL,NULL,'new@example.com');
INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,status,claim_metadata,provenance) VALUES
 ('00000000-0000-0000-0000-0000000000d1','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','email','default','old@example.com','verified','{"kind":"account","version":1}','account'),
 ('00000000-0000-0000-0000-0000000000d2','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b2','email','default','u2@example.com','verified','{"kind":"account","version":1}','account'),
 ('00000000-0000-0000-0000-0000000000df','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000bf','email','default','new@example.com','observed','{"kind":"observation","version":1}','transport_ingress');
-- m1: u1 by email from old@ ; m2: agent ; m3: outsider new@ ; m4: u1 in UI (no identity)
INSERT INTO messages(id,company_id,author_principal_id,authored_identity_id,subject,clean_text_body,direction,role,correlation_id,content_hash) VALUES
 ('00000000-0000-0000-0000-000000000f01','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1','00000000-0000-0000-0000-0000000000d1','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex')),
 ('00000000-0000-0000-0000-000000000f02','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000be',NULL,'s','b','outbound','agent',gen_random_uuid(),decode(repeat('00',32),'hex')),
 ('00000000-0000-0000-0000-000000000f03','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000bf','00000000-0000-0000-0000-0000000000df','s','b','inbound','human',gen_random_uuid(),decode(repeat('00',32),'hex')),
 ('00000000-0000-0000-0000-000000000f04','00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-0000000000b1',NULL,'s','b','outbound','human',gen_random_uuid(),decode(repeat('00',32),'hex'));
INSERT INTO message_participants(company_id,message_id,participant_identity_id,kind,position) VALUES
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-000000000f01','00000000-0000-0000-0000-0000000000d1','sender',0),
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-000000000f02','00000000-0000-0000-0000-0000000000d1','to',0),
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-000000000f02','00000000-0000-0000-0000-0000000000df','cc',0),
 ('00000000-0000-0000-0000-0000000000c1','00000000-0000-0000-0000-000000000f03','00000000-0000-0000-0000-0000000000df','sender',0);
COMMIT;
