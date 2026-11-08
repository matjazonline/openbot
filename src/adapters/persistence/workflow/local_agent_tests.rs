//! Real SQL/worker integration. Native outcomes here are server-side fixtures, not
//! evidence that a provider or the Cloudroom runtime executed a prompt.
use super::action_outcome_support::CompletedOutcome;
use super::message_tests::support::{admitted_as, bound, lease};
use super::*;
use crate::adapters::persistence::local_agents::protocol::test_support::agent;
use crate::adapters::persistence::local_agents::test_support::Fixture as MachineFixture;
use crate::adapters::persistence::local_agents::test_support::Paired;
use crate::application::local_agents::workflow::{KIND, LocalAgentHandler};
use crate::application::local_agents::{
    ConnectionFence, LocalCatalog, LocalMachineStore,
    protocol::{JournalHello, JournalObservation, LocalProtocolStore},
};
use crate::application::workflow::completion::WorkflowCompletion;
use crate::application::workflow::decisions::WorkflowDecisions;
use crate::application::workflow::{
    activation::ActivationRequest, lease::*, polling::WorkflowStepKind, waits::*, worker::*,
};
use crate::domain::local_agents::*;
use serde_json::{Value, json};

#[path = "local_agent_behavior_tests.rs"]
mod behavior;

#[path = "local_agent_resolution_tests.rs"]
mod resolution;

#[path = "local_agent_continuation_tests.rs"]
mod continuation;
#[path = "local_agent_correction_tests.rs"]
mod corrections;

struct Setup {
    fixture: Fixture,
    profile: LocalAgentProfile,
    scope: ActivationRequest,
    paired: Paired,
    journal_id: Uuid,
}

struct NativeTurn {
    fence: ConnectionFence,
    operation: OperationId,
    thread: CloudroomThreadId,
    turn: CloudroomTurnId,
    sequence: u64,
}

enum TestAssociation {
    Company,
    PrivateConversation,
}

