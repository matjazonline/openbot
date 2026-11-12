use super::*;

pub(super) async fn refresh(
    p: &PostgresPersistence,
    actor: Uuid,
    company: Uuid,
    machine: MachineId,
    request: OperationId,
) -> AppResult<LocalAgentOperation> {
    let mut tx = begin(&p.pool).await?;
    authorize(&mut tx, company, actor).await?;
    let locked = lock_machine(&mut tx, company, machine).await?;
    if let Some(saved) = operation_on(&mut tx, company, request).await? {
        let selected: bool = sqlx::query_scalar(
            "SELECT machine_id=$3 FROM local_agent_operations WHERE company_id=$1 AND id=$2",
        )
        .bind(company)
        .bind(request.as_uuid())
        .bind(machine.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if !selected
            || saved.origin != OperationOrigin::Direct
            || !matches!(saved.operation.command(), LocalCommand::RefreshCatalog {})
        {
            return Err(AppError::Conflict("Request identity inputs changed".into()));
        }
        verify_actor_on(&mut tx, company, request, actor).await?;
        tx.commit().await?;
        return Ok(saved.operation);
    }
    if locked.state != "enabled" {
        return Err(AppError::Conflict("Machine is not enabled".into()));
    }
    let command = LocalCommand::RefreshCatalog {};
    shared::validate_capacity_on(&mut tx, CompanyId::new(company), machine, &command).await?;
    let operation = frozen_operation_on(&mut tx, request, command).await?;
    let admission = OperationAdmission {
        company_id: company,
        actor_id: actor,
        machine_id: machine,
        logical_key: LogicalInvocationKey::parse(format!("direct:{request}"))?,
        operation: operation.clone(),
    };
    protocol::commands::insert_admission_on(
        &mut tx,
        &admission,
        InstallationId::new(locked.installation_id).map_err(|_| invalid("installation"))?,
        OperationOrigin::Direct,
    )
    .await?;
    tx.commit().await?;
    Ok(operation)
}

pub(super) async fn start(
    p: &PostgresPersistence,
    request: &StartSession,
) -> AppResult<LocalOperationView> {
    let mut tx = begin(&p.pool).await?;
    authorize(&mut tx, request.company, request.actor).await?;
    let association = SessionAssociation::Conversation {
        conversation_id: request.conversation_id,
    };
    authorize_association_on(&mut tx, request.company, request.actor, &association).await?;
    if let Some(saved) = operation_on(&mut tx, request.company, request.request_id).await? {
        let session = shared::session_on(
            &mut tx,
            CompanyId::new(request.company),
            SessionId::new(request.request_id.as_uuid())
                .map_err(|_| invalid("session identity"))?,
        )
        .await?
        .ok_or_else(missing)?;
        if saved.origin != OperationOrigin::Direct
            || session.profile_id != request.profile_id
            || session.association != association
            || !matches!(saved.operation.command(),LocalCommand::Create { prompt,.. } if prompt==&request.prompt)
        {
            return Err(AppError::Conflict("Request identity inputs changed".into()));
        }
        verify_actor_on(&mut tx, request.company, request.request_id, request.actor).await?;
        tx.commit().await?;
        return Ok(saved);
    }
    let locked = shared::capture_profile_on(
        &mut tx,
        CompanyId::new(request.company),
        request.actor,
        request.profile_id,
    )
    .await?;
    if locked.machine.state != MachineState::Enabled {
        return Err(AppError::Conflict("Machine is not enabled".into()));
    }
    shared::validate_selection_on(
        &mut tx,
        CompanyId::new(request.company),
        locked.machine.id,
        &locked.profile.selection,
    )
    .await?;
    let session = LocalAgentSession {
        id: SessionId::new(request.request_id.as_uuid())
            .map_err(|_| invalid("session identity"))?,
        profile_id: request.profile_id,
        company_id: request.company,
        installation_id: locked.machine.installation_id,
        selection: locked.profile.selection,
        harness: locked.harness,
        association,
        thread_id: None,
    };
    let command = LocalCommand::Create {
        session_id: session.id,
        selection: session.selection.clone(),
        harness: session.harness.clone(),
        prompt: request.prompt.clone(),
    };
    shared::validate_capacity_on(
        &mut tx,
        CompanyId::new(request.company),
        locked.machine.id,
        &command,
    )
    .await?;
    let operation = frozen_operation_on(&mut tx, request.request_id, command).await?;
    shared::insert_session_on(&mut tx, &session).await?;
    insert_on(&mut tx, request.actor, &session, operation).await?;
    let saved = operation_on(&mut tx, request.company, request.request_id)
        .await?
        .ok_or_else(missing)?;
    tx.commit().await?;
    Ok(saved)
}

pub(super) async fn control(
    p: &PostgresPersistence,
    request: &ControlSession,
) -> AppResult<LocalOperationView> {
    let mut tx = begin(&p.pool).await?;
    let session =
        authorized_session_on(&mut tx, request.company, request.actor, request.session_id).await?;
    let thread = session.thread_id.clone().ok_or_else(|| {
        AppError::Conflict("Thread identity is not known; reconcile in Cloudroom".into())
    })?;
    let command = match &request.command {
        SessionCommand::Send { prompt } => LocalCommand::Send {
            session_id: session.id,
            thread_id: thread,
            prompt: prompt.clone(),
        },
        SessionCommand::Inspect => LocalCommand::Inspect {
            session_id: session.id,
            thread_id: thread,
        },
        SessionCommand::Stop => LocalCommand::StopThread {
            session_id: session.id,
            thread_id: thread,
        },
        SessionCommand::Archive => LocalCommand::ArchiveThread {
            session_id: session.id,
            thread_id: thread,
        },
    };
    if let Some(saved) = operation_on(&mut tx, request.company, request.request_id).await? {
        if saved.origin != OperationOrigin::Direct || saved.operation.command() != &command {
            return Err(AppError::Conflict("Request identity inputs changed".into()));
        }
        verify_actor_on(&mut tx, request.company, request.request_id, request.actor).await?;
        tx.commit().await?;
        return Ok(saved);
    }
    if command.is_effect() {
        super::super::native::selected_on(
            &mut tx,
            request.company,
            request.actor,
            &session.harness,
        )
        .await?;
    }
    let machine = lock_machine(&mut tx, request.company, session.selection.machine_id).await?;
    if machine.installation_id != session.installation_id.as_uuid()
        || machine.state == "revoked"
        || (machine.state != "enabled" && command.is_effect())
    {
        return Err(AppError::Conflict(
            "Machine cannot accept this command".into(),
        ));
    }
    if command.is_effect() {
        shared::validate_selection_on(
            &mut tx,
            CompanyId::new(request.company),
            session.selection.machine_id,
            &session.selection,
        )
        .await?;
    }
    shared::validate_capacity_on(
        &mut tx,
        CompanyId::new(request.company),
        session.selection.machine_id,
        &command,
    )
    .await?;
    let operation = frozen_operation_on(&mut tx, request.request_id, command).await?;
    insert_on(&mut tx, request.actor, &session, operation).await?;
    let saved = operation_on(&mut tx, request.company, request.request_id)
        .await?
        .ok_or_else(missing)?;
    tx.commit().await?;
    Ok(saved)
}

async fn verify_actor_on(
    db: &mut PgConnection,
    company: Uuid,
    id: OperationId,
    actor: Uuid,
) -> AppResult<()> {
    let matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_agent_operations WHERE company_id=$1 AND id=$2 AND actor_id=$3)")
        .bind(company).bind(id.as_uuid()).bind(actor).fetch_one(db).await?;
    if !matches {
        return Err(missing());
    }
    Ok(())
}

