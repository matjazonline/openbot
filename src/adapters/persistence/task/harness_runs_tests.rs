use super::*;
use crate::{
    adapters::persistence::test_support::test_pool,
    entities::{
        harness::HarnessKind,
        task::{NewTask, TaskLeaseRef},
    },
    task_queue::TaskPersistence,
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use chrono::Utc;
use serde_json::json;

#[path = "harness_receipt_tests.rs"]
mod receipt_writes;

struct Fixture {
    persistence: PostgresPersistence,
    identity: RunIdentity,
    lease: TaskLeaseRef,
}
impl Fixture {
    async fn new(pool: sqlx::PgPool) -> Self {
        Self::with_contract(pool, None).await
    }
    async fn with_contract(
        pool: sqlx::PgPool,
        contract: Option<crate::entities::response_contract::ResponseContract>,
    ) -> Self {
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("runs-{suffix}@example.test");
        persistence
            .create_user(&format!("runs-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let user = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            user.id,
            CompanyWrite {
                name: "Runs".into(),
                slug: format!("runs-{suffix}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let agent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                response_contract: crate::entities::response_contract::ContractUpdate(Some(
                    contract.clone(),
                )),
                name: "Agent".into(),
                slug: format!("runs-{suffix}"),
                harness_kind: Some(HarnessKind::Rig),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Runs".into(),
                slug: "runs".into(),
                enabled: false,
                agent_ids: Some(vec![agent.id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                None,
                "test",
                json!({}),
            ))
            .await
            .unwrap();
        sqlx::query("UPDATE background_tasks SET owner_principal_id = (SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2) WHERE id = $3")
            .bind(company.id).bind(agent.id).bind(task.id).execute(&persistence.pool).await.unwrap();
        assert!(
            persistence
                .claim_task(
                    task.id,
                    Uuid::new_v4(),
                    Utc::now() + chrono::Duration::minutes(5)
                )
                .await
                .unwrap()
        );
        let task = persistence.get_task_by_id(task.id).await.unwrap().unwrap();
        Self {
            persistence,
            identity: RunIdentity {
                response_contract: contract,
                company_id: company.id,
                task_id: task.id,
                agent_id: agent.id,
                harness: HarnessKind::Rig,
                provider: "openai".into(),
                model: "fixture".into(),
                capability_fingerprint: "b".repeat(64),
            },
            lease: TaskLeaseRef::of(&task).unwrap(),
        }
    }
    fn request(&self) -> OpenRun<'_> {
        OpenRun {
            lease: self.lease,
            identity: &self.identity,
            initial_prompt: "Private fixture prompt",
            policy: RigExecutionPolicy::default(),
        }
    }
    async fn cleanup(&self) {
        CompanyPersistence::delete(&self.persistence, self.identity.company_id)
            .await
            .unwrap();
    }
}
fn applied(outcome: WriteOutcome<RunCheckpoint>) -> RunCheckpoint {
    match outcome {
        WriteOutcome::Applied(run) | WriteOutcome::AlreadyApplied(run) => run,
        other => panic!("Unexpected outcome: {other:?}"),
    }
}
fn write(fixture: &Fixture, run: &RunCheckpoint) -> RunWrite {
    RunWrite {
        company_id: fixture.identity.company_id,
        run_id: run.run_id,
        lease: fixture.lease,
        expected_revision: run.revision,
    }
}
fn reservation() -> ModelReservation {
    ModelReservation {
        repair: None,
        request_id: ModelRequestId(Uuid::new_v4()),
        input_tokens: 100,
        output_tokens: 100,
    }
}

#[tokio::test]
async fn competing_checkpoint_writers_and_stale_generations_are_fenced() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool).await;
    let (left, right) = tokio::join!(
        fixture.persistence.open_run(fixture.request()),
        fixture.persistence.open_run(fixture.request())
    );
    let run = applied(left.unwrap());
    assert_eq!(run.run_id, applied(right.unwrap()).run_id);
    let fence = write(&fixture, &run);
    let barrier = tokio::sync::Barrier::new(2);
    let compete = || async {
        barrier.wait().await;
        fixture
            .persistence
            .reserve_model(&fence, reservation())
            .await
    };
    let (left, right) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(compete(), compete())
    })
    .await
    .expect("competing checkpoint writers must settle within the deadline");
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::Applied(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::RevisionConflict))
            .count(),
        1
    );
    let saved = fixture
        .persistence
        .load_run(
            fixture.identity.company_id,
            fixture.lease.task_id,
            run.run_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.reservations.len(), 1);
    let mut stale = write(&fixture, &saved);
    stale.lease.execution_generation = Uuid::new_v4();
    assert!(matches!(
        fixture
            .persistence
            .reserve_model(&stale, reservation())
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    assert!(
        fixture
            .persistence
            .load_run(Uuid::new_v4(), fixture.lease.task_id, run.run_id)
            .await
            .unwrap()
            .is_none()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn committed_calls_and_final_output_survive_a_new_adapter() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _claim_guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool.clone()).await;
    let mut run = applied(
        fixture
            .persistence
            .open_run(fixture.request())
            .await
            .unwrap(),
    );
    let budget = reservation();
    run = applied(
        fixture
            .persistence
            .reserve_model(&write(&fixture, &run), budget.clone())
            .await
            .unwrap(),
    );
    let id = InvocationId(Uuid::new_v4());
    run = applied(
        fixture
            .persistence
            .commit_model_turn(
                &write(&fixture, &run),
                SavedModelTurn {
                    token_usage_source: crate::services::harness::runs::TokenUsageSource::Reported,
                    invalid_response: None,
                    request_id: budget.request_id,
                    text: String::new(),
                    calls: vec![SavedToolCall {
                        invocation_id: id,
                        call_id: "wire_call".into(),
                        item_id: Some("wire_item".into()),
                        tool_id: "lookup".into(),
                        arguments: json!({"exact":42}),
                    }],
                    continuation: vec![],
                    input_tokens: 70,
                    output_tokens: 50,
                },
            )
            .await
            .unwrap(),
    );
    run = applied(
        fixture
            .persistence
            .prepare_invocation(&write(&fixture, &run), id)
            .await
            .unwrap(),
    );
    run = applied(
        fixture
            .persistence
            .record_result(&write(&fixture, &run), id, json!({"value":42}))
            .await
            .unwrap(),
    );
    let restarted = PostgresPersistence::new(pool);
    assert_eq!(
        restarted
            .load_run(
                fixture.identity.company_id,
                fixture.lease.task_id,
                run.run_id
            )
            .await
            .unwrap()
            .unwrap(),
        run
    );
    let output = AgentExecutionOutput {
        structured: None,
        content: "Done".into(),
        token_usage: Default::default(),
        disposition: crate::services::harness::AgentExecutionDisposition::Completed,
        metadata: None,
    };
    run = applied(
        restarted
            .save_final_output(&write(&fixture, &run), output.clone())
            .await
            .unwrap(),
    );
    assert_eq!(
        applied(restarted.open_run(fixture.request()).await.unwrap()).final_output,
        Some(output)
    );
    assert!(run.pending_call().is_none());
    let receipts = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM task_harness_invocations WHERE run_id = $1 AND state = 'completed'",
    )
    .bind(run.run_id.0)
    .fetch_one(&fixture.persistence.pool)
    .await
    .unwrap();
    assert_eq!(receipts, 1);
    fixture.cleanup().await;
}

