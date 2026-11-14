//! Actual worker attempts at the two nonblocking authority boundaries. Full D3
//! role-writer and three-claimant schedules remain distinct acceptance obligations.
use super::*;
use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gates_on,
};
use crate::adapters::persistence::test_support::minted_agent_notice_key;
use crate::application::notification::{
    NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
};
use crate::transport::WorkerId;
use std::time::Duration;

#[tokio::test]
async fn workflow_escalation_agent_company_nowait_defers_once_and_releases_its_gate() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let mut company_writer = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.people.role.company().as_uuid())
        .execute(&mut *company_writer)
        .await
        .unwrap();
    let worker = notification_worker(p, Default::default());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), worker.drain_once())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(
        notice_queue(&f).await,
        json!({"status":"pending","attempts":1,"class":"route_busy","backoff":true})
    );
    assert_eq!(facts(&f).await["messages"], 0);
    assert_eq!(
        worker.drain_once().await.unwrap(),
        0,
        "the same owner does not immediately reclaim its deferred notice"
    );
    sqlx::query("SET LOCAL lock_timeout='500ms'")
        .execute(&mut *company_writer)
        .await
        .unwrap();
    acquire_channel_gates_on(
        &mut company_writer,
        [ChannelGateKey::new(
            f.people.role.company().as_uuid(),
            f.created.desk_id,
        )],
        ChannelGateAccess::ChangeSettings,
    )
    .await
    .expect("the notice's whole rollback released its shared gate before queue backoff");
    company_writer.rollback().await.unwrap();
    retry_notice(&f).await;
    assert_eq!(notice_queue(&f).await["attempts"], 2);
}

#[tokio::test]
async fn workflow_escalation_agent_real_account_nowait_defers_without_an_agent_account() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let mut retirement = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(f.people.role.admission.binding.target.actor.user_id())
        .execute(&mut *retirement)
        .await
        .unwrap();
    let worker = notification_worker(p, Default::default());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), worker.drain_once())
            .await
            .unwrap()
            .unwrap(),
        1
    );
    assert_eq!(
        notice_queue(&f).await,
        json!({"status":"pending","attempts":1,"class":"route_busy","backoff":true})
    );
    assert_eq!(facts(&f).await["messages"], 0);
    assert_eq!(worker.drain_once().await.unwrap(), 0);
    retirement.rollback().await.unwrap();
    retry_notice(&f).await;
}

#[tokio::test]
async fn workflow_escalation_agent_competing_actual_workers_commit_one_canonical_notice() {
    let f = Box::pin(pending_agent()).await;
    let left = notification_worker(f.people.role.p(), Default::default());
    let right = notification_worker(f.people.role.p(), Default::default());
    let (a, b) = tokio::join!(left.drain_once(), right.drain_once());
    assert_eq!(a.unwrap() + b.unwrap(), 1);
    assert_eq!(notice_queue(&f).await["status"], "projected");
    assert_eq!(
        facts(&f).await,
        json!({"messages":1,"intents":1,"deliveries":0,"person_notifications":0,"runs":0,"cases":1,"hops":1})
    );
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_agent_receipts WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(receipts, 1);
}

#[tokio::test]
async fn workflow_escalation_agent_lost_lease_rolls_back_canonical_message_intent_and_origin() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    sqlx::raw_sql("CREATE SEQUENCE agent_notice_receipt_reached; \
        CREATE FUNCTION slow_agent_notice_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN IF NOT EXISTS(SELECT 1 FROM workflow_escalation_notice_targets AS target \
            JOIN workflow_message_notification_origins AS origin ON origin.company_id=target.company_id \
              AND origin.message_id=target.message_id AND origin.origin_id=target.notice_id \
            JOIN workflow_notification_origins AS token ON token.company_id=target.company_id AND token.id=target.notice_id \
            WHERE target.company_id=NEW.company_id AND target.notice_id=NEW.notice_id) \
          THEN RAISE EXCEPTION 'origin seal was not reached'; END IF; \
          PERFORM nextval('agent_notice_receipt_reached'); PERFORM pg_sleep(2); RETURN NEW; END $$; \
        CREATE TRIGGER zz_slow_agent_notice_receipt AFTER INSERT ON workflow_escalation_agent_receipts \
        FOR EACH ROW EXECUTE FUNCTION slow_agent_notice_receipt();")
        .execute(p.pool()).await.unwrap();
    let claims = p
        .claim_notification_events(WorkerId::random(), Duration::from_secs(1), 1)
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    let expected = p
        .resolve_notification_projection(&claims[0].event)
        .await
        .unwrap();
    let before = facts(&f).await;
    let key = minted_agent_notice_key(&expected);
    let (outcome, _) = p
        .commit_notification_projection(NotificationProjectionCommit {
            fence: &claims[0].lease,
            expected: &expected,
            email: None,
            agent_message_key: key.as_ref(),
            escalation_ack_hours: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(outcome, NotificationProjectionCommitOutcome::LeaseLost);
    let reached: bool = sqlx::query_scalar("SELECT is_called FROM agent_notice_receipt_reached")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(
        reached,
        "the real canonical writer reached receipt insertion before losing its fence"
    );
    assert_eq!(
        facts(&f).await,
        before,
        "message, intent and receipt belong to the lost transaction"
    );
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_agent_receipts")
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(receipts, 0);
    for table in [
        "workflow_escalation_notice_targets",
        "workflow_message_notification_origins",
        "workflow_notification_origins",
        "workflow_message_producer_seals",
    ] {
        let remaining: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 0, "{table} belongs to the lost transaction");
    }
    assert_eq!(notice_queue(&f).await["status"], "processing");
    sqlx::query("DROP TRIGGER zz_slow_agent_notice_receipt ON workflow_escalation_agent_receipts")
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(p.reap_expired_notification_events().await.unwrap(), 1);
    retry_notice(&f).await;
}