impl Setup {
    fn new() -> impl std::future::Future<Output = Self> {
        Self::with_inputs(|_, _| {})
    }
    fn with_inputs(
        customize: impl FnOnce(&mut Value, Uuid),
    ) -> impl std::future::Future<Output = Self> {
        Self::configured(customize, TestAssociation::Company)
    }
    async fn configured(
        customize: impl FnOnce(&mut Value, Uuid),
        association: TestAssociation,
    ) -> Self {
        let local = MachineFixture::new()
            .await
            .expect("local workflow requires PostgreSQL");
        let machine = local.pair(local.company).await;
        local.install_catalog(&machine).await;
        let profile = local.profile(machine.receipt.machine.id);
        local
            .cases
            .set_projects(
                local.owner,
                local.company,
                machine.receipt.machine.id,
                std::slice::from_ref(&profile.selection.project_id),
            )
            .await
            .unwrap();
        let association = match association {
            TestAssociation::Company => RelatedAssociation::Company,
            TestAssociation::PrivateConversation => {
                let thread = local.conversation(local.company).await;
                let channel:Uuid=sqlx::query_scalar("UPDATE channels SET access_mode='allowlist' WHERE id=(SELECT channel_id FROM threads WHERE id=$1) RETURNING id")
                    .bind(thread).fetch_one(&local.database.pool).await.unwrap();
                RelatedAssociation::Thread {
                    channel_id: crate::application::workflow::RelatedChannelId::new(channel),
                    thread_id: crate::application::workflow::RelatedThreadId::new(thread),
                }
            }
        };
        local
            .cases
            .save_profile(local.owner, &profile)
            .await
            .unwrap();
        let fixture = Fixture {
            _db: Arc::new(local.database),
            persistence: local.persistence.as_ref().clone(),
            target: DefinitionTarget {
                company: CompanyId::new(local.company),
                actor: WorkflowActor::authenticated(local.owner).unwrap(),
                workflow: WorkflowId::new(Uuid::from_u128(1)),
            },
        };
        let mut inputs =
            json!({"profile_id":{"literal":profile.id},"prompt":{"literal":"bounded native task"}});
        customize(&mut inputs, fixture.target.actor.user_id());
        let version = publish_local(&fixture, inputs).await;
        let binding = bound(&fixture, version).await;
        let scope = admitted_as(&fixture, binding, association, "local-step").await;
        Self {
            fixture,
            profile,
            scope,
            paired: machine,
            journal_id: Uuid::new_v4(),
        }
    }
    fn handler(&self) -> LocalAgentHandler<local_agents::PostgresLocalAgentWorkflow> {
        LocalAgentHandler::new(
            local_agents::PostgresLocalAgentWorkflow::new(
                self.fixture.persistence.clone(),
                Some(super::approval_test_support::notices(
                    &self.fixture.persistence,
                )),
            ),
            lease(),
        )
    }
    async fn claim(&self) -> ClaimedWorkflow {
        self.fixture
            .persistence
            .claim_io(self.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap()
    }
    async fn activate(
        &self,
        actor: WorkflowActor,
        inputs: Value,
        association: RelatedAssociation,
    ) -> ActivationRequest {
        let fixture = Fixture {
            _db: self.fixture._db.clone(),
            persistence: self.fixture.persistence.clone(),
            target: DefinitionTarget {
                actor,
                workflow: WorkflowId::new(Uuid::new_v4()),
                ..self.fixture.target
            },
        };
        let version = publish_local(&fixture, inputs).await;
        let binding = bound(&fixture, version).await;
        admitted_as(&fixture, binding, association, &Uuid::new_v4().to_string()).await
    }
    async fn park(&self, claim: &ClaimedWorkflow) -> WaitId {
        let inputs = serde_json::from_value(claim.activation.inputs.clone()).unwrap();
        let actions = crate::application::workflow::actions::ActionService::new(
            local_agents::PostgresLocalAgentWorkflow::new(
                self.fixture.persistence.clone(),
                Some(super::approval_test_support::notices(
                    &self.fixture.persistence,
                )),
            ),
        );
        match actions.run_local_agent(claim, &inputs, lease()).await {
            Ok(crate::application::local_agents::workflow::LocalRunProgress::Suspended(wait)) => {
                wait
            }
            Err(error) => panic!("local action transaction: {error:?}"),
            _ => panic!("expected local park"),
        }
    }
    async fn rows(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('calls',(SELECT count(*) FROM workflow_local_agent_calls),'operations',(SELECT count(*) FROM local_agent_operations WHERE command->>'kind' IN ('create','send')),'intents',(SELECT count(*) FROM workflow_action_intents),'markers',(SELECT count(*) FROM workflow_action_dispatches),'receipts',(SELECT count(*) FROM workflow_action_receipts),'waits',(SELECT count(*) FROM workflow_waits))")
            .fetch_one(self.fixture.persistence.pool()).await.unwrap()
    }
    async fn outcome(&self, state: &str, text: &str) {
        sqlx::query("UPDATE local_agent_operations SET outcome=$1,output=$2 WHERE id=(SELECT operation_id FROM workflow_local_agent_calls WHERE company_id=$3 AND run_id=$4)")
            .bind(state).bind(text).bind(self.scope.company.as_uuid()).bind(self.scope.run.as_uuid()).execute(self.fixture.persistence.pool()).await.unwrap();
    }

    async fn peer(&self) -> (ConnectionFence, u64) {
        let p = &self.fixture.persistence;
        let fence = p
            .acquire_connection(
                self.paired.candidate.credential.verifier(),
                self.paired.receipt.machine.installation_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap()
            .unwrap();
        let sequence: i64 = sqlx::query_scalar(
            "SELECT acknowledged_sequence FROM local_machines WHERE company_id=$1 AND id=$2",
        )
        .bind(self.scope.company.as_uuid())
        .bind(self.profile.selection.machine_id.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
        p.connect_journal(
            fence,
            &JournalHello {
                version: 1,
                installation_id: fence.machine.installation_id,
                journal_id: self.journal_id,
                journal_sequence: sequence as u64,
            },
        )
        .await
        .unwrap();
        (fence, sequence as u64)
    }

    async fn native_started(&self) -> NativeTurn {
        let p = &self.fixture.persistence;
        let (fence, sequence) = self.peer().await;
        assert!(p.finish_reconciliation(fence).await.unwrap());
        let deliveries = p.pending_commands(fence).await.unwrap();
        let command = deliveries
            .iter()
            .find(|delivery| delivery.operation.command().is_effect())
            .expect("authorized saved workflow command");
        let mut native = self
            .native_received(fence, &command.operation, sequence)
            .await;
        for delivery in deliveries.iter().filter(|delivery| {
            matches!(
                delivery.operation.command(),
                LocalCommand::RefreshCatalog {}
            )
        }) {
            let catalog:Value=sqlx::query_scalar("SELECT catalog FROM local_machine_capabilities WHERE company_id=$1 AND machine_id=$2 ORDER BY sequence DESC LIMIT 1")
                .bind(self.scope.company.as_uuid()).bind(self.profile.selection.machine_id.as_uuid()).fetch_one(p.pool()).await.unwrap();
            let catalog: LocalCatalog = serde_json::from_value(catalog).unwrap();
            native.sequence += 1;
            assert_eq!(
                p.accept_observation(
                    fence,
                    &agent(
                        native.sequence,
                        delivery.operation.id(),
                        LocalEvidence::Receipt {}
                    )
                )
                .await
                .unwrap(),
                native.sequence
            );
            native.sequence += 1;
            assert_eq!(
                p.accept_observation(
                    fence,
                    &JournalObservation::Catalog {
                        sequence: EventSequence::try_from(native.sequence).unwrap(),
                        operation_id: delivery.operation.id(),
                        catalog
                    }
                )
                .await
                .unwrap(),
                native.sequence
            );
        }
        native
    }

    async fn native_received(
        &self,
        fence: ConnectionFence,
        operation: &LocalAgentOperation,
        sequence: u64,
    ) -> NativeTurn {
        let p = &self.fixture.persistence;
        let id = operation.id();
        let thread = match operation.command() {
            LocalCommand::Send { thread_id, .. } => thread_id.clone(),
            _ => CloudroomThreadId::try_from(format!("thread_{}", id.as_uuid().simple())).unwrap(),
        };
        let turn = CloudroomTurnId::try_from(format!("turn_{}", id.as_uuid().simple())).unwrap();
        let observations = [
            LocalEvidence::Receipt {},
            LocalEvidence::SubmissionIntent {},
            LocalEvidence::Turn {
                thread_id: thread.clone(),
                turn_id: turn.clone(),
                state: TurnState::Running,
                output: None,
                truncated: false,
                usage: None,
            },
        ];
        for (index, evidence) in observations.into_iter().enumerate() {
            let through = sequence + index as u64 + 1;
            let observation = agent(through, id, evidence);
            assert_eq!(
                p.accept_observation(fence, &observation).await.unwrap(),
                through
            );
            assert_eq!(
                p.accept_observation(fence, &observation).await.unwrap(),
                through
            );
        }
        NativeTurn {
            fence,
            operation: id,
            thread,
            turn,
            sequence: sequence + 3,
        }
    }

    async fn native_complete(&self, native: &NativeTurn) {
        self.native_observe(native, native.sequence + 1, TurnState::Completed)
            .await;
    }
    async fn native_observe(&self, native: &NativeTurn, sequence: u64, state: TurnState) {
        let observation = agent(
            sequence,
            native.operation,
            LocalEvidence::Turn {
                thread_id: native.thread.clone(),
                turn_id: native.turn.clone(),
                state,
                output: (state == TurnState::Completed)
                    .then(|| LocalOutput::try_from("native output".to_owned()).unwrap()),
                truncated: false,
                usage: None,
            },
        );
        for _ in 0..2 {
            assert_eq!(
                self.fixture
                    .persistence
                    .accept_observation(native.fence, &observation)
                    .await
                    .unwrap(),
                sequence
            );
        }
    }

    async fn native_success(&self) -> ConnectionFence {
        let native = self.native_started().await;
        self.native_complete(&native).await;
        native.fence
    }
}

#[tokio::test]
async fn cloudroom_workflow_competing_claims_park_one_operation_and_fresh_claim_consumes_once() {
    let s = Setup::new().await;
    let p = &s.fixture.persistence;
    let (one, two) = tokio::join!(
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease()),
        p.claim_io(s.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
    );
    let claims: Vec<_> = [one.unwrap(), two.unwrap()].into_iter().flatten().collect();
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    let wait = s.park(claim).await;
    let command: Value = sqlx::query_scalar("SELECT command FROM local_agent_operations")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(
        command["selection"],
        serde_json::to_value(&s.profile.selection).unwrap()
    );
    assert_eq!(
        s.park(claim).await,
        wait,
        "lost COMMIT acknowledgement replays the exact park"
    );
    assert!(p.confirm_suspension(claim.fence, wait).await.unwrap());
    assert!(matches!(
        p.resume_wait(s.scope).await.unwrap(),
        WaitProgress::Pending
    ));
    assert_eq!(
        s.rows().await,
        json!({"calls":1,"operations":1,"intents":1,"markers":1,"receipts":0,"waits":1})
    );
    let leased: i64 =
        sqlx::query_scalar("SELECT count(*) FROM background_tasks WHERE worker_id IS NOT NULL")
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(leased, 0, "native work owns no execution lease");
    s.native_success().await;
    assert_eq!(p.sweep_waits(8).await.unwrap(), 1);
    assert_eq!(
        p.sweep_waits(8).await.unwrap(),
        0,
        "duplicate native observations create no additional wake"
    );
    assert!(matches!(
        p.resume_wait(s.scope).await.unwrap(),
        WaitProgress::Refused
    ));
    let next = s.claim().await;
    assert_ne!(next.fence.generation, claim.fence.generation);
    let kind = WorkflowStepKind(KIND.into());
    let result = s
        .handler()
        .execute(&kind, &next)
        .await
        .completed_result()
        .unwrap();
    assert_eq!(result["text"], "native output");
    assert_eq!(result["usage"], Value::Null);
    let replay = s
        .handler()
        .execute(&kind, &next)
        .await
        .completed_result()
        .unwrap();
    assert_eq!(result, replay);
    assert!(
        p.complete_io(FencedWorkflowResult {
            fence: next.fence,
            output: result,
            policy: lease()
        })
        .await
        .unwrap()
        .is_some()
    );
    assert_eq!(
        s.rows().await,
        json!({"calls":1,"operations":1,"intents":1,"markers":1,"receipts":1,"waits":1})
    );
}

#[tokio::test]
async fn cloudroom_workflow_result_bounds_utf8_and_classifies_native_uncertainty() {
    for (outcome, success) in [("succeeded", true), ("uncertain", false)] {
        let s = Setup::new().await;
        let claim = s.claim().await;
        s.park(&claim).await;
        s.outcome(outcome, &"€".repeat(6000)).await;
        assert!(matches!(
            s.fixture.persistence.resume_wait(s.scope).await.unwrap(),
            WaitProgress::Continued
        ));
        let next = s.claim().await;
        let result = s
            .handler()
            .execute(&WorkflowStepKind(KIND.into()), &next)
            .await
            .completed_result();
        if success {
            let result = result.unwrap();
            assert!(result["truncated"].as_bool().unwrap());
            assert_eq!(result["text"].as_str().unwrap().len(), 16383);
        } else {
            assert_eq!(
                result.unwrap_err().failure.code().as_str(),
                "local_agent.uncertain"
            );
        }
    }
}

async fn publish_local(fixture: &Fixture, inputs: Value) -> VersionId {
    let mut source: Value = serde_json::from_str(&registry::example(KIND).unwrap().source).unwrap();
    source["workflow_id"] = json!(fixture.target.workflow.to_string());
    source["steps"]["start"]["with"] = inputs;
    graph_support::publish(fixture, source).await
}

#[cfg(unix)]
#[path = "local_agent_e2e_tests.rs"]
mod e2e;
#[path = "local_agent_graph_support.rs"]
mod graph_support;
#[path = "local_agent_retention_tests.rs"]
mod retention;
