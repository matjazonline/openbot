use super::*;
use crate::application::workflow::{RelatedAssociation, RelatedChannelId, RelatedThreadId};

pub(super) async fn authorize_session(
    db: &mut Transaction<'_, Postgres>,
    company: Uuid,
    machine: MachineId,
    actor: Uuid,
    operation: &LocalAgentOperation,
    origin: OperationOrigin,
) -> AppResult<()> {
    authorize_session_with_locks(
        db,
        company,
        machine,
        actor,
        operation,
        origin,
        crate::adapters::persistence::workflow::row_locks::RowLocks::Wait,
    )
    .await
}

pub(super) async fn authorize_session_with_locks(
    db: &mut Transaction<'_, Postgres>,
    company: Uuid,
    machine: MachineId,
    actor: Uuid,
    operation: &LocalAgentOperation,
    origin: OperationOrigin,
    locks: crate::adapters::persistence::workflow::row_locks::RowLocks,
) -> AppResult<()> {
    let command = operation.command();
    let Some(session) = command_session(command) else {
        return Ok(());
    };
    let stored = super::super::shared::session_on(db, CompanyId::new(company), session)
        .await?
        .ok_or_else(missing)?;
    if stored.selection.machine_id != machine {
        return Err(missing());
    }
    if let SessionAssociation::Workflow { run_id: run } = stored.association {
        super::super::super::workflow::local_agents::authorize_run_with_locks_on(
            db,
            CompanyId::new(company),
            run,
            WorkflowActor::authenticated(actor)?,
            locks,
        )
        .await?;
        if origin == OperationOrigin::Workflow {
            super::super::super::workflow::local_agents::authorize_operation_with_locks_on(
                db,
                CompanyId::new(company),
                operation.id(),
                WorkflowActor::authenticated(actor)?,
                locks,
            )
            .await?;
        }
    } else {
        if origin == OperationOrigin::Workflow {
            return Err(missing());
        }
        let SessionAssociation::Conversation {
            conversation_id: conversation,
        } = stored.association
        else {
            return Err(missing());
        };
        let channel: Uuid = sqlx::query_scalar(
            &locks.sql("SELECT channel_id FROM threads WHERE company_id=$1 AND id=$2 FOR SHARE"),
        )
        .bind(company)
        .bind(conversation)
        .fetch_optional(&mut **db)
        .await?
        .ok_or_else(missing)?;
        super::super::super::workflow::association::authorize_association_with_locks(
            db,
            CompanyId::new(company),
            WorkflowActor::authenticated(actor)?,
            RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(channel),
                thread_id: RelatedThreadId::new(conversation),
            },
            locks,
        )
        .await?;
    }
    let selection = &stored.selection;
    if command.is_effect() {
        super::super::native::selected_with_locks_on(db, company, actor, &stored.harness, locks)
            .await?;
    }
    if let LocalCommand::Create {
        selection: command_selection,
        harness,
        ..
    } = command
    {
        if command_selection != selection || harness != &stored.harness {
            return Err(AppError::Conflict("session creation inputs changed".into()));
        }
    } else if target_thread(command).map(CloudroomThreadId::as_str)
        != stored.thread_id.as_ref().map(CloudroomThreadId::as_str)
    {
        return Err(missing());
    }
    Ok(())
}

