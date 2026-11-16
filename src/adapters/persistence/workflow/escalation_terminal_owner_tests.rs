//! D1.3 terminal owners beyond the guard, deadline and recovery writers: a failed wait, a child
//! failure propagated to its root parent, and the parked effect and decision owners. Each starts
//! the default ladder exactly when no authored route consumes the failure.
use super::*;
use crate::application::workflow::batch::{BatchBudget, WorkflowBatch};
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};

/// A root parked on a timer whose wake can no longer activate its successor.
async fn timer_root() -> GuardFixture {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("wait.timer").unwrap().source).unwrap();
    source["input_schema"] = json!(true);
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},
        "output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    let version = republish(&people, source).await;
    let f = Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await;
    assert!(f.p().park_wait(f.scope).await.unwrap().is_some());
    sqlx::query(
        "UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds', \
         deadline=clock_timestamp()-interval '1 second' WHERE company_id=$1 AND run_id=$2 AND state='waiting'",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .execute(f.p().pool())
    .await
    .unwrap();
    // The run's own activation allowance is spent, so the wake fails the wait.
    sqlx::query("UPDATE workflow_runs SET max_steps=1 WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(f.p().pool())
        .await
        .unwrap();
    f
}

async fn case_source(f: &GuardFixture) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_agg(jsonb_build_object('run',run_id,'step',step_id,'reason',reason_code)) \
         FROM workflow_escalation_cases WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_escalation_trigger_wait_failure_by_the_real_sweeper_starts_one_case() {
    let f = Box::pin(timer_root()).await;
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 1);
    let failure: Option<String> = sqlx::query_scalar(
        "SELECT failure_code FROM workflow_waits WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    assert_eq!(failure.as_deref(), Some("activation_limit"));
    assert_eq!(
        case_source(&f).await,
        json!([{"run":f.scope.run.as_uuid(),"step":"start","reason":"workflow.activation_limit"}])
    );
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 0);
    assert_eq!(f.worker().drain_once().await.unwrap(), 1);
    assert_eq!(
        f.state().await,
        json!({"state":"failed","runs":1,"cases":1,"controls":2,"hops":1,"notices":1,"guard_events":0}),
        "one case whose real control delivered its first hop, and a replayed sweep adds nothing"
    );
}

/// A root calling a child, with or without an authored error route for the child's failure.
async fn calling(routed: bool) -> GuardFixture {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let schema = json!({"type":"integer"});
    let leaf = json!({"format_version":1,"workflow_id":Uuid::nil(),"input_schema":true,
        "parameter_schema":true,"output_schema":schema,"resources":[],"entry":"start",
        "steps":{"start":{"type":"data.map","with":{"value":{"literal":1},
            "output_schema":{"literal":schema}},"routes":{"success":"$end"}}},
        "limits":{"max_steps":100,"max_context_bytes":65536}});
    let fixture = &people.role.admission.binding.fixture;
    let child = WorkflowId::new(Uuid::new_v4());
    let child_version = fixture.publish_workflow(child, leaf.clone()).await;
    let mut source = leaf;
    source["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":child,
        "child_version_id":child_version,"with":{"input":{"ref":"/input"}},"routes":{"success":"$end"}});
    if routed {
        source["steps"]["start"]["routes"]["error"] = json!("recover");
        source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":2},
            "output_schema":{"literal":schema}},"routes":{"success":"$end"}});
    }
    let version = republish(&people, source).await;
    Box::pin(GuardFixture::admit(people, Some(policy()), Some(version))).await
}

async fn active(f: &GuardFixture, run: RunId) -> ActivationRequest {
    let (execution, job): (Uuid, Uuid) = sqlx::query_as(
        "SELECT execution.id, job.id FROM workflow_executions AS execution \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.completed_at IS NULL",
    )
    .bind(f.scope.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.p().pool())
    .await
    .unwrap();
    ActivationRequest {
        company: f.scope.company,
        run,
        execution: crate::domain::workflow::ExecutionId::new(execution),
        job: crate::application::workflow::activation::WorkflowJobId(job),
    }
}

