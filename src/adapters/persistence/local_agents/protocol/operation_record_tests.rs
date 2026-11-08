use super::{test_support::*, *};

#[tokio::test]
async fn cloudroom_commands_persisted_command_session_binding_is_checked_beyond_its_digest() {
    let Some(c) = Context::new().await else {
        return;
    };
    let command = c.send();
    let operation = c.admission(command.clone()).operation;
    let record = OperationDb {
        origin: "direct".into(),
        outstanding: true,
        id: operation.id().as_uuid(),
        command: encode(&command).unwrap(),
        deadline: operation.deadline(),
        payload_digest: operation.payload_digest().as_str().to_owned(),
        dispatch: encode(operation.dispatch()).unwrap(),
        outcome: None,
        session_id: Some(Uuid::new_v4()),
        actor_id: Some(c.f.owner),
        delivery_attempted_at: None,
    };
    assert!(
        record.operation().is_err(),
        "a matching payload digest does not authorize a different row session"
    );
    let commands = [
        command,
        LocalCommand::Create {
            session_id: c.session.id,
            selection: c.session.profile.selection.clone(),
            prompt: LocalPrompt::try_from("binding proof".to_owned()).unwrap(),
        },
        LocalCommand::Inspect {
            session_id: c.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        },
        LocalCommand::StopThread {
            session_id: c.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        },
        LocalCommand::ArchiveThread {
            session_id: c.session.id,
            thread_id: CloudroomThreadId::try_from("thr_frozen".to_owned()).unwrap(),
        },
    ];
    for command in commands {
        let request = c.admission(command);
        let mut valid = c.f.database.pool.begin().await.unwrap();
        insert(&mut valid, &c, &request.operation, c.session.id)
            .await
            .unwrap();
        valid.rollback().await.unwrap();
        let mut invalid = c.f.database.pool.begin().await.unwrap();
        let error = insert(
            &mut invalid,
            &c,
            &request.operation,
            SessionId::new(Uuid::new_v4()).unwrap(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("local_operation_command_session")
        );
        invalid.rollback().await.unwrap();
        assert_create_machine_binding(&c, &request.operation).await;
    }
}

async fn assert_create_machine_binding(c: &Context, operation: &LocalAgentOperation) {
    let LocalCommand::Create {
        session_id,
        selection,
        prompt,
    } = operation.command()
    else {
        return;
    };
    let mut changed = selection.clone();
    changed.machine_id = MachineId::new(Uuid::new_v4()).unwrap();
    let forged = LocalAgentOperation::queued(
        operation.id(),
        LocalCommand::Create {
            session_id: *session_id,
            selection: changed,
            prompt: prompt.clone(),
        },
        operation.deadline(),
    )
    .unwrap();
    let mut tx = c.f.database.pool.begin().await.unwrap();
    let error = insert(&mut tx, c, &forged, *session_id).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("local_operation_create_machine")
    );
    tx.rollback().await.unwrap();
}

async fn insert(
    db: &mut PgConnection,
    c: &Context,
    operation: &LocalAgentOperation,
    session: SessionId,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,session_id,logical_key,command,payload_digest,deadline) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(c.f.company).bind(operation.id().as_uuid()).bind(c.paired.receipt.machine.id.as_uuid())
        .bind(c.paired.receipt.machine.installation_id.as_uuid()).bind(session.as_uuid()).bind(operation.id().to_string())
        .bind(encode(operation.command()).unwrap()).bind(operation.payload_digest().as_str()).bind(operation.deadline())
        .execute(db).await?;
    Ok(())
}
