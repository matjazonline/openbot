//! The real projector reaches the native receipt boundary under a live, valid claim.
use super::*;

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct CanonicalEffects {
    messages: i64,
    intents: i64,
    receipts: i64,
    targets: i64,
}

#[tokio::test]
async fn workflow_escalation_agent_native_receipt_rejects_forged_fence_route_and_principal() {
    use crate::adapters::persistence::test_support::{
        minted_agent_notice_key, minted_agent_notice_recipient,
    };
    use crate::application::notification::{NotificationPersistence, NotificationProjectionCommit};
    for mutation in [
        "NEW.projection_execution_id=gen_random_uuid()",
        "NEW.projection_execution_id=NULL",
        "NEW.projection_worker_id=gen_random_uuid()",
        "NEW.projection_worker_id=NULL",
        "NEW.event_id=gen_random_uuid()",
        "NEW.binding_id=gen_random_uuid()",
        "NEW.binding_revision=NEW.binding_revision+1",
        "NEW.agent_id=gen_random_uuid()",
        "NEW.principal_id=gen_random_uuid()",
    ] {
        let f = Box::pin(pending_agent()).await;
        let p = f.people.role.p();
        // Sequence advancement survives rollback and proves this exact boundary was exercised.
        sqlx::raw_sql(&format!(
            "CREATE SEQUENCE native_receipt_probe; \
             CREATE FUNCTION tamper_native_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
               PERFORM nextval('native_receipt_probe'); {mutation}; RETURN NEW; END $$; \
             CREATE TRIGGER aa_tamper_native_receipt BEFORE INSERT ON workflow_escalation_agent_receipts \
               FOR EACH ROW EXECUTE FUNCTION tamper_native_receipt();"
        ))
        .execute(p.pool())
        .await
        .unwrap();
        let claim = p
            .claim_notification_events(
                crate::transport::WorkerId::random(),
                std::time::Duration::from_secs(30),
                1,
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        let expected = p
            .resolve_notification_projection(&claim.event)
            .await
            .unwrap();
        let key = minted_agent_notice_key(&expected);
        let recipient = minted_agent_notice_recipient(&expected);
        let error = p
            .commit_notification_projection(NotificationProjectionCommit {
                fence: &claim.lease,
                expected: &expected,
                email: None,
                agent_message_key: key.as_ref(),
                agent_recipient: recipient.as_ref(),
                escalation_ack_hours: Default::default(),
            })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("agent notice origin must prove"),
            "{mutation}: {error}"
        );
        let reached: bool = sqlx::query_scalar("SELECT is_called FROM native_receipt_probe")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert!(
            reached,
            "{mutation} did not reach the native receipt boundary"
        );
        let effects: CanonicalEffects = sqlx::query_as(
            "SELECT (SELECT count(*) FROM messages) AS messages, (SELECT count(*) FROM workflow_message_intents) AS intents, \
             (SELECT count(*) FROM workflow_escalation_agent_receipts) AS receipts, \
             (SELECT count(*) FROM workflow_escalation_notice_targets) AS targets",
        ).fetch_one(p.pool()).await.unwrap();
        assert_eq!(
            effects,
            CanonicalEffects {
                messages: 0,
                intents: 0,
                receipts: 0,
                targets: 0
            },
            "{mutation} must roll back every private canonical effect"
        );
    }
}
