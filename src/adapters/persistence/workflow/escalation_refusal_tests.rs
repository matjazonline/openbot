use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};
use crate::entities::department::RoleAdmissionRefusal;

#[path = "escalation_control_tests.rs"]
mod controls;

async fn fixture() -> RoleFixture {
    let f = Box::pin(RoleFixture::new()).await;
    let binding = &f.admission.binding;
    let state = binding.state().await;
    let mut request = binding.request(Some(state.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(2).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    f
}

async fn clear(f: &RoleFixture) {
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .clear_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
            DepartmentRole::Lead,
            f.principal,
        )
        .await
        .unwrap();
}

fn assert_refused(result: AppResult<AdmissionResult>) {
    assert!(
        matches!(result,Err(AppError::WorkflowRole(RoleAdmissionRefusal {
        kind:RoleRefusalKind::NoHolder,role:DepartmentRole::Lead,step,..
    })) if step.as_str()=="start")
    );
}

async fn count(f: &RoleFixture, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
        .bind(f.company().as_uuid())
        .fetch_one(f.p().pool())
        .await
        .unwrap()
}

async fn assert_one_refusal(f: &RoleFixture) {
    for table in [
        "workflow_admission_refusals",
        "workflow_escalation_cases",
        "workflow_escalation_control_events",
        "notification_events",
    ] {
        assert_eq!(count(f, table).await, 1, "{table}");
    }
    for table in [
        "workflow_runs",
        "agent_dependency_manifests",
        "workflow_run_role_bindings",
    ] {
        assert_eq!(count(f, table).await, 0, "no synthetic {table}");
    }
    let case:Value=sqlx::query_scalar("SELECT jsonb_build_object('source',source_kind,'run',run_id, \
        'policy',policy,'department',origin_department_id,'code',reason_code,'step',step_id,'role',role) \
        FROM workflow_escalation_cases WHERE company_id=$1")
        .bind(f.company().as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(
        case,
        json!({"source":"admission","run":null,"policy":{"notify":"reviewer","hops":2},
        "department":f.department,"code":"workflow.role_no_holder","step":"start","role":"lead"})
    );
}

#[tokio::test]
async fn workflow_escalation_refusal_commits_once_and_replay_does_not_admit_after_role_repair() {
    let f = Box::pin(fixture()).await;
    let command = f.prepared("refused").await;
    clear(&f).await;
    assert_refused(f.p().admit(&command).await);
    assert_one_refusal(&f).await;
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .assign_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
            DepartmentRole::Lead,
            f.principal,
        )
        .await
        .unwrap();
    assert_refused(f.p().admit(&command).await);
    assert_one_refusal(&f).await;
    let fresh = f.prepared("new-source").await;
    assert!(matches!(
        f.p().admit(&fresh).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    assert_eq!(count(&f, "workflow_runs").await, 1);
    assert_eq!(count(&f, "workflow_escalation_cases").await, 1);
}

#[tokio::test]
async fn workflow_escalation_refusal_command_payload_conflicts_and_source_alias_replays() {
    let f = Box::pin(fixture()).await;
    let trigger = f.admission.manual();
    let request = |key: &str, value: i64| {
        let mut request = f.admission.request(key, trigger.clone());
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.channel));
        request.input = json!({"value":value});
        request
    };
    let command = f.admission.prepare(request("canonical", 1)).await;
    clear(&f).await;
    assert_refused(f.p().admit(&command).await);
    let changed = f.admission.prepare(request("canonical", 2)).await;
    assert_eq!(
        f.p().admit(&changed).await.unwrap(),
        AdmissionResult::Conflict
    );
    let alias = f.admission.prepare(request("source-alias", 1)).await;
    assert_refused(f.p().admit(&alias).await);
    assert_refused(f.p().admit(&alias).await);
    let mut wrong_source = request("source-alias", 1);
    wrong_source.trigger = f.admission.manual();
    let error = WorkflowService::new(
        f.p().clone(),
        f.p().clone(),
        NoRuntime,
        NoRuntime,
        Preflight,
    )
    .admit(wrong_source)
    .await
    .unwrap_err();
    assert!(matches!(error, AppError::Conflict(_)), "{error:?}");
    assert_one_refusal(&f).await;
    assert_eq!(count(&f, "workflow_admission_commands").await, 2);
}

#[tokio::test]
async fn workflow_escalation_refusal_competing_real_admissions_commit_one_case_and_control() {
    let f = Box::pin(fixture()).await;
    let command = f.prepared("competing").await;
    clear(&f).await;
    let barrier = tokio::sync::Barrier::new(3);
    let contender = || async {
        barrier.wait().await;
        f.p().admit(&command).await
    };
    let (first, second, ()) = tokio::join!(contender(), contender(), async {
        barrier.wait().await;
    });
    assert_refused(first);
    assert_refused(second);
    assert_one_refusal(&f).await;
}

#[tokio::test]
async fn workflow_escalation_refusal_case_write_failure_rolls_back_the_authoritative_outcome() {
    let f = Box::pin(fixture()).await;
    let command = f.prepared("rollback").await;
    clear(&f).await;
    sqlx::raw_sql(
        "CREATE FUNCTION refuse_escalation_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN PERFORM 1/0; RETURN NEW; END $$; \
        CREATE TRIGGER refuse_escalation_test BEFORE INSERT ON workflow_escalation_cases \
        FOR EACH ROW EXECUTE FUNCTION refuse_escalation_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    let error = f.p().admit(&command).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    for table in [
        "workflow_admission_refusals",
        "workflow_admission_commands",
        "workflow_escalation_cases",
        "notification_events",
        "workflow_runs",
    ] {
        assert_eq!(count(&f, table).await, 0, "{table} must roll back");
    }
    sqlx::raw_sql(
        "DROP TRIGGER refuse_escalation_test ON workflow_escalation_cases; \
        DROP FUNCTION refuse_escalation_test()",
    )
    .execute(f.p().pool())
    .await
    .unwrap();
    assert_refused(f.p().admit(&command).await);
    assert_one_refusal(&f).await;
}

#[tokio::test]
async fn workflow_escalation_refusal_history_and_policy_are_immutable_and_no_policy_creates_no_case()
 {
    let f = Box::pin(fixture()).await;
    let command = f.prepared("immutable").await;
    clear(&f).await;
    assert_refused(f.p().admit(&command).await);
    for statement in [
        "UPDATE workflow_admission_refusals SET request_fingerprint=repeat('0',64) WHERE company_id=$1",
        "DELETE FROM workflow_admission_refusals WHERE company_id=$1",
        "UPDATE workflow_escalation_cases SET reason_code='changed.reason' WHERE company_id=$1",
        "DELETE FROM workflow_escalation_cases WHERE company_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(f.company().as_uuid())
            .execute(f.p().pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    let unconfigured = Box::pin(RoleFixture::new()).await;
    let command = unconfigured.prepared("no-policy").await;
    clear(&unconfigured).await;
    assert_refused(unconfigured.p().admit(&command).await);
    assert_eq!(count(&unconfigured, "workflow_admission_refusals").await, 1);
    assert_eq!(count(&unconfigured, "workflow_escalation_cases").await, 0);
    assert_eq!(count(&unconfigured, "notification_events").await, 0);
}
