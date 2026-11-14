//! Actual private root -> parked child -> authored recovery and authorized manual rerun.
use super::*;
use crate::application::app_error::WorkflowGuardViolation;
use crate::application::workflow::batch::{BatchBudget, WorkflowBatch};
use crate::application::workflow::children::WorkflowChildren;
use crate::application::workflow::guard::{
    GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
};
use crate::application::workflow::waits::{WaitProgress, WorkflowWaits};
use std::time::Duration;
#[path = "escalation_origin_cleanup_tests.rs"]
mod cleanup;

#[derive(Clone, Copy)]
enum SourceContext {
    Live,
    Retired,
}

fn leaf(schema: &Value) -> Value {
    json!({"format_version":1,"workflow_id":Uuid::nil(),"input_schema":true,"parameter_schema":true,
        "output_schema":schema,"resources":[],"entry":"start",
        "steps":{"start":{"type":"data.map","with":{"value":{"literal":{"body":"Child succeeded"}},
            "output_schema":{"literal":schema}},"routes":{"success":"$end"}}},
        "limits":{"max_steps":100,"max_context_bytes":65536}})
}

async fn configure_caller(f: &AgentCase) {
    own_default_desk(f).await;
    let p = f.people.role.p();
    let binding=WorkflowBindingId::new(sqlx::query_scalar("SELECT (result->>'binding_id')::uuid FROM workflow_provisioning_commands WHERE company_id=$1 AND command_key=$2")
        .bind(f.people.role.company().as_uuid()).bind(crate::adapters::persistence::company_agent::default_desk_key(f.created.agent.id))
        .fetch_one(p.pool()).await.unwrap());
    let state = p
        .binding(f.people.role.company(), binding)
        .await
        .unwrap()
        .unwrap();
    let schema = state
        .configuration
        .bundle()
        .compiled()
        .graph()
        .definition()
        .output_schema
        .clone()
        .unwrap();
    let fixture = &f.people.role.admission.binding.fixture;
    let child = WorkflowId::new(Uuid::new_v4());
    let child_version = fixture.publish_workflow(child, leaf(&schema)).await;
    let mut source = leaf(&schema);
    source["steps"]["start"] = json!({"type":"workflow.call","child_workflow_id":child,"child_version_id":child_version,
        "with":{"input":{"ref":"/input"}},"routes":{"success":"$end","error":"recover"}});
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":{"body":"Recovered"}},
        "output_schema":{"literal":schema}},"routes":{"success":"$end"}});
    let version = fixture
        .publish_workflow(WorkflowId::new(Uuid::new_v4()), source)
        .await;
    let mut request = f
        .people
        .role
        .admission
        .binding
        .request(Some(state.revision));
    request.target.binding = binding;
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.created.desk_id));
    request.version = version;
    request.params = json!({});
    request.resources = Default::default();
    request.message_trigger = Some(MessageTrigger::Any);
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    let configured = f
        .people
        .role
        .admission
        .binding
        .service()
        .configure(request)
        .await
        .unwrap();
    super::super::terminal_tests::select_handler_revision(f, configured.revision).await;
}

async fn root(f: &AgentCase) -> RunId {
    RunId::new(
        sqlx::query_scalar(
            "SELECT run_id FROM workflow_escalation_handling_roots WHERE company_id=$1",
        )
        .bind(f.people.role.company().as_uuid())
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap(),
    )
}

