use super::*;
use crate::adapters::persistence::workflow::schedule_test_support::schedule_use_cases;
use crate::application::message_outcomes::{MessageOutcomeReader, MessageOutcomeScope};
use crate::application::workflow::schedule_admission::{
    Materialized, ScheduleMaterializer, ScheduleWorkerId,
};
use crate::entities::department::{RoleAdmissionRefusal, RoleRefusalKind};
use crate::entities::schedule::{ScheduleTimezone, ScheduleType, ScheduleWrite};

async fn clear_lead(d: &Deployment, desk: &RoleDesk, agent: &RoleAgent) {
    crate::adapters::persistence::test_support::department_use_cases(d.p())
        .clear_role(
            d.fixture.target.actor.user_id(),
            d.fixture.target.company.as_uuid(),
            desk.department,
            DepartmentRole::Lead,
            agent.principal,
        )
        .await
        .unwrap();
}

fn refusal() -> RoleAdmissionRefusal {
    RoleAdmissionRefusal::new(
        StepId::parse("start").unwrap(),
        DepartmentRole::Lead,
        RoleRefusalKind::NoHolder,
    )
}

#[tokio::test]
async fn workflow_role_admission_message_refusal_is_definite_named_visible_and_admits_nothing() {
    let mut d = Deployment::new(vec![]).await;
    let a = agent(&d, "missing-lead").await;
    let desk = desk(&d, "missing-team", &a).await;
    d.channel = desk.id;
    let binding = d.bind(source("agent.run"), Some(MessageTrigger::Any)).await;
    clear_lead(&d, &desk, &a).await;
    d.email(RawInboundPayload {
        headers: Some("Message-ID: <missing-role@test>\n".into()),
        to: d.address(&desk.slug),
        from: CUSTOMER.into(),
        subject: Some("missing lead".into()),
        text: Some("please help".into()),
        ..Default::default()
    })
    .await;
    Box::pin(d.run_until("the role refusal settled without retry", async || {
        d.count("SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND state='done'").await == 1
    })).await;
    #[derive(sqlx::FromRow)]
    struct Recorded {
        message: Uuid,
        thread: Uuid,
        attempts: i16,
        reason: String,
        role_refusal: Value,
    }
    let recorded: Recorded = sqlx::query_as("SELECT intent.message_id AS message,intent.thread_id AS thread,intent.attempts,outcome.reason,outcome.role_refusal \
        FROM workflow_message_intents AS intent JOIN workflow_message_intent_bindings AS outcome \
        ON outcome.company_id=intent.company_id AND outcome.intent_id=intent.id \
        WHERE intent.company_id=$1 AND outcome.binding_id=$2 AND outcome.outcome='refused'")
        .bind(binding.company.as_uuid()).bind(binding.binding.as_uuid()).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(recorded.attempts, 1);
    assert_eq!(recorded.reason, refusal().kind.code());
    assert_eq!(
        serde_json::from_value::<RoleAdmissionRefusal>(recorded.role_refusal).unwrap(),
        refusal()
    );
    let outcomes = d
        .p()
        .message_outcomes(
            MessageOutcomeScope {
                company: binding.company,
                thread_id: recorded.thread,
                viewer: binding.actor,
            },
            &[recorded.message],
        )
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].outcome,
        crate::entities::message_outcome::BindingOutcome::RoleRefused(refusal())
    );
    for table in [
        "workflow_runs",
        "workflow_run_role_bindings",
        "agent_dependency_manifests",
    ] {
        let count: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(binding.company.as_uuid())
                .fetch_one(d.p().pool())
                .await
                .unwrap();
        assert_eq!(count, 0, "refused intent leaves no {table}");
    }
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn workflow_role_admission_schedule_refusal_keeps_typed_step_role_and_does_not_retry() {
    let mut d = Deployment::new(vec![]).await;
    let a = agent(&d, "scheduled-lead").await;
    let desk = desk(&d, "scheduled-team", &a).await;
    d.channel = desk.id;
    let binding = d.bind(source("agent.run"), None).await;
    let schedules = schedule_use_cases(d.p());
    let schedule = schedules
        .create_schedule(
            binding.actor.user_id(),
            binding.company.as_uuid(),
            desk.id,
            ScheduleWrite {
                name: "Role report".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(86400),
                scheduled_at: None,
                timezone: ScheduleTimezone::utc(),
                binding_id: binding.binding.as_uuid(),
                enabled: true,
            },
        )
        .await
        .unwrap();
    clear_lead(&d, &desk, &a).await;
    sqlx::query("UPDATE channel_schedules SET next_run_at=clock_timestamp()-interval '1 minute' WHERE id=$1")
        .bind(schedule.id).execute(d.p().pool()).await.unwrap();
    let materializer = ScheduleMaterializer::new(d.p().clone(), d.engine.services().service());
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = materializer.claim(worker).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        materializer.materialize(worker, &claimed[0]).await.unwrap(),
        Materialized::Refused(refusal().kind.code())
    );
    #[derive(sqlx::FromRow)]
    struct Recorded {
        status: String,
        run_id: Option<Uuid>,
        refusal_code: String,
        role_refusal: Value,
        materialization_attempts: i32,
    }
    let recorded: Recorded = sqlx::query_as("SELECT materialization_status AS status,run_id,refusal AS refusal_code,role_refusal,materialization_attempts FROM schedule_runs WHERE company_id=$1 AND id=$2")
        .bind(binding.company.as_uuid()).bind(claimed[0].id).fetch_one(d.p().pool()).await.unwrap();
    assert_eq!(recorded.status, "failed");
    assert_eq!(recorded.run_id, None);
    assert_eq!(recorded.refusal_code, refusal().kind.code());
    assert_eq!(recorded.materialization_attempts, 1);
    assert_eq!(
        serde_json::from_value::<RoleAdmissionRefusal>(recorded.role_refusal).unwrap(),
        refusal()
    );
    assert!(materializer.claim(worker).await.unwrap().is_empty());
    let message: String =
        sqlx::query_scalar("SELECT last_error FROM channel_schedules WHERE id=$1")
            .bind(schedule.id)
            .fetch_one(d.p().pool())
            .await
            .unwrap();
    assert!(message.contains("start") && message.contains("lead"));
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
            .await,
        0
    );
    assert_eq!(d.llm.finish().await.unwrap(), 0);
}