#[tokio::test]
async fn competing_repairs_replay_committed_slots_and_fence_stale_final_output() {
    use crate::services::response_contract::{InvalidResponse, StructuredResponse};
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let contract = serde_json::from_value(
        json!({"version":1,"format":"json_schema","schema":{"type":"object"}}),
    )
    .unwrap();
    let fixture = Fixture::with_contract(pool.clone(), Some(contract)).await;
    let mut run = applied(
        fixture
            .persistence
            .open_run(fixture.request())
            .await
            .unwrap(),
    );
    let first = reservation();
    run = applied(
        fixture
            .persistence
            .reserve_model(&write(&fixture, &run), first.clone())
            .await
            .unwrap(),
    );
    run = applied(
        fixture
            .persistence
            .commit_model_turn(
                &write(&fixture, &run),
                SavedModelTurn {
                    token_usage_source: crate::services::harness::runs::TokenUsageSource::Reported,
                    request_id: first.request_id,
                    text: "invalid".into(),
                    calls: vec![],
                    continuation: vec![],
                    input_tokens: 2,
                    output_tokens: 3,
                    invalid_response: Some(InvalidResponse::MalformedJson),
                },
            )
            .await
            .unwrap(),
    );
    let fence = write(&fixture, &run);
    let repair = ModelReservation {
        repair: Some(RepairReservation {
            candidate: first.request_id,
            reason: InvalidResponse::MalformedJson,
        }),
        ..reservation()
    };
    let competitor = ModelReservation {
        request_id: ModelRequestId(Uuid::new_v4()),
        ..repair.clone()
    };
    let (left, right) = tokio::join!(
        fixture.persistence.reserve_model(&fence, repair.clone()),
        fixture
            .persistence
            .reserve_model(&fence, competitor.clone())
    );
    let outcomes = [left.unwrap(), right.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::Applied(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WriteOutcome::RevisionConflict))
            .count(),
        1
    );
    let restarted = PostgresPersistence::new(pool.clone());
    run = restarted
        .load_run(
            fixture.identity.company_id,
            fixture.lease.task_id,
            run.run_id,
        )
        .await
        .unwrap()
        .unwrap();
    let winner = run.reservations.last().unwrap().clone();
    assert!(matches!(
        restarted
            .reserve_model(&fence, winner.clone())
            .await
            .unwrap(),
        WriteOutcome::AlreadyApplied(_)
    ));
    assert_eq!(run.repair_count(), 1);
    let mut stale = write(&fixture, &run);
    sqlx::query("UPDATE background_tasks SET execution_generation=gen_random_uuid(),worker_id=gen_random_uuid() WHERE id=$1").bind(fixture.lease.task_id).execute(&pool).await.unwrap();
    assert!(matches!(
        restarted.reserve_model(&stale, competitor).await.unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let structured = StructuredResponse::validate(
        run.identity.response_contract.as_ref().unwrap(),
        "{}",
        None,
        &crate::adapters::response_schema::JsonResponseValidator,
    )
    .unwrap()
    .unwrap();
    let output = AgentExecutionOutput {
        content: structured.body().into(),
        structured: Some(structured),
        token_usage: Default::default(),
        disposition: crate::services::harness::AgentExecutionDisposition::Completed,
        metadata: None,
    };
    stale.expected_revision = run.revision;
    assert!(matches!(
        restarted
            .save_final_output(&stale, output.clone())
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let claimed = restarted
        .get_task_by_id(fixture.lease.task_id)
        .await
        .unwrap()
        .unwrap();
    let lease = TaskLeaseRef::of(&claimed).unwrap();
    let mut current = RunWrite {
        lease,
        ..write(&fixture, &run)
    };
    run = applied(
        restarted
            .commit_model_turn(
                &current,
                SavedModelTurn {
                    token_usage_source: crate::services::harness::runs::TokenUsageSource::Reported,
                    request_id: winner.request_id,
                    text: "{}".into(),
                    calls: vec![],
                    continuation: vec![],
                    input_tokens: 2,
                    output_tokens: 3,
                    invalid_response: None,
                },
            )
            .await
            .unwrap(),
    );
    current.expected_revision = run.revision;
    applied(
        restarted
            .save_final_output(&current, output.clone())
            .await
            .unwrap(),
    );
    let recovered = applied(
        restarted
            .open_run(OpenRun {
                lease,
                ..fixture.request()
            })
            .await
            .unwrap(),
    );
    assert_eq!(recovered.final_output, Some(output));
    assert_eq!(
        recovered.reservations.len(),
        2,
        "saved completion cannot reserve another generation"
    );
    fixture.cleanup().await;
}
