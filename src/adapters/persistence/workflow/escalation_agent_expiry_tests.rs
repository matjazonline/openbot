use super::*;
use crate::application::workflow::message_admission::{
    ClaimedIntent, IntentDeferral, MAX_INTENT_ATTEMPTS,
};

async fn last_expired(f: &AgentCase) -> ClaimedIntent {
    let p = f.people.role.p();
    for attempt in 1..=MAX_INTENT_ATTEMPTS {
        let mut selected = claims(f).await;
        assert_eq!(selected.len(), 1);
        let claim = selected.pop().unwrap();
        assert_eq!(claim.attempts, attempt);
        if attempt == MAX_INTENT_ATTEMPTS {
            sqlx::query("UPDATE workflow_message_intents SET lease_expires_at=clock_timestamp()-INTERVAL '1 second' WHERE company_id=$1 AND id=$2")
                .bind(claim.claim.company.as_uuid()).bind(claim.claim.intent).execute(p.pool()).await.unwrap();
            return claim;
        }
        assert!(
            p.defer_intent(claim.claim, IntentDeferral::Transient)
                .await
                .unwrap()
        );
        sqlx::query("UPDATE workflow_message_intents SET available_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(claim.claim.company.as_uuid()).bind(claim.claim.intent).execute(p.pool()).await.unwrap();
    }
    unreachable!("the existing attempt cap is positive")
}

async fn failures(f: &AgentCase) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1 AND failure_kind='handling' AND reason_code='escalation.notification_handling_exhausted'")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_escalation_agent_competing_expiry_owners_record_one_exhausted_handling_failure() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let expired = last_expired(&f).await;
    let (a, b) = tokio::join!(p.fail_expired_intents(32), p.fail_expired_intents(32));
    let ended: Vec<_> = a.unwrap().into_iter().chain(b.unwrap()).collect();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].intent, expired.claim.intent);
    assert_eq!(failures(&f).await, 1);
    assert!(p.fail_expired_intents(32).await.unwrap().is_empty());
    assert!(
        claims(&f).await.is_empty(),
        "the exhausted notice intent is never immediately reclaimed"
    );
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_escalation_progress WHERE company_id=$1")
            .bind(f.people.role.company().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(state, "owner_waiting");
    assert_eq!(facts(&f).await["cases"], 1);
    assert_eq!(facts(&f).await["runs"], 0);
}

#[tokio::test]
async fn workflow_escalation_agent_expiry_keeps_one_bounded_mixed_page_and_ordinary_outcomes() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let private = last_expired(&f).await;
    ordinary(&f).await;
    let normal = last_expired(&f).await;
    let ended = p.fail_expired_intents(2).await.unwrap();
    let actual: std::collections::BTreeSet<_> = ended.iter().map(|row| row.intent).collect();
    assert_eq!(
        actual,
        [private.claim.intent, normal.claim.intent]
            .into_iter()
            .collect()
    );
    assert_eq!(
        failures(&f).await,
        1,
        "ordinary expiry does not acquire an escalation source"
    );
    assert!(p.fail_expired_intents(2).await.unwrap().is_empty());
    assert!(claims(&f).await.is_empty());
    assert_eq!(facts(&f).await["runs"], 0);
}