async fn frozen_operation_on(
    db: &mut PgConnection,
    id: OperationId,
    command: LocalCommand,
) -> AppResult<LocalAgentOperation> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *db)
        .await?;
    let timestamp = id
        .as_uuid()
        .get_timestamp()
        .filter(|_| id.as_uuid().get_version_num() == 7)
        .ok_or_else(|| {
            AppError::BadRequest("New local requests require a fresh UUIDv7 request_id".into())
        })?;
    let (seconds, nanos) = timestamp.to_unix();
    let issued = DateTime::<Utc>::from_timestamp(
        i64::try_from(seconds)
            .map_err(|_| AppError::BadRequest("Invalid request timestamp".into()))?,
        nanos,
    )
    .ok_or_else(|| AppError::BadRequest("Invalid request timestamp".into()))?;
    if issued < now - chrono::Duration::hours(24) || issued > now + chrono::Duration::minutes(2) {
        return Err(AppError::Conflict(
            "Request expired; reload the form or use a new UUIDv7 request_id".into(),
        ));
    }
    let deadline: DateTime<Utc> =
        sqlx::query_scalar("SELECT clock_timestamp()+make_interval(secs=>$1)")
            .bind(DEFAULT_DEADLINE_SECONDS as f64)
            .fetch_one(db)
            .await?;
    LocalAgentOperation::queued(id, command, deadline).map_err(|_| invalid("operation deadline"))
}

async fn insert_on(
    db: &mut PgConnection,
    actor: Uuid,
    session: &LocalAgentSession,
    operation: LocalAgentOperation,
) -> AppResult<()> {
    let admission = OperationAdmission {
        company_id: session.company_id,
        actor_id: actor,
        machine_id: session.selection.machine_id,
        logical_key: LogicalInvocationKey::parse(format!("direct:{}", operation.id()))?,
        operation,
    };
    protocol::commands::insert_admission_on(
        db,
        &admission,
        session.installation_id,
        OperationOrigin::Direct,
    )
    .await
}
