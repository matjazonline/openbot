use super::*;
use crate::application::workflow::message::{
    MessageArguments, MessagePlan, MessagePublicationIdentity,
};

pub(super) async fn plan(f: &FinalFixture, claim: ReplyPublicationClaim) -> MessagePlan {
    let ReplyPublicationLoad::Ready(draft) = f.p.load_reply_publication(claim).await.unwrap()
    else {
        panic!("ready draft")
    };
    MessagePlanner::new(approval_test_support::composer(&f.p), "agents.test".into())
        .plan(
            MessagePublicationIdentity {
                company: claim.company,
                invocation: draft.invocation,
            },
            &MessageArguments::Reply {
                source: draft.source,
                content: draft.content,
            },
            &draft.routes,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn channel_reply_publication_forged_claims_cannot_load_or_commit() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let live = claim(&f).await;
    let plan = plan(&f, live).await;
    let before = outbound_count(&f).await;
    for forged in [
        ReplyPublicationClaim {
            company: CompanyId::new(Uuid::new_v4()),
            ..live
        },
        ReplyPublicationClaim {
            run: RunId::new(Uuid::new_v4()),
            ..live
        },
        ReplyPublicationClaim {
            proposal: crate::domain::workflow::ReplyProposalId::new(Uuid::new_v4()),
            ..live
        },
        ReplyPublicationClaim {
            worker: ReplyPublicationWorker(Uuid::new_v4()),
            ..live
        },
        ReplyPublicationClaim {
            generation: Uuid::new_v4(),
            ..live
        },
        ReplyPublicationClaim {
            expires_at: live.expires_at + chrono::TimeDelta::milliseconds(1),
            ..live
        },
        ReplyPublicationClaim {
            attempt: live.attempt + 1,
            ..live
        },
    ] {
        assert!(matches!(
            f.p.load_reply_publication(forged).await.unwrap(),
            ReplyPublicationLoad::Lost
        ));
        assert_eq!(
            f.p.commit_reply_publication(forged, &plan).await.unwrap(),
            ReplyPublicationOutcome::Lost
        );
    }
    assert_eq!(outbound_count(&f).await, before);
    assert!(matches!(
        f.p.commit_reply_publication(live, &plan).await.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
}

#[tokio::test]
async fn channel_reply_publication_rechecks_current_send_authority() {
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
    f.advance().await;
    let live = claim(&f).await;
    let plan = plan(&f, live).await;
    let before = outbound_count(&f).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(admin)
        .execute(f.p.pool())
        .await
        .unwrap();
    assert_eq!(
        f.p.commit_reply_publication(live, &plan).await.unwrap(),
        ReplyPublicationOutcome::Refused(ReplyPublicationFailure::AuthorityRevoked)
    );
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_publication_policy_change_refuses_without_redirecting() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let live = claim(&f).await;
    let selected =
        f.p.channel_reply_settings(
            f.scope.company,
            RelatedChannelId::new(f.s.channel),
            f.s.owner(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut changed = selection(
        &f.s,
        selected.responder.unwrap().binding,
        Some(selected.revision),
    )
    .await;
    changed.approval_policy =
        Some(ReplyApprovalPolicy::new(vec![f.s.owner_principal()], 60).unwrap());
    f.p.configure_channel_reply(changed).await.unwrap();
    let before = outbound_count(&f).await;
    assert_eq!(
        publisher(&f).publish_claim(live).await.unwrap(),
        ReplyPublicationOutcome::Refused(ReplyPublicationFailure::PolicyChanged)
    );
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_publication_changed_plan_has_no_partial_effects() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let live = claim(&f).await;
    let mut plan = plan(&f, live).await;
    plan.destinations[0].message.clean_text_body = "substituted result".into();
    let before = outbound_count(&f).await;
    assert_eq!(
        f.p.commit_reply_publication(live, &plan).await.unwrap(),
        ReplyPublicationOutcome::Refused(ReplyPublicationFailure::InvalidProposal)
    );
    assert_eq!(outbound_count(&f).await, before);
}

#[tokio::test]
async fn channel_reply_publication_write_failure_rolls_back_then_replays_original_plan() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let live = claim(&f).await;
    let plan = plan(&f, live).await;
    let before = outbound_count(&f).await;
    sqlx::raw_sql("CREATE FUNCTION reject_final_publication() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected publication failure'; END $$; \
        CREATE TRIGGER reject_final_publication AFTER UPDATE ON channel_reply_decisions FOR EACH ROW WHEN (NEW.state='published') EXECUTE FUNCTION reject_final_publication();")
        .execute(f.p.pool()).await.unwrap();
    assert!(f.p.commit_reply_publication(live, &plan).await.is_err());
    assert_eq!(outbound_count(&f).await, before);
    assert_eq!(f.state().await["decision"], "claimed");
    sqlx::raw_sql("DROP TRIGGER reject_final_publication ON channel_reply_decisions; DROP FUNCTION reject_final_publication();").execute(f.p.pool()).await.unwrap();
    let original = f.p.commit_reply_publication(live, &plan).await.unwrap();
    assert!(matches!(original, ReplyPublicationOutcome::Published(_)));
    assert_eq!(
        publisher(&f).publish_claim(live).await.unwrap(),
        original,
        "a lost acknowledgement observes the original IDs"
    );
    assert_eq!(outbound_count(&f).await, before + 1);
}

#[tokio::test]
async fn channel_reply_publication_approved_content_survives_elapsed_review_deadline_and_contract_edit()
 {
    let s = Intents::new().await;
    let owner_principal = s.owner_principal();
    let f = FinalFixture::from_case(
        s,
        FinalCase {
            source: final_source("data.map"),
            contract: literal_contract("Done"),
            policy: Some(ReplyApprovalPolicy::new(vec![owner_principal], 2).unwrap()),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    )
    .await;
    f.advance().await;
    f.p.submit_decision(f.command("approve").await)
        .await
        .unwrap();
    assert!(matches!(
        f.p.resume_finalization(f.scope).await.unwrap(),
        FinalizationProgress::Completed(_)
    ));
    let selected =
        f.p.channel_reply_settings(
            f.scope.company,
            RelatedChannelId::new(f.s.channel),
            f.s.owner(),
        )
        .await
        .unwrap()
        .unwrap();
    let mut changed = selection(
        &f.s,
        selected.responder.unwrap().binding,
        Some(selected.revision),
    )
    .await;
    changed.approval_policy = selected.approval_policy;
    changed.responder.as_mut().unwrap().contract = literal_contract("New contract result");
    f.p.configure_channel_reply(changed).await.unwrap();
    sqlx::query("SELECT pg_sleep(GREATEST(0,extract(epoch FROM approval_deadline-clock_timestamp()))::double precision+0.01) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).execute(f.p.pool()).await.unwrap();
    let outcome = publisher(&f).publish_claim(claim(&f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = outcome else {
        panic!("{outcome:?}")
    };
    let body: String =
        sqlx::query_scalar("SELECT clean_text_body FROM messages WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(receipt.message.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(body, "Done");
    let claimed = super::delivery::claim_delivery(&f, receipt).await;
    assert!(matches!(
        crate::transport::DeliveryQueue::begin_part(&f.p, &claimed.lease, claimed.parts[0].id)
            .await
            .unwrap(),
        crate::transport::DeliveryOutcome::Applied(_)
    ));
}
