use super::*;
use crate::{
    entities::{
        harness_run::*,
        task::{NewTask, TaskLeaseRef},
    },
    services::harness::runs::*,
    task_queue::TaskPersistence,
    use_cases::channel::{ChannelPersistence, ChannelWrite},
};
use chrono::Utc;

struct DurableFixture {
    p: Arc<PostgresPersistence>,
    scope: McpRunScope,
    lease: TaskLeaseRef,
    invocation: InvocationId,
}
impl DurableFixture {
    async fn new(pool: sqlx::PgPool, endpoint: &str) -> Self {
        let p = Arc::new(PostgresPersistence::with_credential_cipher(
            pool,
            CredentialCipher::for_test(),
        ));
        let company = company(&p).await;
        let agent = agent(&p, company, "durable").await;
        let connection = definition(&p, company, endpoint, "durable", "secret").await;
        p.replace_agent_mcp_selection(
            p.agent_mcp_selection(company, agent).await.unwrap(),
            Some(vec![connection]),
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            p.as_ref(),
            company,
            ChannelWrite {
                name: "Durable MCP".into(),
                slug: "durable".into(),
                enabled: false,
                agent_ids: Some(vec![agent]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let task = p
            .enqueue_task(NewTask::starting_new_chain(
                company,
                channel.id,
                None,
                "test",
                json!({}),
            ))
            .await
            .unwrap();
        sqlx::query("UPDATE background_tasks SET owner_principal_id = (SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2) WHERE id = $3")
            .bind(company).bind(agent).bind(task.id).execute(&p.pool).await.unwrap();
        p.claim_task(
            task.id,
            Uuid::new_v4(),
            Utc::now() + chrono::Duration::minutes(5),
        )
        .await
        .unwrap();
        let lease = TaskLeaseRef::of(&p.get_task_by_id(task.id).await.unwrap().unwrap()).unwrap();
        let identity = RunIdentity {
            response_contract: None,
            company_id: company,
            task_id: task.id,
            agent_id: agent,
            harness: HarnessKind::Rig,
            provider: "openai".into(),
            model: "fixture".into(),
            capability_fingerprint: "e".repeat(64),
        };
        let mut run = applied(
            p.open_run(OpenRun {
                lease,
                identity: &identity,
                initial_prompt: "Search",
                policy: RigExecutionPolicy::default(),
            })
            .await
            .unwrap(),
        );
        let write = |run: &RunCheckpoint| RunWrite {
            company_id: company,
            run_id: run.run_id,
            lease,
            expected_revision: run.revision,
        };
        let reservation = ModelReservation {
            repair: None,
            request_id: ModelRequestId(Uuid::new_v4()),
            input_tokens: 100,
            output_tokens: 100,
        };
        run = applied(
            p.reserve_model(&write(&run), reservation.clone())
                .await
                .unwrap(),
        );
        let invocation = InvocationId(Uuid::new_v4());
        run = applied(
            p.commit_model_turn(
                &write(&run),
                SavedModelTurn {
                    token_usage_source: crate::services::harness::runs::TokenUsageSource::Reported,
                    invalid_response: None,
                    request_id: reservation.request_id,
                    text: String::new(),
                    continuation: vec![],
                    input_tokens: 10,
                    output_tokens: 10,
                    calls: vec![SavedToolCall {
                        invocation_id: invocation,
                        call_id: "remote-call".into(),
                        item_id: Some("remote-item".into()),
                        tool_id: mcp_model_id(&McpToolRef {
                            connection_id: connection,
                            name: "search".to_string().try_into().unwrap(),
                        }),
                        arguments: json!({"key":"record"}),
                    }],
                },
            )
            .await
            .unwrap(),
        );
        run = applied(
            p.prepare_invocation(&write(&run), invocation)
                .await
                .unwrap(),
        );
        Self {
            p,
            scope: McpRunScope {
                company_id: company,
                task_id: task.id,
                agent_id: agent,
                run_id: run.run_id.0,
            },
            lease,
            invocation,
        }
    }
    async fn cleanup(&self) {
        CompanyPersistence::delete(self.p.as_ref(), self.scope.company_id)
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
async fn remote_receipt_commits_with_fence_and_replays_without_network() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let server = scripted_scenario(exchanges("secret")).await;
    let fixture = DurableFixture::new(pool, &server.base_url).await;
    let client = Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([format!("{}/", server.base_url.trim_end_matches('/'))]).unwrap(),
    ));
    let runtime = Arc::new(McpRuntime::new(
        fixture.p.clone(),
        fixture.p.clone(),
        client.clone(),
    ));
    let host = runtime
        .prepare(
            fixture.scope.clone(),
            HarnessKind::Rig,
            fixture.p.mcp_journal(fixture.lease),
        )
        .await
        .unwrap();
    let id = fixture.invocation.0.to_string();
    let result = host
        .invoke(&host.available()[0], &id, json!({"key":"record"}))
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(server.finish().await, Ok(7));
    let restarted = PostgresPersistence::new(fixture.p.pool.clone());
    let journal = restarted.mcp_journal(fixture.lease);
    assert_eq!(
        journal
            .completed(
                &fixture.scope,
                &host.available()[0],
                &id,
                &json!({"key":"record"})
            )
            .await
            .unwrap(),
        Some(result.output)
    );
    assert!(
        journal
            .completed(
                &fixture.scope,
                &host.available()[0],
                &id,
                &json!({"key":"different"})
            )
            .await
            .is_err()
    );
    let run = restarted
        .load_run(
            fixture.scope.company_id,
            fixture.scope.task_id,
            RunId(fixture.scope.run_id),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.invocations[0].state, InvocationState::Completed);
    assert!(run.invocations[0].mcp.is_some());
    client.shutdown().await.unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
async fn competing_remote_claims_and_stale_completion_leave_an_indeterminate_receipt() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let _guard = crate::adapters::persistence::test_support::UNSCOPED_CLAIM
        .lock()
        .await;
    let server = scripted_scenario(discovery("secret")).await;
    let fixture = DurableFixture::new(pool, &server.base_url).await;
    let client = Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([format!("{}/", server.base_url.trim_end_matches('/'))]).unwrap(),
    ));
    let runtime = Arc::new(McpRuntime::new(
        fixture.p.clone(),
        fixture.p.clone(),
        client.clone(),
    ));
    let journal = fixture.p.mcp_journal(fixture.lease);
    let host = runtime
        .prepare(fixture.scope.clone(), HarnessKind::Rig, journal.clone())
        .await
        .unwrap();
    assert_eq!(server.finish().await, Ok(3));
    let declaration = &host.available()[0];
    let id = fixture.invocation.0.to_string();
    let args = json!({"key":"record"});
    let (left, right) = tokio::join!(
        journal.claim(&fixture.scope, declaration, &id, &args),
        journal.claim(&fixture.scope, declaration, &id, &args)
    );
    assert_eq!(
        [left, right]
            .into_iter()
            .filter(|result| matches!(result, Ok(McpClaim::Execute)))
            .count(),
        1
    );
    let mut stale = fixture.lease;
    stale.execution_generation = Uuid::new_v4();
    assert!(
        fixture
            .p
            .mcp_journal(stale)
            .complete(&fixture.scope, &id, &json!({"done":true}))
            .await
            .is_err()
    );
    assert!(matches!(
        journal
            .completed(&fixture.scope, declaration, &id, &args)
            .await,
        Err(AppError::Execution(
            crate::app_error::ExecutionFailure::IndeterminateEffect
        ))
    ));
    let run = fixture
        .p
        .load_run(
            fixture.scope.company_id,
            fixture.scope.task_id,
            RunId(fixture.scope.run_id),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.invocations[0].state, InvocationState::Indeterminate);
    client.shutdown().await.unwrap();
    fixture.cleanup().await;
}
