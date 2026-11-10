use super::{test_support::*, *};
use chrono::Timelike;
use tokio::sync::Barrier;

#[path = "constraint_tests.rs"]
mod constraint_tests;

fn sql_state<T: std::fmt::Debug>(result: Result<T, sqlx::Error>, code: &str) {
    let error = result.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some(code),
        "{error}"
    );
}

async fn seed_operation(
    f: &Fixture,
    paired: &Paired,
    command: LocalCommand,
    session: Option<SessionId>,
    dispatch: DispatchEvidence,
) -> OperationId {
    let operation = LocalAgentOperation::queued(
        OperationId::new(Uuid::new_v4()).unwrap(),
        command,
        Utc::now() + chrono::Duration::minutes(30),
    )
    .unwrap();
    sqlx::query("INSERT INTO local_agent_operations (company_id,id,machine_id,installation_id,session_id,logical_key,command,payload_digest,deadline,dispatch) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(paired.receipt.company_id).bind(operation.id().as_uuid()).bind(paired.receipt.machine.id.as_uuid())
        .bind(paired.receipt.machine.installation_id.as_uuid()).bind(session.map(SessionId::as_uuid)).bind(operation.id().to_string())
        .bind(encode(operation.command()).unwrap()).bind(operation.payload_digest().as_str()).bind(operation.deadline()).bind(encode(&dispatch).unwrap())
        .execute(&f.database.pool).await.unwrap();
    operation.id()
}

async fn competing_connections(f: &Fixture, paired: &Paired) -> ConnectionFence {
    let key = paired.candidate.credential.verifier();
    let installation = paired.candidate.claim.installation_id;
    let barrier = Barrier::new(2);
    let a = async {
        barrier.wait().await;
        f.persistence
            .acquire_connection(key, installation, Uuid::new_v4())
            .await
            .unwrap()
            .unwrap()
    };
    let b = async {
        barrier.wait().await;
        f.persistence
            .acquire_connection(key, installation, Uuid::new_v4())
            .await
            .unwrap()
            .unwrap()
    };
    let (a, b) = tokio::join!(a, b);
    let (old, current) = if a.generation < b.generation {
        (a, b)
    } else {
        (b, a)
    };
    f.persistence.release_connection(old).await.unwrap();
    assert!(!f.persistence.renew_connection(old).await.unwrap());
    assert!(f.persistence.renew_connection(current).await.unwrap());
    current
}

async fn revoke_during_connection(f: &Fixture, paired: &Paired) -> Option<ConnectionFence> {
    let key = paired.candidate.credential.verifier();
    let installation = paired.candidate.claim.installation_id;
    let barrier = Barrier::new(2);
    let connect = async {
        barrier.wait().await;
        f.persistence
            .acquire_connection(key, installation, Uuid::new_v4())
            .await
            .unwrap()
    };
    let revoke = async {
        barrier.wait().await;
        f.cases
            .change(
                f.owner,
                f.company,
                paired.receipt.machine.id,
                MachineChange::Revoke,
            )
            .await
            .unwrap()
    };
    let (connection, machine) = tokio::join!(connect, revoke);
    assert_eq!(machine.state, MachineState::Revoked);
    connection
}

async fn assert_foreign_profile_constraints(
    f: &Fixture,
    paired: &Paired,
    foreign: &Paired,
    conversation: Uuid,
    selection: Value,
) {
    let mut wrong = f.profile(paired.receipt.machine.id);
    wrong.company_id = f.foreign;
    sql_state(sqlx::query("INSERT INTO local_agent_profiles (company_id,id,machine_id,name,selection) VALUES ($1,$2,$3,'foreign',$4)")
        .bind(wrong.company_id).bind(wrong.id.as_uuid()).bind(wrong.selection.machine_id.as_uuid()).bind(encode(&wrong.selection).unwrap())
        .execute(&f.database.pool).await,"23503");
    let foreign_profile = f.profile(foreign.receipt.machine.id);
    sqlx::query("INSERT INTO local_agent_profiles (company_id,id,machine_id,name,selection) VALUES ($1,$2,$3,'foreign',$4)")
        .bind(f.foreign).bind(foreign_profile.id.as_uuid()).bind(foreign.receipt.machine.id.as_uuid()).bind(encode(&foreign_profile.selection).unwrap())
        .execute(&f.database.pool).await.unwrap();
    sql_state(sqlx::query("INSERT INTO local_agent_sessions (company_id,id,machine_id,installation_id,profile_id,selection,conversation_id) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(f.company).bind(Uuid::new_v4()).bind(paired.receipt.machine.id.as_uuid()).bind(paired.receipt.machine.installation_id.as_uuid())
        .bind(foreign_profile.id.as_uuid()).bind(selection).bind(conversation).execute(&f.database.pool).await,"23503");
}

struct RevokedOperations {
    queued: OperationId,
    entered: OperationId,
    delivery_gap: OperationId,
}

async fn assert_revoked(
    f: &Fixture,
    paired: &Paired,
    current: ConnectionFence,
    connection: Option<ConnectionFence>,
    operations: RevokedOperations,
) {
    let RevokedOperations {
        queued,
        entered,
        delivery_gap,
    } = operations;
    let key = paired.candidate.credential.verifier();
    let installation = paired.candidate.claim.installation_id;
    if let Some(connection) = connection {
        assert!(!f.persistence.renew_connection(connection).await.unwrap());
    }
    assert!(!f.persistence.renew_connection(current).await.unwrap());
    assert!(
        f.persistence
            .acquire_connection(key, installation, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.persistence
            .pairing_status(installation, paired.candidate.secret.verifier())
            .await
            .unwrap()
            .is_none()
    );
    let outcomes: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,outcome FROM local_agent_operations ORDER BY id")
            .fetch_all(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(
        outcomes
            .iter()
            .find(|(id, _)| *id == queued.as_uuid())
            .unwrap()
            .1,
        "cancelled"
    );
    assert_eq!(
        outcomes
            .iter()
            .find(|(id, _)| *id == entered.as_uuid())
            .unwrap()
            .1,
        "uncertain"
    );
    assert_eq!(
        outcomes
            .iter()
            .find(|(id, _)| *id == delivery_gap.as_uuid())
            .unwrap()
            .1,
        "uncertain"
    );
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_machine_credentials WHERE revoked_at IS NULL",
    )
    .fetch_one(&f.database.pool)
    .await
    .unwrap();
    assert_eq!(active, 0);
}

async fn assert_profile_refusals(f: &Fixture, mut profile: LocalAgentProfile) {
    profile.selection.policy.reasoning = LocalReasoningLevel::Ultra;
    assert!(matches!(
        f.cases.save_profile(f.owner, &profile).await,
        Err(AppError::BadRequest(_))
    ));
    f.cases
        .change(
            f.owner,
            f.company,
            profile.selection.machine_id,
            MachineChange::Disable,
        )
        .await
        .unwrap();
    assert!(matches!(
        f.cases.save_profile(f.owner, &profile).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn cloudroom_revocation_fences_competing_connections_and_preserves_uncertainty() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paired = f.pair(f.company).await;
    let current = competing_connections(&f, &paired).await;
    f.cases
        .change(
            f.owner,
            f.company,
            paired.receipt.machine.id,
            MachineChange::Disable,
        )
        .await
        .unwrap();
    assert!(f.persistence.renew_connection(current).await.unwrap());
    let queued = seed_operation(
        &f,
        &paired,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::NotDispatched {},
    )
    .await;
    let entered = seed_operation(
        &f,
        &paired,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::SubmissionIntent {
            sequence: EventSequence::try_from(1).unwrap(),
        },
    )
    .await;
    let delivery_gap = seed_operation(
        &f,
        &paired,
        LocalCommand::RefreshCatalog {},
        None,
        DispatchEvidence::NotDispatched {},
    )
    .await;
    sqlx::query("UPDATE local_agent_operations SET delivery_attempted_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(delivery_gap.as_uuid()).execute(&f.database.pool).await.unwrap();
    sql_state(sqlx::query("UPDATE local_agent_operations SET delivery_attempted_at=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(delivery_gap.as_uuid()).execute(&f.database.pool).await,"23514");
    let connection = revoke_during_connection(&f, &paired).await;
    assert_revoked(
        &f,
        &paired,
        current,
        connection,
        RevokedOperations {
            queued,
            entered,
            delivery_gap,
        },
    )
    .await;
}

#[tokio::test]
async fn cloudroom_revocation_repair_is_new_and_installation_reservation_survives_company_deletion()
{
    let Some(f) = Fixture::new().await else {
        return;
    };
    let old = f.pair(f.company).await;
    let replacement = f
        .cases
        .create_pairing(f.owner, f.company, Some(old.receipt.machine.id))
        .await
        .unwrap();
    assert_eq!(
        f.cases
            .machine(f.owner, f.company, old.receipt.machine.id)
            .await
            .unwrap()
            .state,
        MachineState::Revoked
    );
    let mut reused = Candidate::new(replacement.code.clone());
    reused.claim.installation_id = old.candidate.claim.installation_id;
    assert!(
        f.persistence
            .claim_pairing(&reused.claim)
            .await
            .unwrap()
            .is_none()
    );
    let fresh = Candidate::new(replacement.code);
    let repaired = f
        .persistence
        .claim_pairing(&fresh.claim)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(repaired.machine.id, old.receipt.machine.id);
    assert_ne!(
        repaired.machine.installation_id,
        old.receipt.machine.installation_id
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.company)
        .execute(&f.database.pool)
        .await
        .unwrap();
    let reservation: Uuid =
        sqlx::query_scalar("SELECT company_id FROM local_installations WHERE installation_id=$1")
            .bind(old.candidate.claim.installation_id.as_uuid())
            .fetch_one(&f.database.pool)
            .await
            .unwrap();
    assert_eq!(reservation, f.company);
    let foreign = f
        .cases
        .create_pairing(f.owner, f.foreign, None)
        .await
        .unwrap();
    let mut moved = Candidate::new(foreign.code.clone());
    moved.claim.installation_id = old.candidate.claim.installation_id;
    assert!(
        f.persistence
            .claim_pairing(&moved.claim)
            .await
            .unwrap()
            .is_none()
    );
    let new = Candidate::new(foreign.code);
    assert!(
        f.persistence
            .claim_pairing(&new.claim)
            .await
            .unwrap()
            .is_some()
    );
    sql_state(
        sqlx::query("DELETE FROM local_installations WHERE installation_id=$1")
            .bind(old.candidate.claim.installation_id.as_uuid())
            .execute(&f.database.pool)
            .await,
        "23514",
    );
}

#[tokio::test]
async fn cloudroom_company_profiles_sessions_operations_are_scoped_and_frozen() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paired = f.pair(f.company).await;
    let foreign = f.pair(f.foreign).await;
    let SessionSetup {
        mut profile,
        id: session,
        conversation,
        selection,
    } = prepare_session(&f, &paired).await;
    profile.name = "edited profile".into();
    profile.selection.policy.reasoning = LocalReasoningLevel::High;
    f.cases.save_profile(f.owner, &profile).await.unwrap();
    let stored = f
        .persistence
        .profile(f.company, profile.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.name, "edited profile");
    assert_eq!(stored.selection.policy.reasoning, LocalReasoningLevel::High);
    let stored = f
        .persistence
        .session(f.company, session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.selection.policy.reasoning, LocalReasoningLevel::Low);
    assert_eq!(
        stored.association,
        SessionAssociation::Conversation {
            conversation_id: conversation
        }
    );
    assert!(
        f.persistence
            .session(f.foreign, session)
            .await
            .unwrap()
            .is_none()
    );
    sql_state(
        sqlx::query(
            "UPDATE local_agent_sessions SET thread_id='thr_other' WHERE company_id=$1 AND id=$2",
        )
        .bind(f.company)
        .bind(session.as_uuid())
        .execute(&f.database.pool)
        .await,
        "23514",
    );
    assert_foreign_profile_constraints(&f, &paired, &foreign, conversation, selection).await;
    let command = LocalCommand::Send {
        session_id: session,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        prompt: LocalPrompt::try_from("bounded test".to_owned()).unwrap(),
    };
    let operation = seed_operation(
        &f,
        &paired,
        command,
        Some(session),
        DispatchEvidence::NotDispatched {},
    )
    .await;
    sql_state(sqlx::query("UPDATE local_agent_operations SET deadline=deadline+INTERVAL '1 second' WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(operation.as_uuid()).execute(&f.database.pool).await,"23514");
    sql_state(sqlx::query("INSERT INTO local_agent_observations (company_id,machine_id,installation_id,sequence,operation_id,digest,observation) VALUES ($1,$2,$3,1,$4,$5,'{}')")
        .bind(f.foreign).bind(foreign.receipt.machine.id.as_uuid()).bind(foreign.receipt.machine.installation_id.as_uuid()).bind(operation.as_uuid())
        .bind("0".repeat(64)).execute(&f.database.pool).await,"23503");
    assert_profile_refusals(&f, profile).await;
}

#[tokio::test]
async fn cloudroom_company_operation_deadline_round_trip_preserves_digest() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let paired = f.pair(f.company).await;
    let deadline = (Utc::now() + chrono::Duration::minutes(30))
        .with_nanosecond(123_456_789)
        .unwrap();
    let operation = LocalAgentOperation::queued(
        OperationId::new(Uuid::new_v4()).unwrap(),
        LocalCommand::RefreshCatalog {},
        deadline,
    )
    .unwrap();
    sqlx::query("INSERT INTO local_agent_operations (company_id,id,machine_id,installation_id,logical_key,command,payload_digest,deadline) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(f.company).bind(operation.id().as_uuid()).bind(paired.receipt.machine.id.as_uuid()).bind(paired.receipt.machine.installation_id.as_uuid())
        .bind(operation.id().to_string()).bind(encode(operation.command()).unwrap()).bind(operation.payload_digest().as_str()).bind(operation.deadline())
        .execute(&f.database.pool).await.unwrap();
    #[derive(sqlx::FromRow)]
    struct StoredOperation {
        id: Uuid,
        command: Value,
        deadline: DateTime<Utc>,
        payload_digest: String,
        dispatch: Value,
        outcome: Option<String>,
    }
    let stored=sqlx::query_as::<_,StoredOperation>("SELECT id,command,deadline,payload_digest,dispatch,outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(f.company).bind(operation.id().as_uuid()).fetch_one(&f.database.pool).await.unwrap();
    let recovered:LocalAgentOperation=serde_json::from_value(json!({"id":stored.id,"command":stored.command,
        "deadline":stored.deadline,"payload_digest":stored.payload_digest,"dispatch":stored.dispatch,"outcome":stored.outcome}))
        .expect("a stored frozen operation must decode with its original digest");
    assert_eq!(recovered.payload_digest(), operation.payload_digest());
    assert_eq!(recovered.deadline(), operation.deadline());
}
