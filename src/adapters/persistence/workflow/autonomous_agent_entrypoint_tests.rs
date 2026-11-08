use super::*;
use crate::adapters::persistence::workflow::schedule_test_support::schedule_use_cases;
use crate::application::workflow::{
    manual::{self, StartRun},
    message_admission::{MessageAdmissions, MessageIntentWorkerId, WorkflowAdmitter},
    schedule_admission::{ScheduleMaterializer, ScheduleWorkerId},
};
use crate::domain::entities::schedule::{ScheduleTimezone, ScheduleType, ScheduleWrite};

#[path = "agent_history_entrypoint_tests.rs"]
mod history;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct SelectedRun {
    manifest: Uuid,
    revision: Uuid,
    version: Uuid,
    state: String,
}

async fn selected(d: &Deployment, run: RunId) -> SelectedRun {
    sqlx::query_as("SELECT ownership.manifest_id AS manifest,selection.revision_id AS revision,run.version_id AS version,run.state FROM workflow_runs AS run JOIN run_dependency_manifests AS ownership ON ownership.company_id=run.company_id AND ownership.run_id=run.id JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(d.p().pool()).await.unwrap()
}

async fn publish_next(d: &Deployment) -> Uuid {
    let agent: Uuid = sqlx::query_scalar("UPDATE agents SET system_prompt='New independently published entrypoint behavior' WHERE company_id=$1 AND slug='assistant' RETURNING id")
        .bind(d.fixture.target.company.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    d.fixture.publish_agent(agent).await.id.as_uuid()
}

fn assert_new_selection(old: &SelectedRun, new: &SelectedRun, published: Uuid) {
    assert_eq!(old.state, "queued");
    assert_eq!(new.state, "queued");
    assert_ne!(old.manifest, new.manifest);
    assert_ne!(old.revision, new.revision);
    assert_eq!(new.revision, published);
    assert_eq!(old.version, new.version, "the workflow was not republished");
}

fn model_source() -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object"});
    source
}

fn manual_request(d: &Deployment, binding: BindingTarget, key: &str) -> StartRun {
    StartRun {
        history_session: None,
        company_id: d.fixture.target.company,
        actor: d.fixture.target.actor,
        binding_id: binding.binding,
        association: RelatedAssociation::Channel(RelatedChannelId::new(d.channel)),
        input: json!({}),
        idempotency_key: key.into(),
        rerun_of: None,
    }
}

#[tokio::test]
async fn autonomous_agent_entrypoints_manual_queues_exact_selection_and_replays_after_publication()
{
    let d = Deployment::new(vec![]).await;
    let binding = d.bind(model_source(), None).await;
    let service = d.engine.services().service();
    let first = manual::start(&service, manual_request(&d, binding, "manual-v1"))
        .await
        .unwrap();
    let old = selected(&d, first.run).await;
    let published = publish_next(&d).await;
    let second = manual::start(&service, manual_request(&d, binding, "manual-v2"))
        .await
        .unwrap();
    assert_new_selection(&old, &selected(&d, second.run).await, published);
    let replay = manual::start(&service, manual_request(&d, binding, "manual-v1"))
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.run, first.run);
    assert_eq!(selected(&d, first.run).await, old);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    assert_eq!(
        d.llm.finish().await.unwrap(),
        0,
        "selection precedes worker/provider entry"
    );
}

async fn admit_message(d: &Deployment, key: &str, reply_to: Option<&str>) -> RunId {
    let ingested = d.email(customer_email(d, key, reply_to)).await;
    assert_eq!(ingested.intent_ids.len(), 1);
    let admissions =
        MessageAdmissions::new(d.p().clone(), d.engine.services().service(), d.p().clone());
    let claimed = admissions
        .claim(MessageIntentWorkerId(Uuid::new_v4()), 4)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    admissions.process(claimed[0].clone()).await.unwrap();
    admissions.process(claimed[0].clone()).await.unwrap();
    let run: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND trigger_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(ingested.intent_ids[0])
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    RunId::new(run)
}

#[tokio::test]
async fn autonomous_agent_entrypoints_messages_select_at_materialization_and_duplicate_processing_keeps_one_run()
 {
    let d = Deployment::new(vec![]).await;
    d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let first = admit_message(&d, "entry-first", None).await;
    let old = selected(&d, first).await;
    let published = publish_next(&d).await;
    let second = admit_message(&d, "entry-second", Some("entry-first")).await;
    assert_new_selection(&old, &selected(&d, second).await, published);
    assert_eq!(selected(&d, first).await, old);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

struct CommitThenCrash(crate::infra::workflow::ProductionWorkflowService);
#[async_trait]
impl WorkflowAdmitter for CommitThenCrash {
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        self.0.admit(request).await?;
        Err(AppError::Database("injected post-admission crash".into()))
    }
}

async fn schedule(d: &Deployment, binding: BindingTarget) -> Uuid {
    let schedule = schedule_use_cases(d.p())
        .create_schedule(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            d.channel,
            ScheduleWrite {
                name: "Entry report".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                timezone: ScheduleTimezone::utc(),
                binding_id: binding.binding.as_uuid(),
                enabled: true,
            },
        )
        .await
        .unwrap();
    due(d, schedule.id).await;
    schedule.id
}

async fn due(d: &Deployment, schedule: Uuid) {
    sqlx::query("UPDATE channel_schedules SET next_run_at=clock_timestamp()-interval '1 minute' WHERE company_id=$1 AND id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(schedule).execute(d.p().pool()).await.unwrap();
}

async fn scheduled_run(d: &Deployment, occurrence: Uuid) -> RunId {
    let run: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND trigger_id=$2")
            .bind(d.fixture.target.company.as_uuid())
            .bind(occurrence)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    RunId::new(run)
}

#[tokio::test]
async fn autonomous_agent_entrypoints_schedule_post_commit_crash_keeps_recorded_revision_and_next_occurrence_selects_new()
 {
    let d = Deployment::new(vec![]).await;
    let binding = d
        .bind(super::schedule_runtime_tests::reporting_source(), None)
        .await;
    let schedule = schedule(&d, binding).await;
    let crashing = ScheduleMaterializer::new(
        d.p().clone(),
        CommitThenCrash(d.engine.services().service()),
    );
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = crashing.claim(worker).await.unwrap();
    assert_eq!(claimed.len(), 1);
    crashing.materialize(worker, &claimed[0]).await.unwrap();
    let first = scheduled_run(&d, claimed[0].id).await;
    let old = selected(&d, first).await;
    let published = publish_next(&d).await;
    let recovered = ScheduleMaterializer::new(d.p().clone(), d.engine.services().service());
    assert!(
        recovered.claim(worker).await.unwrap().is_empty(),
        "admission atomically settles the occurrence before a post-commit crash"
    );
    let recorded: Value = sqlx::query_scalar("SELECT jsonb_build_object('run',run_id,'status',materialization_status) FROM schedule_runs WHERE company_id=$1 AND id=$2")
        .bind(d.fixture.target.company.as_uuid()).bind(claimed[0].id).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(
        recorded,
        json!({"run":first.as_uuid(),"status":"materialized"})
    );
    assert_eq!(scheduled_run(&d, claimed[0].id).await, first);
    assert_eq!(selected(&d, first).await, old);
    due(&d, schedule).await;
    let next = recovered.claim(worker).await.unwrap();
    assert_eq!(next.len(), 1);
    assert_ne!(next[0].id, claimed[0].id);
    recovered.materialize(worker, &next[0]).await.unwrap();
    let second = scheduled_run(&d, next[0].id).await;
    assert_new_selection(&old, &selected(&d, second).await, published);
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        2
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}
