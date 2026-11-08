use super::*;
use crate::entities::transport::{DeliveryCancellationReason, DeliveryStatus};
use crate::transport::{
    DeliveryOutcome, DeliveryQueue, PartIndex, PartKey, PartResult, ProviderSendOutcome,
};

#[tokio::test]
async fn channel_reply_publication_frozen_association_metadata_and_parts_cannot_change() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else {
        panic!("{outcome:?}")
    };
    for sql in [
        "UPDATE messages SET clean_text_body='replacement' WHERE company_id=$1 AND id=$2",
        "DELETE FROM thread_messages WHERE company_id=$1 AND message_id=$2",
        "UPDATE thread_messages SET entry_kind='note' WHERE company_id=$1 AND message_id=$2",
        "DELETE FROM email_message_metadata WHERE company_id=$1 AND message_id=$2",
        "DELETE FROM message_participants WHERE company_id=$1 AND message_id=$2",
    ] {
        assert!(
            sqlx::query(sql)
                .bind(f.scope.company.as_uuid())
                .bind(receipt.message.as_uuid())
                .execute(f.p.pool())
                .await
                .is_err(),
            "{sql}"
        );
    }
    let delivery = receipt.delivery.unwrap().as_uuid();
    for sql in [
        "UPDATE message_delivery_parts SET content_digest='changed' WHERE company_id=$1 AND delivery_id=$2",
        "INSERT INTO message_delivery_parts(id,company_id,delivery_id,part_index,part_key,payload,content_digest) \
            SELECT gen_random_uuid(),company_id,delivery_id,part_index+1,'injected',payload,content_digest FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2",
        "UPDATE message_deliveries SET external_destination='other@partner.test' WHERE company_id=$1 AND id=$2",
    ] {
        assert!(
            sqlx::query(sql)
                .bind(f.scope.company.as_uuid())
                .bind(delivery)
                .execute(f.p.pool())
                .await
                .is_err(),
            "{sql}"
        );
    }
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.scope.company.as_uuid())
        .execute(f.p.pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn channel_reply_publication_current_binding_is_required_at_commit_and_provider_entry() {
    for boundary in ["publication", "delivery"] {
        let f = FinalFixture::new("data.map", false).await;
        f.advance().await;
        let live = claim(&f).await;
        let plan = super::authority::plan(&f, live).await;
        let receipt = if boundary == "delivery" {
            let ReplyPublicationOutcome::Published(receipt) =
                f.p.commit_reply_publication(live, &plan).await.unwrap()
            else {
                panic!("published")
            };
            Some(receipt)
        } else {
            None
        };
        let current = if let Some(receipt) = receipt {
            Some(super::delivery::claim_delivery(&f, receipt).await)
        } else {
            None
        };
        sqlx::query(
            "UPDATE channel_bindings SET status='paused' WHERE company_id=$1 AND channel_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.s.channel)
        .execute(f.p.pool())
        .await
        .unwrap();
        if let Some(current) = current {
            assert!(matches!(
                f.p.begin_part(&current.lease, current.parts[0].id)
                    .await
                    .unwrap(),
                DeliveryOutcome::Cancelled(_)
            ));
        } else {
            assert_eq!(
                f.p.commit_reply_publication(live, &plan).await.unwrap(),
                ReplyPublicationOutcome::Refused(ReplyPublicationFailure::SourceUnavailable)
            );
        }
        assert_eq!(f.state().await["run"], "succeeded");
    }
}

#[tokio::test]
async fn channel_reply_publication_release_rechecked_after_success_and_before_provider() {
    for boundary in ["publication", "delivery"] {
        let s = Intents::new().await;
        let recipient = super::super::policy_runtime_tests::verified_recipient(&s).await;
        let f = FinalFixture::configured(
            s,
            final_source("data.map"),
            literal_contract("Classified"),
            false,
        )
        .await;
        sqlx::query("UPDATE workflow_runs SET content_level=5 WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .execute(f.p.pool())
            .await
            .unwrap();
        assert_eq!(f.advance().await.disposition, BatchDisposition::Completed);
        let live = claim(&f).await;
        assert_canonical_reason_constraint(&f).await;
        let receipt = if boundary == "delivery" {
            let ReplyPublicationOutcome::Published(receipt) =
                publisher(&f).publish_claim(live).await.unwrap()
            else {
                panic!("published")
            };
            Some(receipt)
        } else {
            None
        };
        sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(recipient)
            .execute(f.p.pool())
            .await
            .unwrap();
        if let Some(receipt) = receipt {
            let current = super::delivery::claim_delivery(&f, receipt).await;
            assert!(matches!(
                f.p.begin_part(&current.lease, current.parts[0].id)
                    .await
                    .unwrap(),
                DeliveryOutcome::Cancelled(_)
            ));
        } else {
            assert_eq!(
                publisher(&f).publish_claim(live).await.unwrap(),
                ReplyPublicationOutcome::Refused(ReplyPublicationFailure::ReleaseDenied)
            );
            assert!(matches!(
                f.p.load_reply_publication(live).await.unwrap(),
                ReplyPublicationLoad::Failed(ReplyPublicationFailure::ReleaseDenied)
            ));
            let reason:String=sqlx::query_scalar("SELECT diagnostic_code FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
                .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
            assert_eq!(reason, "knowledge.audience_below_content");
            assert_eq!(
                serde_json::to_value(ReplyPublicationFailure::ReleaseDenied).unwrap(),
                json!(reason)
            );
            assert_eq!(
                serde_json::from_value::<ReplyPublicationFailure>(json!(reason)).unwrap(),
                ReplyPublicationFailure::ReleaseDenied
            );
            assert!(
                serde_json::from_value::<ReplyPublicationFailure>(json!("release_denied")).is_err()
            );
        }
        assert_eq!(f.state().await["run"], "succeeded");
    }
}

#[tokio::test]
async fn channel_reply_delivery_multipart_cancellation_preserves_accepted_piece() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let live = claim(&f).await;
    let mut plan = super::authority::plan(&f, live).await;
    // Supply a valid two-part transport result before its atomic canonical publication.
    let delivery = plan.destinations[0].delivery.as_mut().unwrap();
    let mut second = delivery.parts[0].clone();
    second.index = PartIndex::new(1);
    second.key = PartKey::parse(format!("{}-second", second.key.as_str())).unwrap();
    delivery.parts =
        crate::transport::BoundedVec::parse("parts", vec![delivery.parts[0].clone(), second])
            .unwrap();
    let ReplyPublicationOutcome::Published(receipt) =
        f.p.commit_reply_publication(live, &plan).await.unwrap()
    else {
        panic!("published")
    };
    let current = super::delivery::claim_delivery(&f, receipt).await;
    assert_eq!(current.parts.len(), 2);
    assert_eq!(
        f.p.begin_part(&current.lease, current.parts[0].id)
            .await
            .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    let delivered = ProviderSendOutcome::Delivered { provider_key: None };
    assert_eq!(
        f.p.complete_part(PartResult {
            fence: &current.lease,
            part_id: current.parts[0].id,
            outcome: &delivered
        })
        .await
        .unwrap(),
        DeliveryOutcome::Applied(DeliveryStatus::Sending)
    );
    let mut tx = f.p.pool().begin().await.unwrap();
    crate::adapters::persistence::delivery::cancellation::cancel_deliveries_on(
        &mut tx,
        f.scope.company.as_uuid(),
        &[current.record.id.as_uuid()],
        DeliveryCancellationReason::WorkflowCancelled,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(matches!(
        f.p.begin_part(&current.lease, current.parts[1].id)
            .await
            .unwrap(),
        DeliveryOutcome::Cancelled(_)
    ));
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2 ORDER BY part_index")
        .bind(f.scope.company.as_uuid()).bind(current.record.id.as_uuid()).fetch_all(f.p.pool()).await.unwrap();
    assert_eq!(statuses, ["delivered", "dead"]);
    assert!(sqlx::query("UPDATE message_delivery_parts SET status='retryable',delivered_at=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(current.parts[0].id.as_uuid()).execute(f.p.pool()).await.is_err());
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_delivery_resolved_owner_can_be_retained_without_losing_message() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let ReplyPublicationOutcome::Published(receipt) =
        publisher(&f).publish_claim(claim(&f).await).await.unwrap()
    else {
        panic!("published")
    };
    let eligible = |f: &FinalFixture| {
        let pool = f.p.pool().clone();
        let company = f.scope.company;
        let run = f.scope.run;
        async move {
            sqlx::query_scalar::<_, bool>(include_str!("storage_retention_eligible.sql"))
                .bind(company.as_uuid())
                .bind(run.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert!(!eligible(&f).await);
    let current = super::delivery::claim_delivery(&f, receipt).await;
    f.p.begin_part(&current.lease, current.parts[0].id)
        .await
        .unwrap();
    f.p.complete_part(PartResult {
        fence: &current.lease,
        part_id: current.parts[0].id,
        outcome: &ProviderSendOutcome::Delivered { provider_key: None },
    })
    .await
    .unwrap();
    assert!(eligible(&f).await);
    super::super::intent_schema_tests::retain_closed_run(&f).await;
    let retained: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages WHERE company_id=$1 AND id=$2)")
            .bind(f.scope.company.as_uuid())
            .bind(receipt.message.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert!(retained);
}

#[tokio::test]
async fn channel_reply_publication_approved_reviewer_must_retain_payload_clearance() {
    let s = Intents::new().await;
    super::super::policy_runtime_tests::verified_recipient(&s).await;
    let reviewer = member(&s.fixture, "admin").await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(reviewer)
        .execute(s.pool())
        .await
        .unwrap();
    let f = FinalFixture::from_case(
        s,
        FinalCase {
            source: final_source("data.map"),
            contract: literal_contract("Classified"),
            policy: Some(ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap()),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    sqlx::query("UPDATE workflow_runs SET content_level=5 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(f.p.pool())
        .await
        .unwrap();
    assert_eq!(f.advance().await.disposition, BatchDisposition::Deferred);
    let mut command = f.command("approve").await;
    command.actor = WorkflowActor::authenticated(reviewer).unwrap();
    f.p.submit_decision(command).await.unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(reviewer)
        .execute(f.p.pool())
        .await
        .unwrap();
    let before = outbound_count(&f).await;
    assert_eq!(
        publisher(&f).publish_claim(claim(&f).await).await.unwrap(),
        ReplyPublicationOutcome::Refused(ReplyPublicationFailure::AuthorityRevoked)
    );
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["run"], "succeeded");
}

async fn assert_canonical_reason_constraint(f: &FinalFixture) {
    for reason in [
        "release_denied",
        "knowledge.invalid",
        "knowledge.audience_below_content",
    ] {
        let mut tx = f.p.pool().begin().await.unwrap();
        let result = sqlx::query(
            "UPDATE channel_reply_decisions SET state='failed',diagnostic_code=$3,
            worker_id=NULL,generation=NULL,lease_expires_at=NULL WHERE company_id=$1 AND run_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .bind(reason)
        .execute(&mut *tx)
        .await;
        if reason == "knowledge.audience_below_content" {
            assert_eq!(result.unwrap().rows_affected(), 1);
        } else {
            let error = result.unwrap_err();
            let database = error.as_database_error().unwrap();
            assert_eq!(database.code().as_deref(), Some("23514"));
            assert_eq!(
                database.constraint(),
                Some("channel_reply_decisions_diagnostic_code_check")
            );
        }
        tx.rollback().await.unwrap();
    }
}
