use super::*;
use crate::adapters::persistence::workflow::channel_reply_views;
use crate::application::thread_activity::ThreadActivityReader;
use crate::application::workflow::channel_reply::{ChannelResponseOutcome, FinalPublicationState};
use crate::application::workflow::run_views::{RunReadScope, RunViews};
use crate::entities::transport::{DeliveryStatus, FailureClass};
use crate::transport::{
    DeliveryOutcome, DeliveryQueue, FailureDetail, PartResult, ProviderSendOutcome,
};

#[path = "channel_reply_continuation_views_tests.rs"]
mod continuations;

async fn outcome(f: &FinalFixture) -> ChannelResponseOutcome {
    channel_reply_views::read(&f.p, f.scope.company, f.s.owner(), &[f.scope.run.as_uuid()])
        .await
        .unwrap()
        .pop()
        .unwrap()
}
async fn publish(f: &FinalFixture) -> ReplyPublicationReceipt {
    let result = publisher(f).publish_claim(claim(f).await).await.unwrap();
    let ReplyPublicationOutcome::Published(receipt) = result else {
        panic!("{result:?}")
    };
    receipt
}
async fn settle(
    f: &FinalFixture,
    receipt: ReplyPublicationReceipt,
    result: ProviderSendOutcome,
) -> DeliveryOutcome {
    let claimed = super::delivery::claim_delivery(f, receipt).await;
    f.p.begin_part(&claimed.lease, claimed.parts[0].id)
        .await
        .unwrap();
    f.p.complete_part(PartResult {
        fence: &claimed.lease,
        part_id: claimed.parts[0].id,
        outcome: &result,
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn channel_reply_views_completed_work_and_safe_delivery_retry_are_independent() {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    assert_eq!(outcome(&f).await.label(), "Processing");
    f.advance().await;
    assert_eq!(outcome(&f).await.label(), "Work completed; reply pending");
    let business = f.state().await["output"].clone();
    let receipt = publish(&f).await;
    assert_eq!(
        outcome(&f).await.final_response.unwrap().message,
        Some(receipt.message)
    );
    let retry = ProviderSendOutcome::RetryAfter {
        retry_after: Duration::ZERO,
        class: FailureClass::Network,
        detail: FailureDetail::parse("Definitely not accepted").unwrap(),
    };
    assert_eq!(
        settle(&f, receipt, retry).await,
        DeliveryOutcome::Applied(DeliveryStatus::Retryable)
    );
    assert_eq!(outcome(&f).await.label(), "Work completed; reply pending");
    assert_eq!(
        settle(
            &f,
            receipt,
            ProviderSendOutcome::Delivered { provider_key: None }
        )
        .await,
        DeliveryOutcome::Applied(DeliveryStatus::Delivered)
    );
    let final_state = outcome(&f).await;
    assert_eq!(final_state.label(), "Completed; provider accepted reply");
    assert_eq!(
        final_state.final_response.unwrap().message,
        Some(receipt.message)
    );
    assert_eq!(f.state().await["output"], business);
    assert_eq!(f.state().await["run"], "succeeded");
}

#[tokio::test]
async fn channel_reply_controls_unknown_delivery_is_visible_without_a_business_or_blind_resend_control()
 {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    f.advance().await;
    let receipt = publish(&f).await;
    settle(
        &f,
        receipt,
        ProviderSendOutcome::OutcomeUnknown {
            class: FailureClass::Timeout,
            detail: FailureDetail::parse("Lost acknowledgement").unwrap(),
        },
    )
    .await;
    let response = outcome(&f).await;
    assert_eq!(response.label(), "Work completed; reply needs attention");
    let html = crate::adapters::http::pages::channel_response_panel(
        &response,
        f.scope.company.as_uuid(),
        crate::adapters::http::pages::RunLinkAccess::Inspect,
    );
    assert!(html.contains("Outcome unconfirmed; do not resend blindly"));
    assert!(html.contains(&format!("entry_id={}", receipt.delivery.unwrap().as_uuid())));
    assert!(!html.contains("hx-post"));
    assert!(!html.contains("/retry"));
    assert!(
        f.p.claim_deliveries(
            crate::transport::WorkerId::random(),
            Duration::from_secs(30),
            1
        )
        .await
        .unwrap()
        .is_empty()
    );
}

#[tokio::test]
async fn channel_reply_views_suppression_and_private_review_are_separate_from_publication() {
    let silent = Box::pin(FinalFixture::new("channel.no_reply", false)).await;
    silent.advance().await;
    let response = outcome(&silent).await;
    assert_eq!(response.label(), "Completed without automatic reply");
    assert_eq!(
        response.final_response.unwrap().publication,
        FinalPublicationState::Suppressed
    );
    let reviewed = Box::pin(FinalFixture::new("data.map", true)).await;
    reviewed.advance().await;
    let response = outcome(&reviewed).await;
    assert_eq!(response.label(), "Waiting for reviewer");
    assert!(response.final_response.is_none());
    assert!(response.prompt.is_none());
    let html = crate::adapters::http::pages::channel_response_panel(
        &response,
        reviewed.scope.company.as_uuid(),
        crate::adapters::http::pages::RunLinkAccess::Label,
    );
    assert!(!html.contains("approval_token"));
    assert!(!html.contains("decision_id"));
    assert!(!html.contains("/workflow-decisions/"));
    assert!(!html.contains("/deliveries"));
}

#[tokio::test]
async fn channel_reply_views_classification_and_company_scope_hide_response_details() {
    let f = Box::pin(classified_fixture()).await;
    let user = member(&f.s.fixture, "member").await;
    let mut tx = f.p.pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
        &mut tx,
        f.scope.company,
        f.scope.run,
        crate::entities::access_level::AccessLevel::L5,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let viewer = WorkflowActor::authenticated(user).unwrap();
    assert!(
        channel_reply_views::read(&f.p, f.scope.company, viewer, &[f.scope.run.as_uuid()])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        channel_reply_views::read(
            &f.p,
            CompanyId::new(Uuid::new_v4()),
            f.s.owner(),
            &[f.scope.run.as_uuid()]
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        channel_reply_views::read(&f.p, f.scope.company, f.s.owner(), &[f.scope.run.as_uuid()])
            .await
            .unwrap()
            .len(),
        1
    );
    let view =
        f.p.run_view(RunReadScope {
            company: f.scope.company,
            run: f.scope.run,
            viewer,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(view.payload_restricted);
    assert!(view.response.is_none());
}

async fn classified_intents() -> Intents {
    let s = Box::pin(Intents::new()).await;
    let company = s.fixture.target.company.as_uuid();
    let principal:Uuid=sqlx::query_scalar("SELECT principal.id FROM thread_principals AS party JOIN principals AS principal ON principal.company_id=party.company_id AND principal.id=party.principal_id WHERE party.company_id=$1 AND party.thread_id=$2 AND principal.kind='external' ORDER BY principal.id LIMIT 1")
        .bind(company).bind(s.thread).fetch_one(s.pool()).await.unwrap();
    let user = s
        .p()
        .create_user(
            &Uuid::new_v4().simple().to_string(),
            "customer@partner.test",
            "hash",
        )
        .await
        .unwrap()
        .id;
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',5)")
        .bind(Uuid::new_v4()).bind(company).bind(user).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE principals SET kind='person',user_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(principal)
        .bind(user)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE participant_identities SET status='verified',provenance='account',claim_metadata='{\"version\":1,\"kind\":\"account\"}'::jsonb WHERE company_id=$1 AND principal_id=$2")
        .bind(company).bind(principal).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    s
}

async fn classified_fixture() -> FinalFixture {
    // Genuine ingress freezes the real L5 audience; never rewrite an immutable run ceiling.
    Box::pin(FinalFixture::configured(
        Box::pin(classified_intents()).await,
        final_source("data.map"),
        literal_contract("Done"),
        false,
    ))
    .await
}

#[tokio::test]
async fn channel_reply_views_source_outside_the_message_window_retains_its_exact_identity() {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    let source = outcome(&f).await.source;
    for _ in 0..201 {
        f.s.message().await;
    }
    let runs =
        f.p.thread_runs(f.scope.company.as_uuid(), f.s.thread, f.s.owner().user_id())
            .await
            .unwrap();
    assert!(
        !runs
            .message_runs
            .iter()
            .any(|message| message.canonical_id == source.as_uuid())
    );
    assert_eq!(runs.response_outcomes.len(), 1);
    assert_eq!(runs.response_outcomes[0].source, source);
    let html = crate::adapters::http::pages::thread_response_outcomes(
        &runs,
        f.scope.company.as_uuid(),
        crate::adapters::http::pages::RunLinkAccess::Label,
    );
    assert!(html.contains(&format!("message_id={}", source.as_uuid())));
    assert!(html.contains(&format!("thread_id={}", f.s.thread)));
}

#[tokio::test]
async fn channel_reply_views_response_rows_are_bounded_independently_of_message_attribution() {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    for _ in 0..50 {
        f.s.intent(TO).await;
        assert_eq!(f.s.drain(&f.s.admissions(admission(f.s.p()))).await, 1);
    }
    let runs =
        f.p.thread_runs(f.scope.company.as_uuid(), f.s.thread, f.s.owner().user_id())
            .await
            .unwrap();
    assert_eq!(runs.response_outcomes.len(), 50);
    assert!(runs.responses_truncated);
    let ids = vec![Uuid::new_v4(); 51];
    assert!(matches!(
        channel_reply_views::read(&f.p, f.scope.company, f.s.owner(), &ids).await,
        Err(crate::app_error::AppError::BadRequest(_))
    ));
}

async fn listener(f: &FinalFixture) -> sqlx::postgres::PgListener {
    let mut listener = sqlx::postgres::PgListener::connect_with(f.p.pool())
        .await
        .unwrap();
    listener.listen("workflow_run_changed").await.unwrap();
    listener.listen("thread_activity").await.unwrap();
    listener
}
async fn notifications(listener: &mut sqlx::postgres::PgListener) -> Vec<(String, Value)> {
    let mut results = vec![];
    for _ in 0..2 {
        let note = tokio::time::timeout(Duration::from_secs(2), listener.recv())
            .await
            .unwrap()
            .unwrap();
        results.push((
            note.channel().to_owned(),
            serde_json::from_str(note.payload()).unwrap(),
        ));
    }
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results
}

#[tokio::test]
async fn channel_reply_views_delivery_wakes_the_completed_run_and_conversation_with_ids_only() {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    f.advance().await;
    let receipt = publish(&f).await;
    let mut listener = listener(&f).await;
    settle(
        &f,
        receipt,
        ProviderSendOutcome::Delivered { provider_key: None },
    )
    .await;
    let notes = notifications(&mut listener).await;
    assert_eq!(notes[0].0, "thread_activity");
    assert_eq!(notes[1].0, "workflow_run_changed");
    assert_eq!(
        notes[0].1,
        json!({"company_id":f.scope.company.as_uuid(),"channel_id":f.s.channel,"thread_id":f.s.thread})
    );
    assert_eq!(
        notes[1].1,
        json!({"company_id":f.scope.company.as_uuid(),"channel_id":f.s.channel,"thread_id":f.s.thread,
        "run_id":f.scope.run.as_uuid(),"state":"succeeded","waiting_reason":null})
    );
    assert_eq!(
        outcome(&f).await.label(),
        "Completed; provider accepted reply"
    );
}

#[tokio::test]
async fn channel_reply_views_notifications_are_transactional_and_rollback_is_silent() {
    let f = Box::pin(FinalFixture::new("data.map", false)).await;
    let mut listener = listener(&f).await;
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT notify_channel_response_run($1,$2)")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.recv())
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    assert_eq!(notifications(&mut listener).await.len(), 2);
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT notify_channel_response_run($1,$2)")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.recv())
            .await
            .is_err()
    );
}
