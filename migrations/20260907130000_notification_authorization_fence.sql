-- Channel policy and grant transitions serialize with notification recipient resolution through
-- the channel row. This turns the projector's authorization re-check into a commit-order guarantee
-- instead of a point-in-time observation that a concurrent grant deletion could overtake.
CREATE OR REPLACE FUNCTION notification_recheck_channel_access() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    row_data JSONB := CASE WHEN TG_OP = 'DELETE' THEN to_jsonb(OLD) ELSE to_jsonb(NEW) END;
    changed_company UUID := (row_data->>'company_id')::UUID;
    changed_channel UUID := COALESCE(
        (row_data->>'channel_id')::UUID,
        (row_data->>'id')::UUID
    );
BEGIN
    PERFORM 1 FROM channels AS channel
     WHERE channel.company_id = changed_company AND channel.id = changed_channel
     FOR UPDATE;
    PERFORM withdraw_unauthorized_notifications(changed_company, changed_channel);
    RETURN COALESCE(NEW, OLD);
END;
$$;
