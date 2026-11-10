use super::*;
use tokio::sync::Barrier;

#[tokio::test]
async fn cloudroom_observations_lease_expiry_is_rechecked_after_an_actual_row_lock_wait() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    let receipt = agent(3, operation.id(), LocalEvidence::Receipt {});
    let mut held = c.f.database.pool.begin().await.unwrap();
    sqlx::query("UPDATE local_machines SET connection_expires_at=clock_timestamp()+INTERVAL '100 milliseconds' WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(c.paired.receipt.machine.id.as_uuid()).execute(&mut *held).await.unwrap();
    let observation = c.f.persistence.accept_observation(c.fence, &receipt);
    let release = async {
        tokio::time::timeout(std::time::Duration::from_secs(2),async {loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE 'SELECT state,journal_id,%')")
                .fetch_one(&c.f.database.pool).await.unwrap();
            if waiting {break;} tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }}).await.expect("the production fence read must actually wait for the held row");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        held.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(observation, release);
    assert!(matches!(result, Err(AppError::InvalidCredentials)));
    let cursor: i64 = sqlx::query_scalar(
        "SELECT acknowledged_sequence FROM local_machines WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(c.paired.receipt.machine.id.as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(cursor, 2);
}

#[tokio::test]
async fn cloudroom_observations_competing_sequence_claims_commit_only_one_content() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    let receipt = agent(3, operation.id(), LocalEvidence::Receipt {});
    let uncertain = agent(3, operation.id(), LocalEvidence::Uncertain {});
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        c.f.persistence.accept_observation(c.fence, &receipt).await
    };
    let second = async {
        barrier.wait().await;
        c.f.persistence
            .accept_observation(c.fence, &uncertain)
            .await
    };
    let (a, b) = tokio::join!(first, second);
    assert_ne!(a.is_ok(), b.is_ok());
    let winner = if a.is_ok() { &receipt } else { &uncertain };
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, winner)
            .await
            .unwrap(),
        3
    );
    let saved: (i64, String) = sqlx::query_as(
        "SELECT sequence,digest FROM local_agent_observations WHERE company_id=$1 AND sequence=3",
    )
    .bind(c.f.company)
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(
        saved,
        (3, winner.canonical_digest().unwrap().as_str().to_owned())
    );
}

#[tokio::test]
async fn cloudroom_observations_thread_stop_is_separate_and_allowed_after_disable() {
    let Some(c) = Context::new().await else {
        return;
    };
    let original =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    for observation in [
        agent(3, original.id(), LocalEvidence::Receipt {}),
        agent(4, original.id(), LocalEvidence::SubmissionIntent {}),
        turn(5, original.id(), "turn_original", TurnState::Running, None),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
    c.f.cases
        .change(
            c.f.owner,
            c.f.company,
            c.paired.receipt.machine.id,
            MachineChange::Disable,
        )
        .await
        .unwrap();
    assert!(
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .is_err()
    );
    let control =
        c.f.persistence
            .enqueue_operation(&c.admission(LocalCommand::StopThread {
                session_id: c.session.id,
                thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            }))
            .await
            .unwrap();
    let delivery = c.f.persistence.pending_commands(c.fence).await.unwrap();
    assert!(
        delivery
            .iter()
            .any(|item| item.operation.id() == control.id() && item.mode == DeliveryMode::Dispatch)
    );
    c.f.persistence
        .accept_observation(c.fence, &agent(6, control.id(), LocalEvidence::Receipt {}))
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(7, control.id(), LocalEvidence::SubmissionIntent {}),
        )
        .await
        .unwrap();
    let wrong = JournalObservation::Control {
        sequence: EventSequence::try_from(8).unwrap(),
        operation_id: control.id(),
        thread_id: CloudroomThreadId::try_from("thr_foreign".to_owned()).unwrap(),
        evidence: ControlEvidence::StopAccepted {},
    };
    assert!(
        c.f.persistence
            .accept_observation(c.fence, &wrong)
            .await
            .is_err()
    );
    let stopped = JournalObservation::Control {
        sequence: EventSequence::try_from(8).unwrap(),
        operation_id: control.id(),
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        evidence: ControlEvidence::StopAccepted {},
    };
    c.f.persistence
        .accept_observation(c.fence, &stopped)
        .await
        .unwrap();
    let outcome: Option<String> = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(original.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert!(
        outcome.is_none(),
        "a stop acknowledgment cannot settle another turn"
    );
}

#[tokio::test]
async fn cloudroom_observations_one_native_turn_cannot_settle_two_operations() {
    let Some(c) = Context::new().await else {
        return;
    };
    let a =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    let b =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    for observation in [
        agent(3, a.id(), LocalEvidence::Receipt {}),
        agent(4, a.id(), LocalEvidence::SubmissionIntent {}),
        turn(5, a.id(), "turn_once", TurnState::Running, None),
        agent(6, b.id(), LocalEvidence::Receipt {}),
        agent(7, b.id(), LocalEvidence::SubmissionIntent {}),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
    assert!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &turn(
                    8,
                    b.id(),
                    "turn_once",
                    TurnState::Completed,
                    Some("wrong operation")
                )
            )
            .await
            .is_err()
    );
    let state:(i64,String)=sqlx::query_as("SELECT machine.acknowledged_sequence,operation.dispatch->>'kind' FROM local_machines AS machine JOIN local_agent_operations AS operation ON operation.company_id=machine.company_id AND operation.machine_id=machine.id WHERE machine.company_id=$1 AND operation.id=$2")
        .bind(c.f.company).bind(b.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(state, (7, "submission_intent".to_owned()));
}

#[tokio::test]
async fn cloudroom_observations_late_result_is_audit_without_successful_wait() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut request = c.admission(c.send());
    request.operation = LocalAgentOperation::queued(
        request.operation.id(),
        request.operation.command().clone(),
        Utc::now() + chrono::Duration::seconds(3),
    )
    .unwrap();
    let operation = c.f.persistence.enqueue_operation(&request).await.unwrap();
    assert!(
        c.f.persistence
            .pending_commands(c.fence)
            .await
            .unwrap()
            .iter()
            .any(|d| d.operation.id() == operation.id())
    );
    for observation in [
        agent(3, operation.id(), LocalEvidence::Receipt {}),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
        turn(5, operation.id(), "turn_late", TurnState::Running, None),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
    let wait = operation
        .deadline()
        .signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or_default()
        + std::time::Duration::from_millis(50);
    tokio::time::sleep(wait).await;
    assert_eq!(c.f.persistence.expire_operations(100).await.unwrap(), 1);
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                6,
                operation.id(),
                "turn_late",
                TurnState::Completed,
                Some("late result"),
            ),
        )
        .await
        .unwrap();
    let saved: (String, String) = sqlx::query_as(
        "SELECT outcome,output FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(saved, ("uncertain".to_owned(), "late result".to_owned()));
    let mut db = c.f.database.pool.acquire().await.unwrap();
    let counts = capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
        .await
        .unwrap();
    assert_eq!(
        counts.unsettled, 0,
        "known late completion frees native capacity without rewriting the expired wait"
    );
}
