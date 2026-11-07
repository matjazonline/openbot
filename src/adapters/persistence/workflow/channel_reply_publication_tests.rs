use super::*;
use crate::application::workflow::message::MessagePlanner;
#[path = "channel_reply_publication_authority_tests.rs"]
mod authority;
#[path = "channel_reply_delivery_tests.rs"]
mod delivery;
#[path = "channel_reply_publication_integrity_tests.rs"]
mod integrity;
#[path = "channel_reply_publisher_lifecycle_tests.rs"]
mod lifecycle;
#[path = "channel_reply_membership_tests.rs"]
mod membership;
#[path = "channel_reply_recovery_tests.rs"]
mod recovery;

pub(super) fn publisher(f: &FinalFixture) -> ChannelReplyPublisher<PostgresPersistence> {
    ChannelReplyPublisher::new(
        f.p.clone(),
        MessagePlanner::new(approval_test_support::composer(&f.p), "agents.test".into()),
    )
}

pub(super) async fn claim(f: &FinalFixture) -> ReplyPublicationClaim {
    let claimed =
        f.p.claim_replies(
            ReplyPublicationWorker(Uuid::new_v4()),
            ReplyPublicationPage::try_from(1).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    claimed[0]
}

async fn outbound_count(f: &FinalFixture) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM messages WHERE company_id=$1 AND direction='outbound'")
        .bind(f.scope.company.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn channel_reply_publication_email_and_in_app_commit_exact_content_and_replay() {
    for delivery_policy in [DeliveryPolicy::Deliver, DeliveryPolicy::InAppOnly] {
        let f = FinalFixture::from_case(
            Intents::new().await,
            FinalCase {
                source: final_source("data.map"),
                contract: literal_contract("Done"),
                policy: None,
                run_as: None,
                publication: FinalPublication::Standard,
                delivery_policy,
            },
        )
        .await;
        f.advance().await;
        let before = outbound_count(&f).await;
        let claim = claim(&f).await;
        let publisher = publisher(&f);
        let outcome = publisher.publish_claim(claim).await.unwrap();
        let ReplyPublicationOutcome::Published(receipt) = outcome else {
            panic!("{outcome:?}");
        };
        assert_eq!(
            receipt.delivery.is_some(),
            delivery_policy == DeliveryPolicy::Deliver
        );
        assert_eq!(outbound_count(&f).await, before + 1);
        assert_eq!(publisher.publish_claim(claim).await.unwrap(), outcome);
        assert_eq!(outbound_count(&f).await, before + 1);
        let exact:bool=sqlx::query_scalar("SELECT message.subject=proposal.content->>'subject' AND message.clean_text_body=proposal.content->>'body' \
            AND filed.thread_id=context.thread_id AND filed.channel_id=context.channel_id \
            FROM channel_reply_decisions AS final_reply JOIN channel_reply_proposals AS proposal ON proposal.company_id=final_reply.company_id AND proposal.id=final_reply.proposal_id \
            JOIN channel_reply_contexts AS context ON context.company_id=final_reply.company_id AND context.intent_id=final_reply.intent_id \
            JOIN messages AS message ON message.company_id=final_reply.company_id AND message.id=final_reply.message_id \
            JOIN thread_messages AS filed ON filed.company_id=message.company_id AND filed.message_id=message.id \
            WHERE final_reply.company_id=$1 AND final_reply.run_id=$2 AND final_reply.state='published'")
            .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
        assert!(exact);
        let slots: i64 = sqlx::query_scalar("SELECT workflow_message_slots($1,$2)")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.execution.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
        assert_eq!(
            slots, 1,
            "canonical publication reuses its reserved invocation"
        );
        assert_eq!(f.state().await["run"], "succeeded");
    }
}

#[tokio::test]
async fn channel_reply_publication_competing_transactions_keep_one_canonical_receipt() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let before = outbound_count(&f).await;
    let claim = claim(&f).await;
    let publisher = publisher(&f);
    let barrier = tokio::sync::Barrier::new(2);
    let publish = || async {
        barrier.wait().await;
        publisher.publish_claim(claim).await.unwrap()
    };
    let (a, b) = tokio::join!(publish(), publish());
    assert!(matches!(a, ReplyPublicationOutcome::Published(_)), "{a:?}");
    assert_eq!(a, b);
    assert_eq!(outbound_count(&f).await, before + 1);
    let deliveries: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply'",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(deliveries, 1);
}

#[tokio::test]
async fn channel_reply_publication_internal_source_never_falls_back_to_email() {
    let f = FinalFixture::from_ingress(
        Intents::new().await,
        FinalCase {
            source: final_source("data.map"),
            contract: literal_contract("Internal reply"),
            policy: None,
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
        FinalIngress::Internal,
    )
    .await;
    f.advance().await;
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(receipt.delivery.is_none());
    let shape: Value = sqlx::query_scalar("SELECT jsonb_build_object('audience',message.audience,'entry',filed.entry_kind, \
        'email',EXISTS(SELECT 1 FROM email_message_metadata WHERE company_id=message.company_id AND message_id=message.id)) \
        FROM messages AS message JOIN thread_messages AS filed ON filed.company_id=message.company_id AND filed.message_id=message.id \
        WHERE message.company_id=$1 AND message.id=$2")
        .bind(f.scope.company.as_uuid()).bind(receipt.message.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(
        shape,
        json!({"audience":"internal_only","entry":"note","email":false})
    );
}
