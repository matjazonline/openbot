ALTER TABLE human_approvals
    ADD COLUMN approver_principal_id UUID;

-- Existing approvals named only an address. Snapshot the corresponding verified person when that
-- address belongs to an account in the same company; external approvers deliberately remain NULL.
UPDATE human_approvals AS approval
SET approver_principal_id = (
    SELECT principal.id
    FROM participant_identities AS identity
    JOIN principals AS principal
      ON principal.company_id = identity.company_id
     AND principal.id = identity.principal_id
    WHERE identity.company_id = approval.company_id
      AND identity.transport = 'email'
      AND identity.status = 'verified'
      AND LOWER(identity.subject) = LOWER(approval.approver_email::text)
      AND principal.kind = 'person'
    ORDER BY identity.created_at, identity.id
    LIMIT 1
);

ALTER TABLE human_approvals
    ADD CONSTRAINT human_approvals_approver_principal_fk
    FOREIGN KEY (company_id, approver_principal_id)
    REFERENCES principals(company_id, id)
    ON DELETE SET NULL (approver_principal_id)
    DEFERRABLE INITIALLY DEFERRED;

CREATE TRIGGER human_approvals_notify_attention
AFTER INSERT OR DELETE OR UPDATE OF status, approver_principal_id, expires_at
ON human_approvals
FOR EACH ROW
EXECUTE FUNCTION notify_attention_changed('approval');
