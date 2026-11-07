use super::*;
use crate::entities::transport::{DeliveryStatus, FailureClass};
use crate::transport::{
    ClaimedDelivery, DeliveryOutcome, DeliveryQueue, FailureDetail, PartResult,
    ProviderSendOutcome, WorkerId,
};

pub(super) async fn claim_delivery(
    f: &FinalFixture,
    receipt: ReplyPublicationReceipt,
) -> ClaimedDelivery {
    f.p.claim_deliveries(WorkerId::random(), Duration::from_secs(120), 64)
        .await
        .unwrap()
        .into_iter()
        .find(|claimed| Some(claimed.record.id) == receipt.delivery)
        .expect("the canonical delivery is claimable")
}

async fn published(f: &FinalFixture) -> ReplyPublicationReceipt {
    f.advance().await;
    let outcome = publisher(f).publish_claim(claim(f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else {
        panic!("{outcome:?}")
    };
    receipt
}

async fn finish(
    f: &FinalFixture,
    claimed: &ClaimedDelivery,
    outcome: ProviderSendOutcome,
) -> DeliveryOutcome {
    assert!(matches!(
        f.p.begin_part(&claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    ));
    f.p.complete_part(PartResult {
        fence: &claimed.lease,
        part_id: claimed.parts[0].id,
        outcome: &outcome,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn channel_reply_delivery_safe_retry_keeps_same_message_and_prepared_part() {
    let f = FinalFixture::new("data.map", false).await;
    let receipt = published(&f).await;
    let before = outbound_count(&f).await;
    let first = claim_delivery(&f, receipt).await;
    let original_key = first.parts[0].rendered.clone();
    assert_eq!(
        finish(
            &f,
            &first,
            ProviderSendOutcome::RetryAfter {
                retry_after: Duration::ZERO,
                class: FailureClass::Timeout,
                detail: FailureDetail::parse("explicit provider refusal").unwrap()
            }
        )
        .await,
        DeliveryOutcome::Applied(DeliveryStatus::Retryable)
    );
    let next = claim_delivery(&f, receipt).await;
    assert_eq!(next.record.id, first.record.id);
    assert_eq!(next.parts[0].id, first.parts[0].id);
    assert_eq!(next.parts[0].rendered, original_key);
    assert_eq!(
        finish(
            &f,
            &next,
            ProviderSendOutcome::Delivered { provider_key: None }
        )
        .await,
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_unknown_is_quarantined_and_cannot_be_rewritten_or_deleted() {
    let f = FinalFixture::new("data.map", false).await;
    let receipt = published(&f).await;
    let current = claim_delivery(&f, receipt).await;
    assert_eq!(
        finish(
            &f,
            &current,
            ProviderSendOutcome::OutcomeUnknown {
                class: FailureClass::Timeout,
                detail: FailureDetail::parse("connection lost after request").unwrap()
            }
        )
        .await,
        DeliveryOutcome::Applied(DeliveryStatus::OutcomeUnknown)
    );
    assert!(
        f.p.claim_deliveries(WorkerId::random(), Duration::from_secs(120), 64)
            .await
            .unwrap()
            .is_empty()
    );
    for sql in [
        "UPDATE message_deliveries SET status='pending' WHERE company_id=$1 AND id=$2",
        "UPDATE message_deliveries SET status='delivered' WHERE company_id=$1 AND id=$2",
        "DELETE FROM message_deliveries WHERE company_id=$1 AND id=$2",
        "DELETE FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2",
        "UPDATE message_delivery_parts SET status='retryable' WHERE company_id=$1 AND delivery_id=$2",
        "UPDATE message_delivery_parts SET status='delivered' WHERE company_id=$1 AND delivery_id=$2",
    ] {
        assert!(
            sqlx::query(sql)
                .bind(f.scope.company.as_uuid())
                .bind(current.record.id.as_uuid())
                .execute(f.p.pool())
                .await
                .is_err(),
            "{sql}"
        );
    }
    let run_delete = sqlx::query("DELETE FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(f.p.pool())
        .await;
    assert!(
        run_delete.is_err(),
        "unresolved truth protects its real owner"
    );
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_revocation_before_provider_entry_cancels_unsent_work() {
    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let f = FinalFixture::from_case(
        s,
        FinalCase {
            source: final_source("data.map"),
            contract: literal_contract("Done"),
            policy: None,
            run_as: Some(admin),
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    let receipt = published(&f).await;
    let current = claim_delivery(&f, receipt).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(admin)
        .execute(f.p.pool())
        .await
        .unwrap();
    assert!(matches!(
        f.p.begin_part(&current.lease, current.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(_)
    ));
    let started: bool = sqlx::query_scalar("SELECT request_started_at IS NOT NULL FROM message_delivery_parts WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(current.parts[0].id.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert!(!started);
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_expired_started_request_recovers_unknown_without_resend() {
    let f = FinalFixture::new("data.map", false).await;
    let receipt = published(&f).await;
    let current =
        f.p.claim_deliveries(WorkerId::random(), Duration::from_secs(1), 64)
            .await
            .unwrap()
            .into_iter()
            .find(|claim| Some(claim.record.id) == receipt.delivery)
            .unwrap();
    assert_eq!(
        f.p.begin_part(&current.lease, current.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    sqlx::query("SELECT pg_sleep(GREATEST(0,extract(epoch FROM lock_expires_at-clock_timestamp()))::double precision+0.01) FROM message_deliveries WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(current.record.id.as_uuid()).execute(f.p.pool()).await.unwrap();
    f.p.reap_expired_deliveries().await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT status FROM message_deliveries WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(current.record.id.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(status, "outcome_unknown");
    assert!(
        f.p.claim_deliveries(WorkerId::random(), Duration::from_secs(120), 64)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.p.complete_part(PartResult {
            fence: &current.lease,
            part_id: current.parts[0].id,
            outcome: &ProviderSendOutcome::Delivered { provider_key: None }
        })
        .await
        .unwrap(),
        DeliveryOutcome::LeaseLost
    );
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_unknown_parent_and_run_cannot_be_removed_together() {
    let f = FinalFixture::new("data.map", false).await;
    let receipt = published(&f).await;
    let current = claim_delivery(&f, receipt).await;
    finish(
        &f,
        &current,
        ProviderSendOutcome::OutcomeUnknown {
            class: FailureClass::Timeout,
            detail: FailureDetail::parse("provider result unknown").unwrap(),
        },
    )
    .await;
    let mut tx = f.p.pool().begin().await.unwrap();
    let result: Result<(), sqlx::Error> = async {
        sqlx::query("DELETE FROM message_deliveries WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(current.record.id.as_uuid())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM background_tasks WHERE company_id=$1 AND workflow_execution_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
    .await;
    let error = result.expect_err(
        "unknown provider truth must survive a delivery-plus-owner deletion transaction",
    );
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_authority_lock_deadline_leaves_request_unstarted() {
    let f = FinalFixture::new("data.map", false).await;
    let receipt = published(&f).await;
    let current = claim_delivery(&f, receipt).await;
    let mut blocker = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.s.channel)
        .fetch_all(&mut *blocker)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        f.p.begin_part(&current.lease, current.parts[0].id),
    )
    .await
    .expect("the database lock deadline precedes the outer work bound");
    assert!(
        matches!(result, Err(AppError::DatabaseTimeout(_))),
        "{result:?}"
    );
    blocker.rollback().await.unwrap();
    let started:bool=sqlx::query_scalar("SELECT request_started_at IS NOT NULL FROM message_delivery_parts WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(current.parts[0].id.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert!(!started);
    assert!(matches!(
        f.p.release_delivery(&current.lease).await.unwrap(),
        DeliveryOutcome::Applied(_)
    ));
}