async fn validate_selection(
    db: &mut PgConnection,
    company: Uuid,
    machine: MachineId,
    command: &LocalCommand,
) -> AppResult<()> {
    let selection = match command {
        LocalCommand::Create {
            session_id,
            selection,
            ..
        } => {
            let unmapped: bool = sqlx::query_scalar("SELECT thread_id IS NULL FROM local_agent_sessions WHERE company_id=$1 AND id=$2 AND machine_id=$3")
                .bind(company).bind(session_id.as_uuid()).bind(machine.as_uuid()).fetch_optional(&mut *db).await?.ok_or_else(missing)?;
            if !unmapped {
                return Err(AppError::Conflict("session is already mapped".into()));
            }
            Some(selection.clone())
        }
        LocalCommand::Send { session_id, .. } => {
            let value:Value=sqlx::query_scalar("SELECT selection FROM local_agent_sessions WHERE company_id=$1 AND id=$2 AND machine_id=$3")
                .bind(company).bind(session_id.as_uuid()).bind(machine.as_uuid()).fetch_optional(&mut *db).await?.ok_or_else(missing)?;
            Some(decode(value, "session selection")?)
        }
        _ => None,
    };
    if let Some(selection) = selection {
        if selection.machine_id != machine {
            return Err(missing());
        }
        super::super::shared::validate_selection_on(
            db,
            CompanyId::new(company),
            machine,
            &selection,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn enqueue(
    p: &PostgresPersistence,
    admission: &OperationAdmission,
) -> AppResult<LocalAgentOperation> {
    let op = &admission.operation;
    if op.outcome().is_some() || !matches!(op.dispatch(), DispatchEvidence::NotDispatched {}) {
        return Err(AppError::BadRequest(
            "admission requires a new frozen operation".into(),
        ));
    }
    let mut tx = begin(&p.pool).await?;
    authorize(&mut tx, admission.company_id, admission.actor_id).await?;
    authorize_session(
        &mut tx,
        admission.company_id,
        admission.machine_id,
        admission.actor_id,
        op,
        OperationOrigin::Direct,
    )
    .await?;
    let machine = lock_machine(&mut tx, admission.company_id, admission.machine_id).await?;
    let existing=sqlx::query_as::<_,OperationDb>(&format!("SELECT {} FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND logical_key=$3", operation_columns()))
        .bind(admission.company_id).bind(admission.machine_id.as_uuid()).bind(admission.logical_key.as_str()).fetch_optional(&mut *tx).await?;
    if let Some(existing) = existing {
        let original = existing.operation()?;
        if original.payload_digest() != op.payload_digest() {
            return Err(AppError::Conflict(
                "logical invocation inputs changed".into(),
            ));
        }
        tx.commit().await?;
        return Ok(original);
    }
    if machine.state == "revoked"
        || (machine.state == "disabled" && !op.command().is_thread_control())
    {
        return Err(AppError::Conflict("machine is not enabled".into()));
    }
    validate_selection(
        &mut tx,
        admission.company_id,
        admission.machine_id,
        op.command(),
    )
    .await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    validate_deadline(now, op.deadline()).map_err(|e| AppError::BadRequest(e.into()))?;
    let counts = capacity(&mut tx, admission.company_id, admission.machine_id).await?;
    validate_capacity(
        counts.unsettled as usize,
        counts.queued as usize,
        op.command(),
    )
    .map_err(|e| AppError::Conflict(e.into()))?;
    insert_admission_on(
        &mut tx,
        admission,
        InstallationId::new(machine.installation_id)
            .map_err(|_| invalid("installation identity"))?,
        OperationOrigin::Direct,
    )
    .await?;
    tx.commit().await?;
    Ok(op.clone())
}

/// The caller holds authority and machine locks and has validated selection, deadline and capacity.
pub(in crate::adapters::persistence) async fn insert_admission_on(
    db: &mut PgConnection,
    admission: &OperationAdmission,
    installation: InstallationId,
    origin: OperationOrigin,
) -> AppResult<()> {
    let op = &admission.operation;
    sqlx::query("INSERT INTO local_agent_operations(company_id,id,machine_id,installation_id,session_id,logical_key,actor_id,command,payload_digest,deadline,origin) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(admission.company_id).bind(op.id().as_uuid()).bind(admission.machine_id.as_uuid()).bind(installation.as_uuid())
        .bind(command_session(op.command()).map(SessionId::as_uuid)).bind(admission.logical_key.as_str()).bind(admission.actor_id)
        .bind(encode(op.command())?).bind(op.payload_digest().as_str()).bind(op.deadline()).bind(origin.as_str()).execute(db).await.map_err(|error| {
            if error.as_database_error().is_some_and(|e|e.is_unique_violation()) {AppError::Conflict("operation identity was reused".into())} else {error.into()}
        })?;
    Ok(())
}

pub(super) async fn pending(
    p: &PostgresPersistence,
    fence: ConnectionFence,
) -> AppResult<Vec<CommandDelivery>> {
    // Read a bounded candidate set without taking locks out of company→machine order.
    let mut tx = begin(&p.pool).await?;
    let current = lock_fence(&mut tx, fence).await?;
    if current.journal_generation != fence.generation.get() as i64 {
        return Err(AppError::InvalidCredentials);
    }
    let candidates:Vec<Uuid>=sqlx::query_scalar(&format!("SELECT id FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND {OUTSTANDING_OPERATION_SQL} \
        AND (delivery_attempted_at IS NULL OR last_observed_generation<>$3) ORDER BY delivery_attempted_at NULLS LAST,created_at,id LIMIT 8"))
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.generation.get() as i64).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut deliveries = Vec::new();
    for id in candidates {
        if let Some(delivery) = prepare(p, fence, id).await? {
            deliveries.push(delivery);
        }
    }
    Ok(deliveries)
}

async fn prepare(
    p: &PostgresPersistence,
    fence: ConnectionFence,
    id: Uuid,
) -> AppResult<Option<CommandDelivery>> {
    let mut tx = begin(&p.pool).await?;
    let snapshot = sqlx::query_as::<_, OperationDb>(&format!(
        "SELECT {} FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND id=$3",
        operation_columns()
    ))
    .bind(fence.machine.company_id)
    .bind(fence.machine.machine_id.as_uuid())
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(missing)?;
    let operation = snapshot.operation()?;
    let authority = match snapshot.actor_id {
        Some(actor) => match authorize(&mut tx, fence.machine.company_id, actor).await {
            Ok(()) => {
                authorize_session(
                    &mut tx,
                    fence.machine.company_id,
                    fence.machine.machine_id,
                    actor,
                    &operation,
                    snapshot.origin()?,
                )
                .await
            }
            Err(error) => Err(error),
        },
        None if matches!(operation.command(), LocalCommand::RefreshCatalog {}) => Ok(()),
        None => Err(missing()),
    };
    let authority = match authority {
        Err(error)
            if !matches!(
                &error,
                AppError::NotFound(_) | AppError::Forbidden(_) | AppError::Conflict(_)
            ) =>
        {
            return Err(error);
        }
        other => other,
    };
    let row = lock_fence(&mut tx, fence).await?;
    if row.journal_generation != fence.generation.get() as i64 {
        return Err(AppError::InvalidCredentials);
    }
    let current = sqlx::query_as::<_, OperationDb>(&format!(
        "SELECT {} FROM local_agent_operations WHERE company_id=$1 AND id=$2 FOR UPDATE",
        operation_columns()
    ))
    .bind(fence.machine.company_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let result = prepare_locked(&mut tx, fence, &row, &current, authority).await?;
    commit_cursor(&mut tx, fence, row.acknowledged_sequence as u64).await?;
    tx.commit().await?;
    Ok(result)
}

async fn prepare_locked(
    db: &mut PgConnection,
    fence: ConnectionFence,
    machine: &FenceRow,
    row: &OperationDb,
    authority: AppResult<()>,
) -> AppResult<Option<CommandDelivery>> {
    if !row.outstanding {
        return Ok(None);
    }
    let operation = row.operation()?;
    if row.delivery_attempted_at.is_some() {
        // Withdrawal after a possible write never becomes proof of non-execution.
        return Ok(Some(CommandDelivery {
            operation,
            mode: DeliveryMode::Reconcile,
        }));
    }
    let permission = match authority {
        Ok(()) => {
            validate_selection(
                db,
                fence.machine.company_id,
                fence.machine.machine_id,
                operation.command(),
            )
            .await
        }
        Err(error) => Err(error),
    };
    match permission {
        Ok(()) => {}
        Err(AppError::NotFound(_) | AppError::Forbidden(_) | AppError::Conflict(_)) => {
            sqlx::query("UPDATE local_agent_operations SET outcome='cancelled' WHERE company_id=$1 AND id=$2 AND outcome IS NULL")
                .bind(fence.machine.company_id).bind(row.id).execute(db).await?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    if (machine.state != "enabled" && !operation.command().is_thread_control())
        || machine.reconciled_generation != fence.generation.get() as i64
    {
        return Ok(None);
    }
    let written=sqlx::query("UPDATE local_agent_operations SET delivery_attempted_at=clock_timestamp() WHERE company_id=$1 AND id=$2 AND outcome IS NULL AND delivery_attempted_at IS NULL AND deadline>clock_timestamp()")
        .bind(fence.machine.company_id).bind(row.id).execute(db).await?;
    Ok((written.rows_affected() == 1).then_some(CommandDelivery {
        operation,
        mode: DeliveryMode::Dispatch,
    }))
}
