use super::*;

#[tokio::test]
async fn cloudroom_commands_accepted_controls_still_bound_the_outstanding_queue() {
    let Some(c) = Context::new().await else {
        return;
    };
    for _ in 0..MAX_QUEUED_COMMANDS {
        c.f.persistence
            .enqueue_operation(&c.admission(LocalCommand::Inspect {
                session_id: c.session.id,
                thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
            }))
            .await
            .unwrap();
    }
    let mut sequence = 3;
    for _ in 0..4 {
        let deliveries = c.f.persistence.pending_commands(c.fence).await.unwrap();
        assert_eq!(deliveries.len(), 8);
        for delivery in deliveries {
            c.f.persistence
                .accept_observation(
                    c.fence,
                    &agent(sequence, delivery.operation.id(), LocalEvidence::Receipt {}),
                )
                .await
                .unwrap();
            sequence += 1;
        }
    }
    let extra = c.admission(LocalCommand::StopThread {
        session_id: c.session.id,
        thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
    });
    assert!(matches!(
        c.f.persistence.enqueue_operation(&extra).await,
        Err(AppError::Conflict(_))
    ));
    c.reconnect(c.journal, sequence - 1).await;
    let outstanding: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM local_agent_operations WHERE company_id=$1 AND outcome IS NULL",
    )
    .bind(c.f.company)
    .fetch_one(&c.f.database.pool)
    .await
    .unwrap();
    assert_eq!(outstanding, 32);
}

#[tokio::test]
async fn cloudroom_commands_live_actor_withdrawal_races_first_dispatch_without_false_refusal() {
    let Some(c) = Context::new().await else {
        return;
    };
    let mut admission = c.admission(c.send());
    admission.actor_id = c.f.admin;
    let operation = c.f.persistence.enqueue_operation(&admission).await.unwrap();
    let barrier = Barrier::new(2);
    let delivery = async {
        barrier.wait().await;
        c.f.persistence.pending_commands(c.fence).await.unwrap()
    };
    let withdrawal = async {
        barrier.wait().await;
        sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
            .bind(c.f.company)
            .bind(c.f.admin)
            .execute(&c.f.database.pool)
            .await
            .unwrap();
    };
    let (delivery, ()) = tokio::join!(delivery, withdrawal);
    let state:(Option<String>,Option<DateTime<Utc>>)=sqlx::query_as("SELECT outcome,delivery_attempted_at FROM local_agent_operations WHERE company_id=$1 AND id=$2")
        .bind(c.f.company).bind(operation.id().as_uuid()).fetch_one(&c.f.database.pool).await.unwrap();
    if delivery.iter().any(|d| d.operation.id() == operation.id()) {
        assert!(state.0.is_none());
        assert!(state.1.is_some());
        let replay = c.f.persistence.pending_commands(c.fence).await.unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].mode, DeliveryMode::Reconcile);
    } else {
        assert_eq!(state.0.as_deref(), Some("cancelled"));
        assert!(state.1.is_none());
    }
}

#[tokio::test]
async fn cloudroom_commands_foreign_session_and_observation_are_missing_to_an_authorized_manager() {
    let Some(c) = Context::new().await else {
        return;
    };
    let foreign = c.f.pair(c.f.foreign).await;
    let mut admission = c.admission(c.send());
    admission.company_id = c.f.foreign;
    admission.machine_id = foreign.receipt.machine.id;
    assert!(matches!(
        c.f.persistence.enqueue_operation(&admission).await,
        Err(AppError::NotFound(_))
    ));
    admission.operation = LocalAgentOperation::queued(
        admission.operation.id(),
        LocalCommand::RefreshCatalog {},
        admission.operation.deadline(),
    )
    .unwrap();
    let operation = c.f.persistence.enqueue_operation(&admission).await.unwrap();
    assert!(matches!(
        c.f.persistence
            .accept_observation(
                c.fence,
                &agent(3, operation.id(), LocalEvidence::Receipt {})
            )
            .await,
        Err(AppError::NotFound(_))
    ));
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
async fn cloudroom_commands_create_binds_once_and_logical_replay_survives_native_mapping() {
    let Some(c) = Context::new().await else {
        return;
    };
    let session = c.unmapped_session().await;
    let create = c.create(session);
    let admission = c.admission(create.clone());
    let operation = c.f.persistence.enqueue_operation(&admission).await.unwrap();
    assert!(matches!(
        c.f.persistence
            .enqueue_operation(&c.admission(create))
            .await,
        Err(AppError::Conflict(_))
    ));
    c.f.persistence.pending_commands(c.fence).await.unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(3, operation.id(), LocalEvidence::Receipt {}),
        )
        .await
        .unwrap();
    c.f.persistence
        .accept_observation(
            c.fence,
            &agent(4, operation.id(), LocalEvidence::SubmissionIntent {}),
        )
        .await
        .unwrap();
    let observation = agent(
        5,
        operation.id(),
        LocalEvidence::Turn {
            thread_id: CloudroomThreadId::try_from("thr_created".to_owned()).unwrap(),
            turn_id: CloudroomTurnId::try_from("turn_created".to_owned()).unwrap(),
            state: TurnState::Running,
            output: None,
            truncated: false,
            usage: None,
        },
    );
    c.f.persistence
        .accept_observation(c.fence, &observation)
        .await
        .unwrap();
    let replay = c.f.persistence.enqueue_operation(&admission).await.unwrap();
    assert_eq!(replay.id(), operation.id());
    let stored =
        c.f.persistence
            .session(c.f.company, session)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(stored.thread_id.unwrap().as_str(), "thr_created");
    let mapped = c.admission(LocalCommand::Create {
        harness: crate::domain::local_agents::NativeHarnessSelection::Unconfigured,
        session_id: c.session.id,
        selection: c.session.profile.selection.clone(),
        prompt: LocalPrompt::try_from("must not replace mapping".to_owned()).unwrap(),
    });
    assert!(matches!(
        c.f.persistence.enqueue_operation(&mapped).await,
        Err(AppError::Conflict(_))
    ));
}
