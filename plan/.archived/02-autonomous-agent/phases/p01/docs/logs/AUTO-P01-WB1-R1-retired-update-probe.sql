\set ON_ERROR_STOP on
BEGIN;
SELECT gen_random_uuid() AS uid,gen_random_uuid() AS cid,gen_random_uuid() AS aid,gen_random_uuid() AS sid,gen_random_uuid() AS old_assignment,gen_random_uuid() AS fresh_assignment \gset
INSERT INTO users(id,username,email) VALUES(:'uid','review-'||:'uid',:'uid'||'@example.invalid');
INSERT INTO companies(id,user_id,name,slug) VALUES(:'cid',:'uid','Review probe','review-'||:'cid');
INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES(gen_random_uuid(),:'cid',:'uid','owner',9);
INSERT INTO agents(id,company_id,name,slug,created_by) VALUES(:'aid',:'cid','Review probe','review-probe','{"actor_type":"system","actor_id":null,"actor_name":"Review probe"}');
INSERT INTO skills(id,company_id,slug,name,description,instructions,created_by) VALUES(:'sid',:'cid','review-skill','Review skill','Review description','Old instructions','{"actor_type":"system","actor_id":null,"actor_name":"Review probe"}');
INSERT INTO agent_skills(company_id,agent_id,skill_id,assignment_id,position) VALUES(:'cid',:'aid',:'sid',:'old_assignment',0);
DELETE FROM skills WHERE id=:'sid';
SELECT live_skill_id IS NULL AS original_assignment_retired FROM agent_skill_assignments WHERE id=:'old_assignment';
INSERT INTO skills(id,company_id,slug,name,description,instructions,created_by) VALUES(:'sid',:'cid','review-skill','Review skill','Review description','Replacement instructions','{"actor_type":"system","actor_id":null,"actor_name":"Review probe"}');
INSERT INTO agent_skills(company_id,agent_id,skill_id,assignment_id,position) VALUES(:'cid',:'aid',:'sid',:'fresh_assignment',0);
UPDATE agent_skills SET assignment_id=:'old_assignment' WHERE agent_id=:'aid';
SET CONSTRAINTS ALL IMMEDIATE;
SELECT draft.assignment_id=:'old_assignment' AS retired_assignment_reattached, retained.live_skill_id IS NULL AS retained_pointer_still_null
FROM agent_skills AS draft JOIN agent_skill_assignments AS retained ON retained.id=draft.assignment_id WHERE draft.agent_id=:'aid';
ROLLBACK;
SELECT count(*) AS remaining_probe_companies FROM companies WHERE id=:'cid';
