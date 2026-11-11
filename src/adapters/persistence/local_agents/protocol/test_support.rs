use super::*;
use crate::adapters::persistence::local_agents::test_support::{
    Fixture, Paired, SessionSetup, prepare_session,
};

pub(crate) struct Context {
    pub f: Fixture,
    pub paired: Paired,
    pub session: SessionSetup,
    pub journal: Uuid,
    pub fence: ConnectionFence,
}

impl Context {
    pub async fn new() -> Option<Self> {
        let f = Fixture::new().await?;
        let paired = f.pair(f.company).await;
        let journal = Uuid::new_v4();
        let fence = f
            .persistence
            .acquire_connection(
                paired.candidate.credential.verifier(),
                paired.receipt.machine.installation_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap()
            .unwrap();
        let hello = JournalHello {
            version: 2,
            installation_id: fence.machine.installation_id,
            journal_id: journal,
            journal_sequence: 0,
        };
        assert!(
            !f.persistence
                .connect_journal(fence, &hello)
                .await
                .unwrap()
                .recovery_required
        );
        let session = prepare_session(&f, &paired).await;
        assert!(f.persistence.finish_reconciliation(fence).await.unwrap());
        let deliveries = f.persistence.pending_commands(fence).await.unwrap();
        assert_eq!(deliveries.len(), 1);
        assert!(matches!(
            deliveries[0].operation.command(),
            LocalCommand::RefreshCatalog {}
        ));
        let id = deliveries[0].operation.id();
        let receipt = agent(1, id, LocalEvidence::Receipt {});
        assert_eq!(
            f.persistence
                .accept_observation(fence, &receipt)
                .await
                .unwrap(),
            1
        );
        let catalog = f
            .persistence
            .capabilities(
                f.company,
                paired.receipt.machine.id,
                EventSequence::try_from(1).unwrap(),
            )
            .await
            .unwrap()
            .unwrap()
            .catalog;
        let observation = JournalObservation::Catalog {
            sequence: EventSequence::try_from(2).unwrap(),
            operation_id: id,
            catalog,
        };
        assert_eq!(
            f.persistence
                .accept_observation(fence, &observation)
                .await
                .unwrap(),
            2
        );
        Some(Self {
            f,
            paired,
            session,
            journal,
            fence,
        })
    }
    pub fn send(&self) -> LocalCommand {
        LocalCommand::Send {
            session_id: self.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            prompt: LocalPrompt::try_from("bounded protocol proof".to_owned()).unwrap(),
        }
    }
    pub fn create(&self, session_id: SessionId) -> LocalCommand {
        LocalCommand::Create {
            session_id,
            selection: self.session.profile.selection.clone(),
            prompt: LocalPrompt::try_from("new native thread".to_owned()).unwrap(),
        }
    }
    pub async fn expire(&self, operation: &LocalAgentOperation) {
        let wait = operation
            .deadline()
            .signed_duration_since(Utc::now())
            .to_std()
            .unwrap_or_default()
            + std::time::Duration::from_millis(30);
        tokio::time::sleep(wait).await;
        assert_eq!(self.f.persistence.expire_operations(100).await.unwrap(), 1);
    }
    pub fn admission(&self, command: LocalCommand) -> OperationAdmission {
        let id = OperationId::new(Uuid::new_v4()).unwrap();
        OperationAdmission {
            company_id: self.f.company,
            actor_id: self.f.owner,
            machine_id: self.paired.receipt.machine.id,
            logical_key: LogicalInvocationKey::parse(id.to_string()).unwrap(),
            operation: LocalAgentOperation::queued(
                id,
                command,
                Utc::now() + chrono::Duration::minutes(30),
            )
            .unwrap(),
        }
    }
    pub async fn unmapped_session(&self) -> SessionId {
        let session = SessionId::new(Uuid::new_v4()).unwrap();
        sqlx::query("INSERT INTO local_agent_sessions(company_id,id,machine_id,installation_id,profile_id,selection,conversation_id) SELECT company_id,$3,machine_id,installation_id,profile_id,selection,conversation_id FROM local_agent_sessions WHERE company_id=$1 AND id=$2")
            .bind(self.f.company).bind(self.session.id.as_uuid()).bind(session.as_uuid()).execute(&self.f.database.pool).await.unwrap();
        session
    }
    pub async fn reconnect(&self, journal: Uuid, sequence: u64) -> (ConnectionFence, Welcome) {
        let fence = self
            .f
            .persistence
            .acquire_connection(
                self.paired.candidate.credential.verifier(),
                self.paired.receipt.machine.installation_id,
                Uuid::new_v4(),
            )
            .await
            .unwrap()
            .unwrap();
        let welcome = self
            .f
            .persistence
            .connect_journal(
                fence,
                &JournalHello {
                    version: 2,
                    installation_id: fence.machine.installation_id,
                    journal_id: journal,
                    journal_sequence: sequence,
                },
            )
            .await
            .unwrap();
        (fence, welcome)
    }
}

pub(crate) fn agent(sequence: u64, id: OperationId, evidence: LocalEvidence) -> JournalObservation {
    JournalObservation::Agent {
        observation: LocalAgentObservation {
            sequence: EventSequence::try_from(sequence).unwrap(),
            operation_id: id,
            evidence,
        },
    }
}

pub(crate) fn turn(
    sequence: u64,
    id: OperationId,
    turn_id: &str,
    state: TurnState,
    output: Option<&str>,
) -> JournalObservation {
    agent(
        sequence,
        id,
        LocalEvidence::Turn {
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            turn_id: CloudroomTurnId::try_from(turn_id.to_owned()).unwrap(),
            state,
            output: output.map(|v| LocalOutput::try_from(v.to_owned()).unwrap()),
            truncated: false,
            usage: None,
        },
    )
}
