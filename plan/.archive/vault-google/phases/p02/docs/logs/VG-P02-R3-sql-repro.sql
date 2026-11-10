BEGIN;
INSERT INTO integration_accounts(id,owner_namespace,owner_subject,key,provider_id,auth_config_id,auth_method,credential_schema,envelope,status,metadata)
VALUES(gen_random_uuid(),'review-vg-p02','missing-label','missing-label','static-bearer','static-bearer','static_secret','static-v1','enc:v2:fixture','ready','{}')
RETURNING metadata;
INSERT INTO integration_accounts(id,owner_namespace,owner_subject,key,provider_id,auth_config_id,auth_method,credential_schema,external_identity,authorization_key,envelope,status,metadata)
VALUES(gen_random_uuid(),'review-vg-p02','null-identity-key','null-key','google','google-production','oauth_authorization_code','google-v1','{"kind":"google-subject-client","subject":"fixture","authorization_key":null}','canonical-fixture','enc:v2:fixture','ready','{"label":"fixture"}')
RETURNING external_identity,authorization_key;
ROLLBACK;
