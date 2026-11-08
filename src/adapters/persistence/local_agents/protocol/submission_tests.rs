use super::{test_support::*, *};

fn submitted(
    sequence: u64,
    id: OperationId,
    thread: &str,
    state: SubmissionState,
) -> JournalObservation {
    agent(
        sequence,
        id,
        LocalEvidence::Submission {
            thread_id: CloudroomThreadId::try_from(thread.to_owned()).unwrap(),
            state,
        },
    )
}

async fn intent(c: &Context, operation: &LocalAgentOperation) {
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    for observation in [
        agent(3, operation.id(), LocalEvidence::Receipt {}),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn cloudroom_submission_queued_thread_maps_once_without_completing_native_work() {
    let Some(c) = Context::new().await else {
        return;
    };
    let session = c.unmapped_session().await;
    let create = c.create(session);
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(create))
            .await
            .unwrap();
    intent(&c, &operation).await;
    let queued = submitted(5, operation.id(), "thr_queued", SubmissionState::Queued);
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, &queued)
            .await
            .unwrap(),
        5
    );
    assert_eq!(
        c.f.persistence
            .accept_observation(c.fence, &queued)
            .await
            .unwrap(),
        5
    );
    let stored =
        c.f.persistence
            .session(c.f.company, session)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(stored.thread_id.unwrap().as_str(), "thr_queued");
    let saved: (Option<String>, String, Value) = sqlx::query_as("SELECT outcome,observed_turn_state,dispatch FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    assert_eq!(saved.0, None);
    assert_eq!(saved.1, "queued");
    assert!(saved.2["turn_id"].is_null());
    let control = LocalCommand::StopThread {
        session_id: session,
        thread_id: CloudroomThreadId::try_from("thr_queued".to_owned()).unwrap(),
    };
    c.f.persistence
        .enqueue_operation(&c.admission(control))
        .await
        .expect("explicit control can target the known queued thread");
    assert!(matches!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &submitted(6, operation.id(), "thr_other", SubmissionState::Accepted)
            )
            .await,
        Err(AppError::BadRequest(_))
    ));
    let mut completed = turn(
        6,
        operation.id(),
        "turn_after_queue",
        TurnState::Completed,
        Some("correct result"),
    );
    if let JournalObservation::Agent { observation } = &mut completed
        && let LocalEvidence::Turn { thread_id, .. } = &mut observation.evidence
    {
        *thread_id = CloudroomThreadId::try_from("thr_queued".to_owned()).unwrap();
    }
    c.f.persistence
        .accept_observation(c.fence, &completed)
        .await
        .unwrap();
    let result: String = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(result, "succeeded");
}

#[tokio::test]
async fn cloudroom_submission_requires_intent_and_preserves_known_turn_and_foreign_binding() {
    let Some(c) = Context::new().await else {
        return;
    };
    let operation =
        c.f.persistence
            .enqueue_operation(&c.admission(c.send()))
            .await
            .unwrap();
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    let accepted = submitted(3, operation.id(), "thr_frozen", SubmissionState::Accepted);
    assert!(matches!(
        c.f.persistence.accept_observation(c.fence, &accepted).await,
        Err(AppError::BadRequest(_))
    ));
    for observation in [
        agent(3, operation.id(), LocalEvidence::Receipt {}),
        agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
    ] {
        c.f.persistence
            .accept_observation(c.fence, &observation)
            .await
            .unwrap();
    }
    assert!(matches!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &submitted(5, operation.id(), "thr_foreign", SubmissionState::Accepted)
            )
            .await,
        Err(AppError::NotFound(_))
    ));
    c.f.persistence
        .accept_observation(
            c.fence,
            &submitted(5, operation.id(), "thr_frozen", SubmissionState::Accepted),
        )
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(6, operation.id(), "turn_bound", TurnState::Running, None),
        )
        .await
        .unwrap();
    assert!(matches!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &submitted(7, operation.id(), "thr_frozen", SubmissionState::Queued)
            )
            .await,
        Err(AppError::BadRequest(_))
    ));
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                7,
                operation.id(),
                "turn_bound",
                TurnState::Completed,
                Some("native completion"),
            ),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn cloudroom_submission_missing_and_late_identity_preserve_uncertainty_and_capacity() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut admission = c.admission(c.send());
    admission.operation = LocalAgentOperation::queued(
        admission.operation.id(),
        c.send(),
        Utc::now() + chrono::Duration::seconds(2),
    )
    .unwrap();
    let operation = c.f.persistence.enqueue_operation(&admission).await.unwrap();
    intent(&c, &operation).await;
    c.expire(&operation).await;
    for (sequence, state) in [
        (5, SubmissionState::Accepted),
        (6, SubmissionState::Queued),
        (7, SubmissionState::Missing),
    ] {
        c.f.persistence
            .accept_observation(
                c.fence,
                &submitted(sequence, operation.id(), "thr_frozen", state),
            )
            .await
            .unwrap();
        let outcome: String = sqlx::query_scalar(
            "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
        )
        .bind(c.f.company)
        .bind(operation.id().as_uuid())
        .fetch_one(&c.f.database.pool)
        .await
        .unwrap();
        assert_eq!(outcome, "uncertain");
        let mut db = c.f.database.pool.acquire().await.unwrap();
        assert_eq!(
            capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
                .await
                .unwrap()
                .unsettled,
            1
        );
    }
    c.f.persistence
        .accept_observation(
            c.fence,
            &turn(
                8,
                operation.id(),
                "turn_late_submission",
                TurnState::Completed,
                Some("late evidence"),
            ),
        )
        .await
        .unwrap();
    let mut db = c.f.database.pool.acquire().await.unwrap();
    assert_eq!(
        capacity(&mut db, c.f.company, c.paired.receipt.machine.id)
            .await
            .unwrap()
            .unsettled,
        0
    );
    let outcome: String = sqlx::query_scalar(
        "SELECT outcome FROM local_agent_operations WHERE company_id=$1 AND id=$2",
    )
    .bind(c.f.company)
    .bind(operation.id().as_uuid())
    .fetch_one(&mut *db)
    .await
    .unwrap();
    assert_eq!(outcome, "uncertain");
}
