-- Company membership participates in channel authorization. Reconcile recipient records when a
-- membership is removed or its role changes so revoked alerts are withdrawn immediately, not only
-- when the user next follows the deep link.
CREATE FUNCTION notification_recheck_membership() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    changed_company UUID := COALESCE(NEW.company_id, OLD.company_id);
    changed_user UUID := COALESCE(NEW.user_id, OLD.user_id);
BEGIN
    UPDATE notifications AS notification
       SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
           updated_at = CURRENT_TIMESTAMP
     WHERE notification.company_id = changed_company
       AND notification.recipient_user_id = changed_user
       AND notification.state = 'active'
       AND (
           notification.recipient_principal_id IS NULL
           OR NOT notification_principal_can_view(
               notification.company_id,
               notification.channel_id,
               notification.recipient_principal_id
           )
       );
    RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE TRIGGER notification_membership_recheck
AFTER UPDATE OF role OR DELETE ON company_members
FOR EACH ROW EXECUTE FUNCTION notification_recheck_membership();
