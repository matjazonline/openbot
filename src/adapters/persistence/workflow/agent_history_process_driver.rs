use super::*;
use std::io::{BufRead, Write};
use std::str::FromStr;

struct CrashHandler {
    persistence: PostgresPersistence,
    spec: ProcessSpec,
}
#[async_trait::async_trait]
impl WorkflowHandler for CrashHandler {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == "agent.run"
    }
    async fn execute(
        &self,
        kind: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        emit(
            "AH_PROCESS_CLAIM",
            serde_json::json!({"worker":claim.fence.worker.0,"generation":claim.fence.generation.0,"attempt":claim.fence.attempt.0}),
        );
        if self.spec.point == CrashPoint::Frozen {
            self.persistence
                .prepare_agent(claim, crash_policy())
                .await
                .unwrap()
                .expect_ready();
            ready();
        }
        if self.spec.point == CrashPoint::BeforeWake {
            self.persistence
                .prepare_agent(claim, crash_policy())
                .await
                .unwrap()
                .expect_ready();
            emit("AH_PROCESS_PREPARED", json!({"prepared":true}));
            let command = tokio::task::spawn_blocking(|| {
                let mut command = String::new();
                std::io::stdin().lock().read_line(&mut command).unwrap();
                command
            })
            .await
            .unwrap();
            assert_eq!(command.trim(), "continue");
        }
        let result = Box::pin(runtime_on(&self.persistence).execute(kind, claim)).await;
        if self.spec.point == CrashPoint::Enqueued {
            assert!(matches!(result, WorkflowHandlerOutcome::Suspended(_)));
        } else {
            assert_eq!(
                result.completed_result().unwrap(),
                json!("P07_TARGET_OUTPUT")
            );
        }
        ready();
    }
}
fn emit(marker: &str, value: Value) {
    println!("{marker} {value}");
    std::io::stdout().flush().unwrap();
}
fn ready() -> ! {
    emit("AH_PROCESS_READY", json!({"durable_frontier":true}));
    loop {
        std::thread::park();
    }
}

// This technical entry is exercised by its parent process tests, with an owned database.
#[tokio::test]
#[ignore = "spawned with isolated fixture by agent_history_e2e process parents"]
async fn history_crash_child_entry() {
    let spec: ProcessSpec = serde_json::from_str(
        &std::env::var("AGENT_HISTORY_PROCESS_SPEC").expect("parent-owned fixture specification"),
    )
    .unwrap();
    spec.validate_fixture();
    let options =
        sqlx::postgres::PgConnectOptions::from_str(&std::env::var("TEST_DATABASE_URL").unwrap())
            .unwrap()
            .database(&spec.database.0);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    let persistence =
        PostgresPersistence::with_credential_cipher(pool, CredentialCipher::for_test());
    spec.listener.register_agent(spec.agent).await.unwrap();
    let scope = spec.scope();
    let handler = CrashHandler {
        persistence: persistence.clone(),
        spec,
    };
    let worker = WorkflowWorker {
        port: &persistence,
        handler: &handler,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: crash_policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 128).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    Box::pin(worker.process(
        PollCandidate {
            scope,
            work: PollWork::Job,
        },
        &CancellationToken::new(),
    ))
    .await
    .unwrap();
    panic!("the actual worker must reach the required crash barrier");
}