async fn active_scope(f: &AgentCase, run: RunId) -> ActivationRequest {
    let row: ScopeIds=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution \
        JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
        WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.completed_at IS NULL")
        .bind(f.people.role.company().as_uuid()).bind(run.as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    ActivationRequest {
        company: f.people.role.company(),
        run,
        execution: ExecutionId::new(row.execution),
        job: WorkflowJobId(row.job),
    }
}

async fn assert_origin(f: &AgentCase, run: RunId, expected: Uuid) {
    let actual: Uuid = sqlx::query_scalar(
        "SELECT origin_id FROM workflow_run_notification_origins WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(actual, expected);
}

async fn park(f: &AgentCase, parent: ActivationRequest) -> RunId {
    let p = f.people.role.p();
    let parked = p.park_child(parent).await.unwrap().unwrap();
    RunId::new(
        sqlx::query_scalar("SELECT child_run_id FROM workflow_waits WHERE company_id=$1 AND id=$2")
            .bind(f.people.role.company().as_uuid())
            .bind(parked.wait.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap(),
    )
}

async fn fail(f: &AgentCase, scope: ActivationRequest) {
    assert_eq!(
        f.people
            .role
            .p()
            .fail_run_on_guard(GuardFailure {
                scope,
                violation: WorkflowGuardViolation::from_constraint(
                    "workflow_guard:test_escalation_guard",
                    "forced guard"
                )
                .unwrap(),
                site: GuardSite::Step,
                fence: None,
            })
            .await
            .unwrap(),
        GuardFailureOutcome::Failed
    );
}

async fn rerun(f: &AgentCase, original: RunId) -> PreparedAdmission {
    #[derive(sqlx::FromRow)]
    struct Source {
        channel_id: Uuid,
        thread_id: Uuid,
        binding_id: Uuid,
        input: Value,
    }
    let row:Source=sqlx::query_as("SELECT channel_id,thread_id,binding_id,input FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.people.role.company().as_uuid()).bind(original.as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    let mut request = f
        .people
        .role
        .admission
        .request("marked-rerun", f.people.role.admission.manual());
    request.binding_id = WorkflowBindingId::new(row.binding_id);
    request.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(row.channel_id),
        thread_id: RelatedThreadId::new(row.thread_id),
    };
    request.input = row.input;
    request.rerun_of = Some(original);
    request.delivery_policy = crate::application::workflow::DeliveryPolicy::InAppOnly;
    f.people.role.admission.prepare(request).await
}

async fn child_recovery(
    f: &AgentCase,
    source: RunId,
    context: SourceContext,
    origin: Uuid,
) -> RunId {
    let p = f.people.role.p();
    let parent = root(f).await;
    assert_origin(f, parent, origin).await;
    let scope = active_scope(f, parent).await;
    let child = Box::pin(park(f, scope)).await;
    assert_origin(f, child, origin).await;
    if matches!(context, SourceContext::Retired) {
        retire(f, source).await;
    }
    fail(f, active_scope(f, child).await).await;
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    p.advance_pure(
        active_scope(f, parent).await,
        BatchBudget::new(4, Duration::from_secs(5)).unwrap(),
    )
    .await
    .unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.people.role.company().as_uuid())
            .bind(parent.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        state, "succeeded",
        "authored recovery kept the genuine handling root capable"
    );
    parent
}

async fn rerun_failure(f: &AgentCase, parent: RunId, context: SourceContext, origin: Uuid) {
    let p = f.people.role.p();
    let command = Box::pin(rerun(f, parent)).await;
    let AdmissionResult::Created(restarted) = p.admit(&command).await.unwrap() else {
        panic!("real authorized rerun was not created")
    };
    assert_eq!(
        p.admit(&command).await.unwrap(),
        AdmissionResult::Replayed(restarted)
    );
    assert_origin(f, restarted, origin).await;
    fail(f, active_scope(f, restarted).await).await;
    assert_eq!(counts(f).await["roots"], 1);
    let failures:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1 AND failure_kind='handling'")
        .bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        failures, 0,
        "neither descendant nor rerun may settle the intended root's recipient"
    );
    assert_eq!(
        counts(f).await["cases"],
        if matches!(context, SourceContext::Live) {
            1
        } else {
            0
        }
    );
    if matches!(context, SourceContext::Live) {
        let progress: String = sqlx::query_scalar(
            "SELECT state FROM workflow_escalation_progress WHERE company_id=$1",
        )
        .bind(f.people.role.company().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        assert_eq!(
            progress, "waiting",
            "root success is not an implicit acknowledgement"
        );
    }
}

async fn inherited(context: SourceContext) {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    Box::pin(configure_caller(&f)).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    process(&f, claims(&f).await.pop().unwrap()).await;
    let origin: Uuid = sqlx::query_scalar(
        "SELECT notice_id FROM workflow_escalation_notice_targets WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    // Each substantive storage phase is boxed to keep the stock test-thread stack.
    let parent = Box::pin(child_recovery(&f, source, context, origin)).await;
    Box::pin(rerun_failure(&f, parent, context, origin)).await;
}

#[tokio::test]
async fn workflow_escalation_origin_real_child_recovery_and_authorized_rerun_keep_origin_across_source_retention()
 {
    for context in [SourceContext::Live, SourceContext::Retired] {
        Box::pin(inherited(context)).await;
    }
}

#[tokio::test]
async fn workflow_escalation_origin_missing_child_or_rerun_seal_rolls_back_real_creation_then_retries()
 {
    let (f, _) = Box::pin(pending_agent_from_run()).await;
    Box::pin(configure_caller(&f)).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    process(&f, claims(&f).await.pop().unwrap()).await;
    let parent = root(&f).await;
    let scope = active_scope(&f, parent).await;
    let origin: Uuid = sqlx::query_scalar(
        "SELECT notice_id FROM workflow_escalation_notice_targets WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    sqlx::raw_sql("CREATE FUNCTION omit_inherited_origin() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        IF EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=NEW.company_id AND id=NEW.run_id \
            AND (parent_run_id IS NOT NULL OR rerun_of_run_id IS NOT NULL)) THEN RETURN NULL; END IF; RETURN NEW; END $$; \
        CREATE TRIGGER aa_omit_inherited_origin BEFORE INSERT ON workflow_run_notification_origins \
          FOR EACH ROW EXECUTE FUNCTION omit_inherited_origin();")
        .execute(p.pool()).await.unwrap();
    let error = p.park_child(scope).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("child and rerun must seal"),
        "{error:?}"
    );
    assert_eq!(counts(&f).await["runs"], 2);
    sqlx::query("DROP TRIGGER aa_omit_inherited_origin ON workflow_run_notification_origins")
        .execute(p.pool())
        .await
        .unwrap();
    let child = Box::pin(park(&f, scope)).await;
    assert_origin(&f, child, origin).await;
    p.advance_pure(
        active_scope(&f, child).await,
        BatchBudget::new(4, Duration::from_secs(5)).unwrap(),
    )
    .await
    .unwrap();
    assert!(matches!(
        p.resume_wait(scope).await.unwrap(),
        WaitProgress::Completed(_)
    ));
    let command = Box::pin(rerun(&f, parent)).await;
    sqlx::query("CREATE TRIGGER aa_omit_inherited_origin BEFORE INSERT ON workflow_run_notification_origins FOR EACH ROW EXECUTE FUNCTION omit_inherited_origin()")
        .execute(p.pool()).await.unwrap();
    let error = p.admit(&command).await.unwrap_err();
    assert!(
        format!("{error:?}").contains("child and rerun must seal"),
        "{error:?}"
    );
    assert_eq!(counts(&f).await["runs"], 3);
    sqlx::query("DROP TRIGGER aa_omit_inherited_origin ON workflow_run_notification_origins")
        .execute(p.pool())
        .await
        .unwrap();
    let AdmissionResult::Created(restarted) = p.admit(&command).await.unwrap() else {
        panic!("rerun retry was not created")
    };
    assert_origin(&f, restarted, origin).await;
    assert_eq!(counts(&f).await["roots"], 1);
}
