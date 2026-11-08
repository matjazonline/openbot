use super::*;
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    agent::AgentPreparation,
    polling::{PollCandidate, PollPolicy, PollWork, WorkflowPolling},
    worker::{WorkflowHandler, WorkflowWorker},
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[path = "agent_history_process_tests.rs"]
mod crashes;
#[path = "agent_history_process_driver.rs"]
mod driver;
#[path = "agent_history_process_fixture.rs"]
mod process;
#[path = "agent_history_queue_crash_tests.rs"]
mod queue_crashes;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum CrashPoint {
    Enqueued,
    Acquisition,
    Frozen,
    ModelSettled,
    FinalCheckpoint,
    BeforeWake,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnedDatabaseName(String);
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureEndpoint(String);
#[derive(Clone, Serialize, Deserialize)]
struct ProcessSpec {
    database: OwnedDatabaseName,
    endpoint: FixtureEndpoint,
    agent: Uuid,
    company: Uuid,
    run: Uuid,
    execution: Uuid,
    job: Uuid,
    point: CrashPoint,
    listener: crate::services::test_support::InheritedScriptedListener,
}
impl ProcessSpec {
    fn scope(&self) -> ActivationRequest {
        ActivationRequest {
            company: CompanyId::new(self.company),
            run: RunId::new(self.run),
            execution: ExecutionId::new(self.execution),
            job: WorkflowJobId(self.job),
        }
    }
    fn validate_fixture(&self) {
        let suffix = self
            .database
            .0
            .strip_prefix("open_bots_own_")
            .expect("parent-owned disposable database");
        assert!(Uuid::parse_str(suffix).is_ok());
        let endpoint = url::Url::parse(&self.endpoint.0).unwrap();
        assert_eq!(endpoint.scheme(), "http");
        assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
        assert!(endpoint.port().unwrap() > 0);
        assert!(endpoint.username().is_empty() && endpoint.password().is_none());
    }
}

fn crash_policy() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(3)).unwrap()
}
fn runtime_on(
    p: &PostgresPersistence,
) -> crate::adapters::harness::rig::WorkflowAgentRuntime<PostgresPersistence, NoTools> {
    crate::adapters::harness::rig::WorkflowAgentRuntime::new(
        crate::adapters::harness::rig::providers::ProviderRegistry::standard().unwrap(),
        p.clone(),
        NoTools,
        None,
        crash_policy(),
    )
}
async fn work(p: &PostgresPersistence, candidate: PollCandidate) {
    let runtime = runtime_on(p);
    let worker = WorkflowWorker {
        port: p,
        handler: &runtime,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: crash_policy(),
        poll: PollPolicy::new(Duration::from_millis(50), 128).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    // Keep the real worker/agent seam boxed at the stock 2 MiB test stack.
    Box::pin(worker.process(candidate, &CancellationToken::new()))
        .await
        .unwrap();
}

async fn pending(f: &ManualHistory, input: Value) -> ActivationRequest {
    let company = f.f.s.fixture.target.company;
    let request = AdmitWorkflowRequest {
        company_id: company,
        actor: f.actor,
        binding_id: f.binding,
        association: RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(f.f.s.channel),
            thread_id: RelatedThreadId::new(f.f.s.thread),
        },
        trigger: TriggerRef::new(
            company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
        correlation_id: CorrelationId::new(),
        idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
        input,
        rerun_of: None,
        history_session: f
            .from_run
            .map(|from_run| AgentHistorySessionRef { from_run }),
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
        admission_claim: None,
    };
    let AdmissionResult::Created(run) = admission(f.f.s.p()).admit(request).await.unwrap() else {
        panic!("fresh root")
    };
    #[derive(sqlx::FromRow)]
    struct Source {
        execution: Uuid,
        job: Uuid,
    }
    let source:Source=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.completed_at IS NULL")
        .bind(company.as_uuid()).bind(run.as_uuid()).fetch_one(f.f.s.pool()).await.unwrap();
    ActivationRequest {
        company,
        run,
        execution: ExecutionId::new(source.execution),
        job: WorkflowJobId(source.job),
    }
}

async fn drive_reconstructed(p: &PostgresPersistence, scope: ActivationRequest) {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let state: String =
                sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
                    .bind(scope.company.as_uuid())
                    .bind(scope.run.as_uuid())
                    .fetch_one(p.pool())
                    .await
                    .unwrap();
            if state == "succeeded" {
                return;
            }
            assert_ne!(
                state, "failed",
                "native recovery must retain a safe attempt"
            );
            let page = p.poll_work(None, 128).await.unwrap();
            if let Some(candidate) = page
                .candidates
                .into_iter()
                .find(|candidate| candidate.scope == scope)
            {
                // Genuine competing replacement workers, each with its own durable claim identity.
                tokio::join!(work(p, candidate), work(p, candidate));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("real expired lease/poll recovery completes the original root");
}
