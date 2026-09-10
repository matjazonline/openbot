use super::*;
use crate::entities::{
    company::Company,
    task::{TaskLeaseRef, TaskStatus},
};
use crate::services::harness::runs::*;
use serde_json::json;

struct Fixture {
    persistence: PostgresPersistence,
    company: Company,
    subject: ApprovalSubject,
    lease: TaskLeaseRef,
    agent_id: Uuid,
}
impl Fixture {
    async fn new(pool: sqlx::PgPool) -> Self {
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("approval-run-{suffix}@example.test");
        persistence
            .create_user(&format!("approval-run-{suffix}"), &email, "hash")
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
                name: "Approval run".into(),
                slug: format!("approval-run-{suffix}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let agent_id = seed_channel_agent(&persistence, company.id, "checkpoint").await;
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Approvals".into(),
                slug: "approvals".into(),
                enabled: false,
                agent_ids: Some(vec![agent_id]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let thread = persistence
            .create_thread(channel.id, "Checkpoints", &[])
            .await
            .unwrap();
        let task = persistence
            .enqueue_task(NewTask::starting_new_chain(
                company.id,
                channel.id,
                Some(thread.id),
                "test",
                json!({}),
            ))
            .await
            .unwrap();
        sqlx::query("UPDATE background_tasks SET owner_principal_id = (SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2), retry_count = 1 WHERE id = $3")
            .bind(company.id).bind(agent_id).bind(task.id).execute(&persistence.pool).await.unwrap();
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
        let lease = TaskLeaseRef::of(&task).unwrap();
        let subject = ApprovalSubject {
            suspension: Some(TaskSuspension::Leased(lease)),
            ..approval_subject(&company, &channel, thread.id, &email)
        };
        Self {
            persistence,
            company,
            subject,
            lease,
            agent_id,
        }
    }
    fn write(&self, run: &RunCheckpoint) -> RunWrite {
        RunWrite {
            company_id: self.company.id,
            run_id: run.run_id,
            lease: self.lease,
            expected_revision: run.revision,
        }
    }
    async fn prepared(&self) -> RunCheckpoint {
        let identity = RunIdentity {
            response_contract: None,
            company_id: self.company.id,
            task_id: self.lease.task_id,
            agent_id: self.agent_id,
            harness: crate::entities::harness::HarnessKind::Rig,
            provider: "openai".into(),
            model: "fixture".into(),
            capability_fingerprint: "d".repeat(64),
        };
        let mut run = applied(
            self.persistence
                .open_run(OpenRun {
                    lease: self.lease,
                    identity: &identity,
                    initial_prompt: "Keep the entire original conversation",
                    policy: RigExecutionPolicy::default(),
                })
                .await
                .unwrap(),
        );
        let reservation = ModelReservation {
            repair: None,
            request_id: ModelRequestId(Uuid::new_v4()),
            input_tokens: 100,
            output_tokens: 100,
        };
        run = applied(
            self.persistence
                .reserve_model(&self.write(&run), reservation.clone())
                .await
                .unwrap(),
        );
        let calls = vec![
            SavedToolCall {
                invocation_id: InvocationId(Uuid::new_v4()),
                call_id: "original-call".into(),
                item_id: Some("original-item".into()),
                tool_id: "request_approval".into(),
                arguments: json!({"title":"Plan", "proposal":"The concrete proposal"}),
            },
            SavedToolCall {
                invocation_id: InvocationId(Uuid::new_v4()),
                call_id: "later-call".into(),
                item_id: None,
                tool_id: "lookup".into(),
                arguments: json!({"saved":42}),
            },
        ];
        let id = calls[0].invocation_id;
        run = applied(
            self.persistence
                .commit_model_turn(
                    &self.write(&run),
                    SavedModelTurn {
                        token_usage_source:
                            crate::services::harness::runs::TokenUsageSource::Reported,
                        invalid_response: None,
                        request_id: reservation.request_id,
                        text: String::new(),
                        calls,
                        continuation: vec![],
                        input_tokens: 20,
                        output_tokens: 20,
                    },
                )
                .await
                .unwrap(),
        );
        applied(
            self.persistence
                .prepare_invocation(&self.write(&run), id)
                .await
                .unwrap(),
        )
    }
    async fn park(&self, run: &RunCheckpoint) -> HumanApproval {
        let (notice, delivery) =
            approval_notice(&self.persistence, &self.subject, "checkpoint").await;
        self.persistence
            .create_approval(NewApproval {
                subject: &self.subject,
                invocation: Some(InvocationRef {
                    run_id: run.run_id,
                    invocation_id: run.pending_call().unwrap().invocation_id,
                    expected_revision: run.revision,
                }),
                action: &ApprovalAction {
                    step_key: "checkpoint".into(),
                    action_type: "checkpoint_v1".into(),
                    title: "Plan".into(),
                    summary: "The concrete proposal".into(),
                    payload: json!({"proposal":"The concrete proposal"}),
                },
                message: &notice,
                delivery,
                token: Uuid::new_v4(),
                expires_at: Utc::now() + chrono::Duration::hours(24),
            })
            .await
            .unwrap()
            .0
    }
    async fn cleanup(&self) {
        CompanyPersistence::delete(&self.persistence, self.company.id)
            .await
            .unwrap();
    }
}
fn applied(outcome: WriteOutcome<RunCheckpoint>) -> RunCheckpoint {
    match outcome {
        WriteOutcome::Applied(run) | WriteOutcome::AlreadyApplied(run) => run,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn approval_restores_the_exact_checkpoint_and_only_one_decision_wins() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool.clone()).await;
    let run = fixture.prepared().await;
    let approval = fixture.park(&run).await;
    assert_eq!(
        fixture.park(&run).await.id,
        approval.id,
        "repeated creation verifies the existing wait without another notice"
    );
    let parked = fixture
        .persistence
        .load_run(fixture.company.id, fixture.lease.task_id, run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parked.state, RunState::Waiting);
    assert_eq!(
        parked.invocations.len(),
        1,
        "the second call has not executed"
    );
    assert!(matches!(
        fixture
            .persistence
            .reserve_model(
                &fixture.write(&parked),
                ModelReservation {
                    repair: None,
                    request_id: ModelRequestId(Uuid::new_v4()),
                    input_tokens: 100,
                    output_tokens: 100
                }
            )
            .await
            .unwrap(),
        WriteOutcome::OwnershipLost
    ));
    let (first, duplicate) = tokio::join!(
        fixture.persistence.decide_approval_and_transition(
            &approval.token,
            ApprovalStatus::Approved,
            Utc::now()
        ),
        fixture.persistence.decide_approval_and_transition(
            &approval.token,
            ApprovalStatus::Approved,
            Utc::now()
        )
    );
    assert_eq!(
        [first.unwrap(), duplicate.unwrap()]
            .iter()
            .filter(|value| value.is_some())
            .count(),
        1
    );
    let restarted = PostgresPersistence::new(pool);
    let restored = restarted
        .load_run(fixture.company.id, fixture.lease.task_id, run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.state, RunState::Active);
    assert_eq!(restored.invocations[0].state, InvocationState::Completed);
    assert_eq!(
        restored.invocations[0].result.as_ref().unwrap()["status"],
        "approved"
    );
    assert_eq!(restored.pending_call().unwrap().call_id, "later-call");
    assert_eq!(restored.messages[0], run.messages[0]);
    let task = restarted
        .get_task_by_id(fixture.lease.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.retry_count, 1);
    let decisions = restarted
        .list_thread_message_views(fixture.subject.thread_id)
        .await
        .unwrap();
    assert_eq!(
        decisions
            .iter()
            .filter(|message| message.body.contains("Human approval approved"))
            .count(),
        1
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn expiry_closes_the_wait_and_stops_the_task_without_charging_an_attempt() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool).await;
    let run = fixture.prepared().await;
    let approval = fixture.park(&run).await;
    sqlx::query("UPDATE human_approvals SET created_at = CURRENT_TIMESTAMP - INTERVAL '25 hours', expires_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' WHERE id = $1")
        .bind(approval.id).execute(&fixture.persistence.pool).await.unwrap();
    fixture.persistence.expire_due_approvals(128).await.unwrap();
    let task = fixture
        .persistence
        .get_task_by_id(fixture.lease.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Stopped);
    assert_eq!(task.retry_count, 1);
    assert_eq!(task.last_error.as_deref(), Some("Human approval expired"));
    let saved = fixture
        .persistence
        .load_run(fixture.company.id, task.id, run.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.state, RunState::Superseded);
    assert_eq!(saved.invocations[0].state, InvocationState::Failed);
    assert!(
        fixture
            .persistence
            .decide_approval_and_transition(&approval.token, ApprovalStatus::Approved, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn decision_rolls_back_when_the_linked_checkpoint_cannot_commit() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let fixture = Fixture::new(pool.clone()).await;
    let run = fixture.prepared().await;
    let approval = fixture.park(&run).await;
    sqlx::query("UPDATE task_harness_runs SET checkpoint = jsonb_set(checkpoint, '{policy,model_calls}', '0') WHERE id = $1")
        .bind(run.run_id.0).execute(&pool).await.unwrap();
    assert!(
        fixture
            .persistence
            .decide_approval_and_transition(&approval.token, ApprovalStatus::Approved, Utc::now())
            .await
            .is_err()
    );
    let status: String = sqlx::query_scalar("SELECT status FROM human_approvals WHERE id = $1")
        .bind(approval.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(
        fixture
            .persistence
            .get_task_by_id(fixture.lease.task_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::PendingApproval
    );
    let notes = fixture
        .persistence
        .list_thread_message_views(fixture.subject.thread_id)
        .await
        .unwrap();
    assert!(
        !notes
            .iter()
            .any(|message| message.body.contains("Human approval approved"))
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn expiry_sweep_backs_off_poison_and_settles_other_work_on_two_iterations() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let poison = Fixture::new(pool.clone()).await;
    let broken = poison.prepared().await;
    let poisoned_approval = poison.park(&broken).await;
    sqlx::query("UPDATE task_harness_runs SET checkpoint = jsonb_set(checkpoint, '{policy,model_calls}', '0') WHERE id = $1")
        .bind(broken.run_id.0).execute(&pool).await.unwrap();
    for _ in 0..2 {
        let healthy = Fixture::new(pool.clone()).await;
        let run = healthy.prepared().await;
        let approval = healthy.park(&run).await;
        sqlx::query("UPDATE human_approvals SET created_at = CURRENT_TIMESTAMP - interval '2 days', expires_at = CURRENT_TIMESTAMP - interval '1 minute', expiry_retry_at = NULL WHERE id = ANY($1)")
            .bind(vec![approval.id, poisoned_approval.id]).execute(&pool).await.unwrap();
        assert!(healthy.persistence.expire_due_approvals(128).await.unwrap() >= 1);
        assert_eq!(
            healthy
                .persistence
                .get_task_by_id(healthy.lease.task_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            TaskStatus::Stopped
        );
        let retry: bool = sqlx::query_scalar("SELECT status = 'pending' AND expiry_retry_at > CURRENT_TIMESTAMP FROM human_approvals WHERE id = $1")
            .bind(poisoned_approval.id).fetch_one(&pool).await.unwrap();
        assert!(
            retry,
            "poison remains visible and is deferred rather than retried in a hot loop"
        );
        healthy.cleanup().await;
    }
    poison.cleanup().await;
}
