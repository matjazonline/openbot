//! The actual I/O retirement transaction owns the case, after final-error routing.
use super::*;
use crate::application::workflow::lease::{
    ClaimedWorkflow, LeasePolicy, LeaseReleaseCause, WorkflowFailure, WorkflowLeases,
    WorkflowWorkerId,
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
use std::time::Duration;

fn lease_policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}

fn failure(class: FailureClass, safety: RetrySafety) -> LeaseReleaseCause {
    failure_code(
        class,
        safety,
        FailureCode::parse("provider.rejected").unwrap(),
    )
}

fn failure_code(class: FailureClass, safety: RetrySafety, code: FailureCode) -> LeaseReleaseCause {
    LeaseReleaseCause::Classified(WorkflowFailure {
        failure: StepFailure::new(class, code, None).unwrap(),
        safety,
    })
}

#[tokio::test]
async fn workflow_escalation_recovery_source_uses_the_actual_last_retired_attempt_not_old_history()
{
    let f = Box::pin(fixture(None)).await;
    let first = claim(&f).await;
    assert!(
        f.p()
            .release_io(
                first.fence,
                lease_policy(),
                failure_code(
                    FailureClass::Retryable,
                    RetrySafety::SafeToRetry,
                    FailureCode::parse("provider.older").unwrap(),
                )
            )
            .await
            .unwrap()
    );
    assert_eq!(f.state().await["cases"], 0);
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp() WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.job.0)
    .execute(f.p().pool())
    .await
    .unwrap();
    let last = claim(&f).await;
    assert_eq!(last.fence.attempt.0, first.fence.attempt.0 + 1);
    assert!(
        f.p()
            .release_io(
                last.fence,
                lease_policy(),
                failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
            )
            .await
            .unwrap()
    );
    let next: i32 = sqlx::query_scalar("SELECT attempt_ordinal FROM background_tasks WHERE id=$1")
        .bind(f.scope.job.0)
        .fetch_one(f.p().pool())
        .await
        .unwrap();
    assert_eq!(next, last.fence.attempt.0 + 1);
    for (code, valid) in [("provider.older", false), ("provider.rejected", true)] {
        let actual: bool =
            sqlx::query_scalar("SELECT workflow_escalation_run_source_valid($1,$2,$3,$4,'start')")
                .bind(f.scope.company.as_uuid())
                .bind(f.scope.run.as_uuid())
                .bind(f.scope.execution.as_uuid())
                .bind(code)
                .fetch_one(f.p().pool())
                .await
                .unwrap();
        assert_eq!(actual, valid, "{code}");
    }
    assert_eq!(f.state().await["cases"], 1);
}

async fn fixture(target: Option<&str>) -> GuardFixture {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("context.history").unwrap().source).unwrap();
    source["input_schema"] = json!(true);
    source["output_schema"] = json!(true);
    source["steps"]["start"]["routes"]
        .as_object_mut()
        .unwrap()
        .remove("error");
    if let Some(target) = target {
        source["steps"]["start"]["routes"]["error"] = json!(target);
    }
    if target == Some("recover") {
        source["steps"]["recover"] = json!({"type":"data.map","with":{
            "value":{"literal":42},"output_schema":{"literal":{"type":"integer"}}},
            "routes":{"success":"$end"}});
    }
    let version = republish(&people, source).await;
    Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await
}

async fn claim(f: &GuardFixture) -> ClaimedWorkflow {
    f.p()
        .claim_io(f.scope, WorkflowWorkerId(Uuid::new_v4()), lease_policy())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn workflow_escalation_recovery_actual_terminal_attempt_creates_one_case_and_notice() {
    let f = Box::pin(fixture(None)).await;
    let owned = claim(&f).await;
    assert!(
        f.p()
            .release_io(
                owned.fence,
                lease_policy(),
                failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
            )
            .await
            .unwrap()
    );
    assert_eq!(f.state().await["state"], "failed");
    assert_eq!(f.state().await["cases"], 1);
    let code: String =
        sqlx::query_scalar("SELECT reason_code FROM workflow_escalation_cases WHERE company_id=$1")
            .bind(f.scope.company.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert_eq!(code, "provider.rejected");
    assert!(
        !f.p()
            .release_io(
                owned.fence,
                lease_policy(),
                failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
            )
            .await
            .unwrap()
    );
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    assert_eq!(f.state().await["cases"], 1);
    assert_eq!(f.state().await["notices"], 1);
}

#[tokio::test]
async fn workflow_escalation_recovery_case_failure_rolls_back_actual_attempt_retirement() {
    let f = Box::pin(fixture(None)).await;
    let owned = claim(&f).await;
    let before = f.state().await;
    sqlx::raw_sql(
        "CREATE FUNCTION reject_recovery_case() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN PERFORM 1/0; RETURN NEW; END $$; \
        CREATE TRIGGER reject_recovery_case BEFORE INSERT ON workflow_escalation_cases \
        FOR EACH ROW EXECUTE FUNCTION reject_recovery_case()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let error = f
        .p()
        .release_io(
            owned.fence,
            lease_policy(),
            failure(FailureClass::Terminal, RetrySafety::SafeToRetry),
        )
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert_eq!(f.state().await, before);
    assert!(
        f.p()
            .validate_io(owned.fence, lease_policy())
            .await
            .unwrap()
    );
    sqlx::query("DROP TRIGGER reject_recovery_case ON workflow_escalation_cases")
        .execute(f.p().pool())
        .await
        .unwrap();
    assert!(
        f.p()
            .release_io(
                owned.fence,
                lease_policy(),
                failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
            )
            .await
            .unwrap()
    );
    assert_eq!(f.state().await["cases"], 1);
}

#[tokio::test]
async fn workflow_escalation_recovery_authored_handlers_do_not_start_an_automatic_ladder() {
    for target in ["$end", "recover"] {
        let f = Box::pin(fixture(Some(target))).await;
        let owned = claim(&f).await;
        assert!(
            f.p()
                .release_io(
                    owned.fence,
                    lease_policy(),
                    failure(FailureClass::Terminal, RetrySafety::SafeToRetry)
                )
                .await
                .unwrap()
        );
        let route: String = sqlx::query_scalar(
            "SELECT committed_route FROM workflow_executions WHERE company_id=$1 AND id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap();
        assert_eq!(route, "final_error");
        assert_eq!(f.state().await["cases"], 0);
        assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    }
}

#[tokio::test]
async fn workflow_escalation_recovery_retry_and_unknown_effect_do_not_claim_terminal_failure() {
    for safety in [RetrySafety::SafeToRetry, RetrySafety::EffectOutcomeUnknown] {
        let f = Box::pin(fixture(None)).await;
        let owned = claim(&f).await;
        assert!(
            f.p()
                .release_io(
                    owned.fence,
                    lease_policy(),
                    failure(FailureClass::Retryable, safety)
                )
                .await
                .unwrap()
        );
        assert_eq!(f.state().await["cases"], 0);
        assert_eq!(
            f.state().await["state"],
            if safety == RetrySafety::SafeToRetry {
                "running"
            } else {
                "waiting"
            }
        );
        assert_eq!(f.worker().drain_once().await.unwrap(), 0);
    }
}
