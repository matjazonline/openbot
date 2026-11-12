use super::*;
use crate::application::ports::account_deletion::HostAccountDeletion;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::use_cases::company_invite::CompanyInvitePersistence;

async fn second_decision(f: &DecisionFixture) -> DecisionId {
    let command = f
        .admission
        .prepare(f.admission.request("notice-second", f.admission.manual()))
        .await;
    f.persistence().admit(&command).await.unwrap();
    let job: Uuid =
        sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
            .bind(command.first_execution_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let scope = ActivationRequest {
        company: f.scope.company,
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    assert!(
        f.persistence()
            .park_decision(scope, &f.notices)
            .await
            .unwrap()
            .is_some()
    );
    DecisionId(
        sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.execution.as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap(),
    )
}

#[derive(Clone, Copy)]
enum Retirement {
    Membership,
    Account,
}
#[derive(sqlx::FromRow)]
struct NoticeState {
    status: String,
    cancellation_reason: Option<String>,
}

#[tokio::test]
async fn workflow_reviewer_principal_retirement_cancels_pending_and_inflight_notices() {
    for retirement in [Retirement::Membership, Retirement::Account] {
        let f = Box::pin(DecisionFixture::new(2, None)).await;
        let first = f.park().await;
        let second = second_decision(&f).await;
        let pending = reviewer_rows(&f, first.decision).await[0]
            .notice_delivery_id
            .unwrap();
        let sending = reviewer_rows(&f, second).await[0]
            .notice_delivery_id
            .unwrap();
        sqlx::query("UPDATE message_deliveries SET status='sending',execution_id=gen_random_uuid(),owner_worker_id=gen_random_uuid(),locked_at=clock_timestamp(),lock_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(sending).execute(f.persistence().pool()).await.unwrap();
        match retirement {
            Retirement::Membership => f
                .persistence()
                .remove_member(f.scope.company.as_uuid(), f.users[0])
                .await
                .unwrap(),
            Retirement::Account => f
                .persistence()
                .delete_user_account(f.users[0])
                .await
                .unwrap(),
        }
        let pending: NoticeState =
            sqlx::query_as("SELECT status,cancellation_reason FROM message_deliveries WHERE id=$1")
                .bind(pending)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        let sending: NoticeState =
            sqlx::query_as("SELECT status,cancellation_reason FROM message_deliveries WHERE id=$1")
                .bind(sending)
                .fetch_one(f.persistence().pool())
                .await
                .unwrap();
        assert_eq!(pending.status, "dead_letter");
        assert_eq!(sending.status, "sending");
        assert_eq!(
            pending.cancellation_reason.as_deref(),
            Some("recipient_revoked")
        );
        assert_eq!(
            sending.cancellation_reason.as_deref(),
            Some("recipient_revoked")
        );
        let kept:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers AS reviewer JOIN message_deliveries AS delivery ON delivery.id=reviewer.notice_delivery_id WHERE reviewer.company_id=$1 AND reviewer.principal_id=$2 AND delivery.status='pending' AND delivery.cancellation_requested_at IS NULL")
            .bind(f.scope.company.as_uuid()).bind(f.principals[1]).fetch_one(f.persistence().pool()).await.unwrap();
        assert_eq!(kept, 2);
        let history = f
            .persistence()
            .read_decision(
                f.scope.company,
                f.admission.binding.target.actor,
                first.decision,
            )
            .await
            .unwrap();
        assert_eq!(history.reviewers, first.reviewers);
        assert_eq!(history.reviewer_labels, first.reviewer_labels);
    }
}

#[tokio::test]
async fn workflow_reviewer_principal_schema_person_guard_and_immutable_bounded_label() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let oversized = "😀".repeat(64);
    let error = sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.principals[0])
        .bind(&oversized)
        .execute(f.persistence().pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("principals_display_label_check")
    );
    let largest = format!("{}abc", "😀".repeat(63));
    sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.principals[0])
        .bind(&largest)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let view = f.park().await;
    assert_eq!(view.reviewer_labels[0].label, largest);
    assert_eq!(view.reviewer_labels[0].label.len(), 255);
    let legacy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='public' AND table_name='workflow_decision_reviewers' AND column_name='user_id')")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert!(!legacy);
    let error=sqlx::query("UPDATE workflow_decision_reviewers SET reviewer_label='forged' WHERE company_id=$1 AND decision_id=$2")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_decision_fact_immutable")
    );
    let agent = AgentPersistence::create(
        f.persistence(),
        f.scope.company.as_uuid(),
        AgentWrite {
            name: "Not a person".into(),
            slug: "not-person".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(agent.id)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    let error=sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,ordinal,principal_id,reviewer_label,notice_opted_out) VALUES($1,$2,1,$3,'forged',true)")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(principal).execute(f.persistence().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:workflow_decision_person_snapshot")
    );
}

#[tokio::test]
async fn workflow_reviewer_principal_insert_derives_label_and_decision_ui_escapes_it() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let hostile = "Person </p><img src=x onerror=alert(1)>";
    sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.principals[0])
        .bind(hostile)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let view = f.park().await;
    assert_eq!(view.reviewer_labels[0].principal, f.principals[0]);
    assert_eq!(view.reviewer_labels[0].label, hostile);
    let html = crate::adapters::http::pages::workflow_decision_section(&view, None, None);
    assert!(html.contains("Person &lt;/p&gt;&lt;img") && !html.contains("<img src=x"));
    let owner = f.admission.binding.target.actor.user_id();
    let owner_principal = f.admission.binding.fixture.principal(owner).await;
    sqlx::query("INSERT INTO user_notification_preferences(user_id,decision_email_enabled) VALUES($1,false)")
        .bind(owner).execute(f.persistence().pool()).await.unwrap();
    sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,ordinal,principal_id,reviewer_label,notice_opted_out) VALUES($1,$2,1,$3,'forged',true)")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(owner_principal).execute(f.persistence().pool()).await.unwrap();
    let actual:String=sqlx::query_scalar("SELECT reviewer_label FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 AND principal_id=$3")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(owner_principal).fetch_one(f.persistence().pool()).await.unwrap();
    let expected: String =
        sqlx::query_scalar("SELECT display_label FROM principals WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(owner_principal)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(actual, expected);
    assert_ne!(actual, "forged");
}
