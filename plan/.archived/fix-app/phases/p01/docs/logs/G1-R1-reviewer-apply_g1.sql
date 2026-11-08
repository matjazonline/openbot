-- Emulate G1 r1 D2/D3 schema decisions on the scratch DB only.
\set ON_ERROR_STOP 1
ALTER TABLE principals DROP CONSTRAINT principals_company_agent_fk,
  ADD CONSTRAINT principals_company_agent_fk FOREIGN KEY (company_id, agent_id) REFERENCES agents(company_id, id);
ALTER TABLE messages DROP CONSTRAINT messages_author_principal_fk,
  ADD CONSTRAINT messages_author_principal_fk FOREIGN KEY (company_id, author_principal_id) REFERENCES principals(company_id, id);
ALTER TABLE message_participants DROP CONSTRAINT message_participants_identity_fk,
  ADD CONSTRAINT message_participants_identity_fk FOREIGN KEY (company_id, participant_identity_id) REFERENCES participant_identities(company_id, id);
ALTER TABLE companies ADD CONSTRAINT companies_owner_member_fk FOREIGN KEY (id, user_id)
  REFERENCES company_members(company_id, user_id) DEFERRABLE INITIALLY DEFERRED;
CREATE FUNCTION enforce_company_owner_agreement() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE checked uuid;
BEGIN
  IF TG_TABLE_NAME = 'companies' THEN checked := NEW.id;
  ELSIF TG_OP = 'DELETE' THEN checked := OLD.company_id;
  ELSE checked := NEW.company_id;
  END IF;
  IF EXISTS (SELECT 1 FROM companies AS company WHERE company.id = checked)
     AND NOT EXISTS (SELECT 1 FROM companies AS company
                     JOIN company_members AS member
                       ON (member.company_id, member.user_id) = (company.id, company.user_id)
                     WHERE company.id = checked AND member.role = 'owner') THEN
    RAISE EXCEPTION 'owner member row disagrees with companies.user_id'
      USING ERRCODE = '23514', CONSTRAINT = 'companies_owner_member_agreement';
  END IF;
  RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER companies_owner_agreement AFTER INSERT OR UPDATE OF user_id ON companies
  DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_company_owner_agreement();
CREATE CONSTRAINT TRIGGER company_members_owner_agreement
  AFTER INSERT OR DELETE OR UPDATE OF company_id, user_id, role ON company_members
  DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION enforce_company_owner_agreement();
