use super::*;
#[path = "escalation_browser.rs"]
mod browser;
use crate::application::app_error::WorkflowGuardViolation;
use crate::application::ports::channel::ChannelOwnershipWrite;
use crate::application::ports::department::DepartmentWrite;
use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
use crate::application::workflow::guard::{
    GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
};
use crate::domain::workflow::RunId;
use crate::entities::{department::DepartmentRole, transport::PrincipalId};

struct Escalating {
    f: Fixture,
    decision: Uuid,
    principal: Uuid,
}

impl Escalating {
    async fn new() -> Self {
        let f = Fixture::new().await.expect("owned test database");
        sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
            .bind(f.owner)
            .execute(f.persistence.pool())
            .await
            .unwrap();
        let principal: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
                .bind(f.company)
                .bind(f.owner)
                .fetch_one(f.persistence.pool())
                .await
                .unwrap();
        sqlx::query("UPDATE principals SET display_label=$3 WHERE company_id=$1 AND id=$2")
            .bind(f.company)
            .bind(principal)
            .bind("Review <img src=x onerror=alert(1)> & \"team\"")
            .execute(f.persistence.pool())
            .await
            .unwrap();
        let departments =
            crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
        let department = departments
            .create_department(
                f.owner,
                f.company,
                DepartmentWrite {
                    name: "Operations".into(),
                    slug: "operations".into(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        departments
            .add_member(
                f.owner,
                f.company,
                department.id,
                PrincipalId::new(principal),
            )
            .await
            .unwrap();
        departments
            .assign_role(
                f.owner,
                f.company,
                department.id,
                DepartmentRole::Reviewer,
                PrincipalId::new(principal),
            )
            .await
            .unwrap();
        let channel = ChannelPersistence::create(
            &f.persistence,
            f.company,
            ChannelWrite {
                name: "Operations".into(),
                description: None,
                slug: "operations-work".into(),
                alias_slugs: vec![],
                participant_emails: None,
                enabled: true,
                add_3rd_party: false,
                created_by: None,
                ownership: Some(ChannelOwnershipWrite {
                    department_id: Some(Some(department.id)),
                    members_only: Some(false),
                }),
            },
        )
        .await
        .unwrap();
        let (_, version) = f
            .published(f.owner, f.company, example("data.map", Uuid::new_v4()))
            .await;
        let binding = Uuid::new_v4();
        let (status, configured) = answer(
            bindings::configure(
                f.state(),
                by(f.owner),
                Path((f.company, binding)),
                body(json!({
                    "association":{"kind":"channel","channel_id":channel.id},"version_id":version,
                    "escalation":{"notify":"reviewer","hops":1}
                })),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{configured}");
        let (status, activated) = f
            .activate(
                f.owner,
                f.company,
                binding,
                configured["state_revision"].as_u64().unwrap(),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{activated}");
        let (status, started) = f
            .start(
                f.owner,
                f.company,
                binding,
                "administrative-route",
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{started}");
        let run = uuid(&started["run_id"]);
        #[derive(sqlx::FromRow)]
        struct Scope {
            execution: Uuid,
            job: Uuid,
        }
        let scope:Scope=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution \
            JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
            WHERE execution.company_id=$1 AND execution.run_id=$2")
            .bind(f.company).bind(run).fetch_one(f.persistence.pool()).await.unwrap();
        let failure = GuardFailure {
            scope: ActivationRequest {
                company: company(f.company),
                run: RunId::new(run),
                execution: crate::domain::workflow::ExecutionId::new(scope.execution),
                job: WorkflowJobId(scope.job),
            },
            violation: WorkflowGuardViolation::from_constraint(
                "workflow_guard:escalation_route_test",
                "test failure",
            )
            .unwrap(),
            site: GuardSite::Step,
            fence: None,
        };
        assert_eq!(
            f.persistence.fail_run_on_guard(failure).await.unwrap(),
            GuardFailureOutcome::Failed
        );
        let worker = crate::adapters::persistence::test_support::notification_worker(
            &f.persistence,
            Default::default(),
        );
        assert_eq!(worker.drain_once().await.unwrap(), 1);
        assert_eq!(worker.drain_once().await.unwrap(), 1);
        let decision = sqlx::query_scalar(
            "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
        )
        .bind(f.company)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
        Self {
            f,
            decision,
            principal,
        }
    }
}

#[tokio::test]
async fn workflow_escalation_api_reads_current_authority_and_audits_exact_command_replay() {
    let s = Box::pin(Escalating::new()).await;
    for (user, scope) in [(s.f.stranger, s.f.company), (s.f.owner, s.f.other)] {
        let (status, _) =
            answer(escalations::read(s.f.state(), by(user), Path((scope, s.decision))).await).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, view) = answer(
        escalations::read(s.f.state(), by(s.f.owner), Path((s.f.company, s.decision))).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["can_acknowledge"], true);
    let post = |choice: Value| {
        escalations::submit(
            s.f.state(),
            by(s.f.owner),
            Path((s.f.company, s.decision)),
            body(json!({"command_key":"api-ack","choice":choice})),
        )
    };
    for replay in [false, true] {
        let (status, result) = answer(post(json!({"kind":"acknowledge"})).await).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["replayed"], replay);
    }
    let (status, _) = answer(post(json!({"kind":"reassign","principal":s.principal})).await).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, history) = answer(
        escalations::read(s.f.state(), by(s.f.owner), Path((s.f.company, s.decision))).await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history["command"]["actor_user"], s.f.owner.to_string());
    assert_eq!(history["command"]["choice"]["kind"], "acknowledge");
}

#[tokio::test]
async fn workflow_escalation_ui_submits_reassignment_and_acknowledgement_on_the_same_decision_surface()
 {
    let s = Box::pin(Escalating::new()).await;
    let (status, page) =
        answer(escalations::page(s.f.state(), by(s.f.owner), Path(s.decision)).await).await;
    assert_eq!(status, StatusCode::OK);
    let html = page.as_str().unwrap();
    assert!(html.contains("Review &lt;img") && !html.contains("Review <img"));
    assert!(
        html.contains("Saving…") && html.contains("hx-disabled-elt") && html.contains("Assign to")
    );
    let (status, result) = answer(
        escalations::ui_submit(
            s.f.state(),
            by(s.f.owner),
            Path(s.decision),
            Form(escalations::SubmitForm {
                command_key: "ui-assign".into(),
                choice: "reassign".into(),
                principal: Some(s.principal),
                expected_run_revision: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(result.as_str().unwrap().contains("View current assignment"));
    let next: Uuid = sqlx::query_scalar(
        "SELECT current_decision_id FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(s.f.company)
    .fetch_one(s.f.persistence.pool())
    .await
    .unwrap();
    let (status, result) = answer(
        escalations::ui_submit(
            s.f.state(),
            by(s.f.owner),
            Path(next),
            Form(escalations::SubmitForm {
                command_key: "ui-ack".into(),
                choice: "acknowledge".into(),
                principal: None,
                expected_run_revision: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(result.as_str().unwrap().contains("Acknowledged"));
    assert!(result.as_str().unwrap().contains("Command history"));
    let commands: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_commands WHERE company_id=$1")
            .bind(s.f.company)
            .fetch_one(s.f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(commands, 2);
}

/// Every escalation mutation and page route answers another tenant's decision as absent, and
/// leaves no command behind (`src/adapters/http/AGENTS.md`).
#[tokio::test]
async fn workflow_escalation_mutation_routes_treat_a_foreign_tenant_decision_as_absent() {
    let s = Box::pin(Escalating::new()).await;
    for (user, scope) in [(s.f.stranger, s.f.company), (s.f.owner, s.f.other)] {
        let (status, _) = answer(
            escalations::submit(
                s.f.state(),
                by(user),
                Path((scope, s.decision)),
                body(json!({"command_key":"foreign-ack","choice":{"kind":"acknowledge"}})),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "submit {user} {scope}");
    }
    let (status, _) =
        answer(escalations::page(s.f.state(), by(s.f.stranger), Path(s.decision)).await).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "page");
    for (key, choice, principal) in [
        ("foreign-ui-ack", "acknowledge", None),
        ("foreign-ui-assign", "reassign", Some(s.principal)),
    ] {
        let (status, _) = answer(
            escalations::ui_submit(
                s.f.state(),
                by(s.f.stranger),
                Path(s.decision),
                Form(escalations::SubmitForm {
                    command_key: key.into(),
                    choice: choice.into(),
                    principal,
                    expected_run_revision: None,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "ui_submit {choice}");
    }
    let commands: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_commands WHERE company_id=$1")
            .bind(s.f.company)
            .fetch_one(s.f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(commands, 0, "no foreign attempt recorded a command");
}
