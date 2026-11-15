\set ON_ERROR_STOP on
BEGIN;
CREATE TEMP TABLE review_accounts(id integer PRIMARY KEY);
CREATE TEMP TABLE review_members(tenant integer, user_id integer REFERENCES review_accounts(id) ON DELETE CASCADE, PRIMARY KEY(tenant,user_id));
CREATE TEMP TABLE review_receipts(id integer PRIMARY KEY, tenant integer NOT NULL, recipient_user_id integer NOT NULL REFERENCES review_accounts(id) ON DELETE CASCADE, source_kind text NOT NULL, read_at integer, member_recipient_user_id integer GENERATED ALWAYS AS (CASE WHEN source_kind='direct_message' THEN NULL ELSE recipient_user_id END) STORED, FOREIGN KEY(tenant,member_recipient_user_id) REFERENCES review_members(tenant,user_id) ON DELETE CASCADE);
INSERT INTO review_accounts VALUES(1);
INSERT INTO review_members VALUES(10,1);
INSERT INTO review_receipts(id,tenant,recipient_user_id,source_kind,read_at) VALUES(1,10,1,'direct_message',42),(2,10,1,'delivery',43),(3,10,1,'decision_reviewer',44),(4,10,1,'escalation_notice',45);
DELETE FROM review_members WHERE user_id=1;
DO $$ BEGIN
IF (SELECT count(*) FROM review_receipts) <> 1 OR NOT EXISTS(SELECT 1 FROM review_receipts WHERE id=1 AND read_at=42 AND member_recipient_user_id IS NULL) THEN RAISE EXCEPTION 'retention/cascade assertion failed'; END IF;
END $$;
INSERT INTO review_members VALUES(10,1);
DO $$ BEGIN
IF (SELECT count(*) FROM review_receipts) <> 1 OR NOT EXISTS(SELECT 1 FROM review_receipts WHERE id=1 AND read_at=42 AND member_recipient_user_id IS NULL) THEN RAISE EXCEPTION 'rejoin mutated retained row'; END IF;
BEGIN
UPDATE review_receipts SET member_recipient_user_id=1 WHERE id=1;
RAISE EXCEPTION 'generated key unexpectedly caller-writable';
EXCEPTION WHEN generated_always THEN NULL;
END;
END $$;
DELETE FROM review_accounts WHERE id=1;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM review_receipts) THEN RAISE EXCEPTION 'account delete did not cascade'; END IF; END $$;
ROLLBACK;
SELECT 'PASS: generated FK child retains direct row/read state, cascades other families, rejects caller override, account deletion cascades' AS review_result;
