//! p08 D1.9 end to end through the production composition: a due schedule slot becomes one run
//! of its binding, which the scripted agent answers once, also when the first materialization
//! crashed after admitting it.
use super::*;
use crate::adapters::persistence::workflow::schedule_test_support::schedule_use_cases;
use crate::application::workflow::message_admission::WorkflowAdmitter;
use crate::application::workflow::schedule_admission::{
    Materialized, ScheduleMaterializer, ScheduleWorkerId,
};
use crate::domain::entities::schedule::{ScheduleTimezone, ScheduleType, ScheduleWrite};

/// Admission that commits, then fails as a materializer that crashed before recording it would.
struct CrashAfterAdmission<A>(A);

#[async_trait]
impl<A: WorkflowAdmitter> WorkflowAdmitter for CrashAfterAdmission<A> {
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        self.0.admit(request).await?;
        Err(AppError::Database("scripted crash after admission".into()))
    }
}

/// An agent writes the report for the slot that came due.
fn reporting_source() -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": {
            "type": "object",
            "properties": {"scheduled_for": {"type": "string", "maxLength": 64}},
            "required": ["scheduled_for"],
            "additionalProperties": false
        },
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "report",
        "steps": {
            "report": {"type": "agent.run", "with": {
                "agent": {"literal": "assistant"},
                "context": {"object": {"scheduled_for": {"ref": "/input/scheduled_for"}}},
                "output_schema": {"literal": {"type": "string", "maxLength": 128}}},
                "routes": {"success": "$end"}}
        },
        "limits": {"max_steps": 100, "max_context_bytes": 65536}
    })
}

/// F011.4: a scheduled run driven through the composed engine and agent runtime; and a crash
/// after admission retries into the same run, so the agent is called once.
#[tokio::test]
async fn schedule_end_to_end_a_due_slot_runs_its_workflow_once() {
    let d = Deployment::new(vec![LlmTurn::text("\"Report ready.\"")]).await;
    let binding = d.bind(reporting_source(), None).await;
    let owner = d.fixture.target.actor.user_id();
    let company = d.fixture.target.company.as_uuid();
    let schedule = schedule_use_cases(d.p())
        .create_schedule(
            owner,
            company,
            d.channel,
            ScheduleWrite {
                name: "Morning report".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(86_400),
                scheduled_at: None,
                timezone: ScheduleTimezone::utc(),
                binding_id: binding.binding.as_uuid(),
                enabled: true,
            },
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE channel_schedules SET next_run_at = clock_timestamp() - interval '1 minute' WHERE id = $1",
    )
    .bind(schedule.id)
    .execute(d.p().pool())
    .await
    .unwrap();

    // The first materializer admits the run, then crashes before recording it.
    let crashing =
        ScheduleMaterializer::new(d.p().clone(), CrashAfterAdmission(d.engine.service()));
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = crashing.claim(worker).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        crashing.materialize(worker, &claimed[0]).await.unwrap(),
        Materialized::LeaseLost
    );
    let occurrence = claimed[0].id;
    sqlx::query(
        "UPDATE schedule_runs SET materialization_available_at = clock_timestamp() WHERE id = $1",
    )
    .bind(occurrence)
    .execute(d.p().pool())
    .await
    .unwrap();

    // The production materializer retries into the same run, and the workers run it.
    d.run_until("the scheduled run succeeded and was recorded", async || {
        d.count(
            "SELECT count(*) FROM schedule_runs AS occurrence JOIN workflow_runs AS run \
               ON run.company_id = occurrence.company_id AND run.id = occurrence.run_id \
             WHERE occurrence.company_id = $1 AND run.state = 'succeeded'",
        )
        .await
            == 1
    })
    .await;

    let runs: Vec<(Uuid, Uuid, Value, Uuid)> = sqlx::query_as(
        "SELECT id, trigger_id, input, actor_id FROM workflow_runs WHERE binding_id = $1",
    )
    .bind(binding.binding.as_uuid())
    .fetch_all(d.p().pool())
    .await
    .unwrap();
    assert_eq!(runs.len(), 1, "one run for the slot");
    let (run, trigger, input, actor) = runs[0].clone();
    assert_eq!((trigger, actor), (occurrence, owner));
    assert!(input["scheduled_for"].is_string());
    let (recorded, attempts): (Option<Uuid>, i32) =
        sqlx::query_as("SELECT run_id, materialization_attempts FROM schedule_runs WHERE id = $1")
            .bind(occurrence)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert_eq!((recorded, attempts), (Some(run), 1));
    assert_eq!(
        d.count("SELECT count(*) FROM threads WHERE company_id = $1")
            .await,
        0,
        "no thread or prompt message for a scheduled run"
    );
    assert_eq!(d.llm.finish().await.unwrap(), 1, "the agent answered once");
}