/// Park the root on its child and fail the child through the real guard owner.
async fn failed_child(f: &GuardFixture) -> RunId {
    let parked = f.p().park_child(f.scope).await.unwrap().unwrap();
    let child = RunId::new(
        sqlx::query_scalar("SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(parked.wait.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap(),
    );
    let mut failure = f.failure();
    failure.scope = active(f, child).await;
    assert_eq!(
        f.p().fail_run_on_guard(failure).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        f.state().await["cases"],
        0,
        "a child never escalates itself; its parent's propagation decides"
    );
    child
}

#[tokio::test]
async fn workflow_escalation_trigger_unrouted_child_failure_escalates_once_at_the_root_parent() {
    let f = Box::pin(calling(false)).await;
    let child = Box::pin(failed_child(&f)).await;
    assert!(matches!(
        f.p().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    assert_eq!(f.state().await["state"], "failed");
    assert_eq!(
        case_source(&f).await,
        json!([{"run":f.scope.run.as_uuid(),"step":"start","reason":"workflow.child_failed"}]),
        "exactly one case, at the root parent and not the child {child:?}"
    );
    assert_eq!(f.state().await["controls"], 1);
    assert!(matches!(
        f.p().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Completed(_) | WaitProgress::Pending
    ));
    assert_eq!(
        f.state().await["cases"],
        1,
        "a replayed propagation adds nothing"
    );
}

#[tokio::test]
async fn workflow_escalation_trigger_authored_parent_route_consumes_child_failure_without_a_case() {
    let f = Box::pin(calling(true)).await;
    Box::pin(failed_child(&f)).await;
    assert!(matches!(
        f.p().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    let recover = active(&f, f.scope.run).await;
    f.p()
        .advance_pure(
            recover,
            BatchBudget::new(4, std::time::Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(f.state().await["state"], "succeeded");
    assert_eq!(f.state().await["cases"], 0);
}

/// The case guard proves every new owner's typed reason against the owner's own audit, so no
/// owner transaction can abort on its escalation and no forged reason is accepted. The
/// decision-bound kinds (token allowance and action approval refusals) cannot be fabricated here
/// at all: `workflow_approval_event_kind` admits them only with their owning decision.
#[tokio::test]
async fn workflow_escalation_source_proves_each_parked_owner_reason_from_its_audit() {
    let f = Box::pin(timer_root()).await;
    assert_eq!(f.p().sweep_waits(128).await.unwrap(), 1);
    let accepted = [
        ("message_delivery_failed", "message.delivery_failed"),
        ("memory_delivery_failed", "memory.save_expired"),
        ("memory_delivery_failed", "memory.save_failed"),
        ("workflow.child_failed", "workflow.child_failed"),
        ("workflow.child_cancelled", "workflow.child_cancelled"),
        (
            "message.final_response_refused",
            "message.final_response_refused",
        ),
        ("local_agent.refused", "local_agent.refused"),
    ];
    let refused = [
        ("message_delivery_failed", "message.final_response_refused"),
        ("memory_delivery_failed", "workflow.invalid_output"),
        ("workflow.child_failed", "workflow.child_cancelled"),
        ("message.final_response_refused", "message.delivery_failed"),
    ];
    for (audit, reason, valid) in accepted
        .iter()
        .map(|(audit, reason)| (*audit, *reason, true))
        .chain(
            refused
                .iter()
                .map(|(audit, reason)| (*audit, *reason, false)),
        )
    {
        // The run's real case already exists; the guard is a BEFORE INSERT trigger, so it still
        // judges every candidate before the conflict leaves it unwritten.
        let mut tx = f.p().pool().begin().await.unwrap();
        sqlx::query(
            "INSERT INTO workflow_run_events(company_id,run_id,sequence,event_kind,actor_id,execution_id) \
             SELECT run.company_id,run.id,(SELECT max(sequence)+1 FROM workflow_run_events WHERE company_id=$1 AND run_id=$2), \
                    $4,run.actor_id,$3 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .bind(audit)
        .execute(&mut *tx)
        .await
        .unwrap();
        let result = sqlx::query(
            "INSERT INTO workflow_escalation_cases \
             (company_id,id,source_kind,run_id,execution_id,binding_id,binding_revision, \
              origin_department_id,policy,reason_code,step_id) \
             SELECT run.company_id,gen_random_uuid(),'run',run.id,$3,run.binding_id,run.binding_revision, \
                    manifest.role_context_department_id,run.escalation,$4,'start' \
             FROM workflow_runs AS run JOIN run_dependency_manifests AS ownership \
               ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
             JOIN agent_dependency_manifests AS manifest ON manifest.company_id=ownership.company_id AND manifest.id=ownership.manifest_id \
             WHERE run.company_id=$1 AND run.id=$2 \
             ON CONFLICT (company_id,run_id) WHERE run_id IS NOT NULL DO NOTHING",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .bind(f.scope.execution.as_uuid())
        .bind(reason)
        .execute(&mut *tx)
        .await;
        match (valid, result) {
            (true, Ok(_)) => {}
            (false, Err(error)) => assert_eq!(
                error.as_database_error().and_then(|e| e.constraint()),
                Some("workflow_escalation_case_guard"),
                "{audit}/{reason}: {error}"
            ),
            (valid, result) => panic!("{audit}/{reason}: expected valid={valid}, got {result:?}"),
        }
        tx.rollback().await.unwrap();
    }
}
