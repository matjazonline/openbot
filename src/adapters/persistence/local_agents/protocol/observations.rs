use super::*;

struct Mutation {
    dispatch: DispatchEvidence,
    outcome: Option<OperationOutcome>,
    output: Option<LocalOutput>,
    truncated: bool,
    state: Option<TurnState>,
    usage: Option<MeasuredUsage>,
    definitive_refusal: bool,
}

impl Mutation {
    fn new(operation: &LocalAgentOperation) -> Self {
        Self {
            dispatch: operation.dispatch().clone(),
            outcome: operation.outcome(),
            output: None,
            truncated: false,
            state: None,
            usage: None,
            definitive_refusal: false,
        }
    }
}

pub(super) async fn accept(
    p: &PostgresPersistence,
    fence: ConnectionFence,
    observation: &JournalObservation,
) -> AppResult<u64> {
    let digest = observation.canonical_digest()?;
    let sequence = observation.sequence().get();
    let operation = observation.operation_id();
    let mut tx = begin(&p.pool).await?;
    let machine = lock_fence(&mut tx, fence).await?;
    if machine.journal_generation != fence.generation.get() as i64 {
        return Err(AppError::InvalidCredentials);
    }
    let acknowledged = machine.acknowledged_sequence as u64;
    if sequence <= acknowledged {
        let old:Option<String>=sqlx::query_scalar("SELECT digest FROM local_agent_observations WHERE company_id=$1 AND installation_id=$2 AND sequence=$3")
            .bind(fence.machine.company_id).bind(fence.machine.installation_id.as_uuid()).bind(sequence as i64).fetch_optional(&mut *tx).await?;
        if old.as_deref() != Some(digest.as_str()) {
            return Err(AppError::Conflict(
                "journal sequence content changed".into(),
            ));
        }
        reconcile_generation(&mut tx, fence, operation).await?;
        commit_cursor(&mut tx, fence, acknowledged).await?;
        tx.commit().await?;
        return Ok(acknowledged);
    }
    if sequence != acknowledged + 1 {
        return Err(AppError::Conflict("journal sequence gap".into()));
    }
    let row=sqlx::query_as::<_,OperationDb>(&format!("SELECT {} FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND installation_id=$3 AND id=$4 FOR UPDATE", operation_columns()))
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid()).bind(operation.as_uuid())
        .fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    if row.delivery_attempted_at.is_none() {
        return Err(AppError::BadRequest(
            "observation precedes possible delivery".into(),
        ));
    }
    let original = row.operation()?;
    let mut mutation = apply(&original, observation)?;
    validate_terminal(&mut tx, &original, &mut mutation).await?;
    persist_evidence(&mut tx, fence, &row, observation, &mutation).await?;
    sqlx::query("INSERT INTO local_agent_observations(company_id,machine_id,installation_id,sequence,operation_id,digest,observation) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
        .bind(sequence as i64).bind(operation.as_uuid()).bind(digest.as_str()).bind(encode(observation)?).execute(&mut *tx).await?;
    reconcile_generation(&mut tx, fence, operation).await?;
    commit_cursor(&mut tx, fence, sequence).await?;
    tx.commit().await?;
    Ok(sequence)
}

async fn reconcile_generation(
    db: &mut PgConnection,
    fence: ConnectionFence,
    operation: OperationId,
) -> AppResult<()> {
    sqlx::query("UPDATE local_agent_operations SET last_observed_generation=$4 WHERE company_id=$1 AND machine_id=$2 AND id=$3")
        .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(operation.as_uuid()).bind(fence.generation.get() as i64).execute(db).await?;
    Ok(())
}

fn apply(operation: &LocalAgentOperation, observation: &JournalObservation) -> AppResult<Mutation> {
    let mut mutation = Mutation::new(operation);
    match observation {
        JournalObservation::Agent { observation } => {
            apply_agent(operation, observation, &mut mutation)?
        }
        JournalObservation::Catalog { sequence, .. } => {
            if matches!(mutation.dispatch, DispatchEvidence::NotDispatched {}) {
                return Err(AppError::BadRequest(
                    "catalog observation requires a receipt".into(),
                ));
            }
            if !matches!(operation.command(), LocalCommand::RefreshCatalog {}) {
                return Err(missing());
            }
            mutation.dispatch = DispatchEvidence::CatalogSnapshot {
                sequence: *sequence,
            };
            mutation.outcome = Some(OperationOutcome::Succeeded);
        }
        JournalObservation::Control {
            sequence,
            thread_id,
            evidence,
            ..
        } => {
            if matches!(mutation.dispatch, DispatchEvidence::NotDispatched {})
                || (!matches!(operation.command(), LocalCommand::Inspect { .. })
                    && !matches!(
                        mutation.dispatch,
                        DispatchEvidence::SubmissionIntent { .. }
                            | DispatchEvidence::NativeIdentity { .. }
                    ))
            {
                return Err(AppError::BadRequest(
                    "control observation requires local dispatch evidence".into(),
                ));
            }
            if target_thread(operation.command()) != Some(thread_id) {
                return Err(missing());
            }
            match (operation.command(), evidence) {
                (LocalCommand::Inspect { .. }, ControlEvidence::Snapshot { state }) => {
                    mutation.state = Some(*state)
                }
                (LocalCommand::StopThread { .. }, ControlEvidence::StopAccepted {})
                | (LocalCommand::ArchiveThread { .. }, ControlEvidence::ArchiveAccepted {}) => {}
                _ => {
                    return Err(AppError::BadRequest(
                        "control evidence does not match command".into(),
                    ));
                }
            }
            mutation.dispatch = DispatchEvidence::NativeIdentity {
                sequence: *sequence,
                thread_id: thread_id.clone(),
                turn_id: None,
            };
            mutation.outcome = Some(OperationOutcome::Succeeded);
        }
    }
    let mut frozen = encode(operation)?;
    frozen["dispatch"] = encode(&mutation.dispatch)?;
    frozen["outcome"] = encode(&mutation.outcome)?;
    let _: LocalAgentOperation = decode(frozen, "observation transition")?;
    Ok(mutation)
}

fn apply_agent(
    operation: &LocalAgentOperation,
    observation: &LocalAgentObservation,
    mutation: &mut Mutation,
) -> AppResult<()> {
    match &observation.evidence {
        LocalEvidence::Receipt {} => {
            if matches!(mutation.dispatch, DispatchEvidence::NotDispatched {}) {
                mutation.dispatch = DispatchEvidence::AcceptedLocally {
                    sequence: observation.sequence,
                };
            }
        }
        LocalEvidence::SubmissionIntent {} => {
            if matches!(mutation.dispatch, DispatchEvidence::NotDispatched {}) {
                return Err(AppError::BadRequest(
                    "submission intent requires receipt".into(),
                ));
            }
            if matches!(mutation.dispatch, DispatchEvidence::AcceptedLocally { .. }) {
                mutation.dispatch = DispatchEvidence::SubmissionIntent {
                    sequence: observation.sequence,
                };
            }
        }
        LocalEvidence::Submission { thread_id, state } => {
            apply_submission(operation, observation.sequence, thread_id, *state, mutation)?;
        }
        LocalEvidence::Refused { reason } => {
            if !matches!(
                mutation.dispatch,
                DispatchEvidence::NotDispatched {} | DispatchEvidence::AcceptedLocally { .. }
            ) {
                return Err(AppError::BadRequest(
                    "possible SDK submission cannot become pre-dispatch refusal".into(),
                ));
            }
            mutation.definitive_refusal = true;
            mutation.outcome = Some(if *reason == LocalRefusal::Expired {
                OperationOutcome::Expired
            } else {
                OperationOutcome::Failed
            });
        }
        LocalEvidence::Uncertain {} => mutation.outcome = Some(OperationOutcome::Uncertain),
        LocalEvidence::Turn { .. } => apply_turn(operation, observation, mutation)?,
    }
    Ok(())
}

fn apply_submission(
    operation: &LocalAgentOperation,
    sequence: EventSequence,
    thread_id: &CloudroomThreadId,
    state: SubmissionState,
    mutation: &mut Mutation,
) -> AppResult<()> {
    if !operation.command().is_effect()
        || target_thread(operation.command()).is_some_and(|expected| expected != thread_id)
    {
        return Err(missing());
    }
    match &mutation.dispatch {
        DispatchEvidence::SubmissionIntent { .. } => {}
        DispatchEvidence::NativeIdentity {
            thread_id: known,
            turn_id: None,
            ..
        } if known == thread_id => {}
        _ => {
            return Err(AppError::BadRequest(
                "submission requires intent and cannot replace native turn evidence".into(),
            ));
        }
    }
    mutation.dispatch = DispatchEvidence::NativeIdentity {
        sequence,
        thread_id: thread_id.clone(),
        turn_id: None,
    };
    mutation.state = match state {
        SubmissionState::Accepted => None,
        SubmissionState::Queued => Some(TurnState::Queued),
        SubmissionState::Missing => {
            mutation.outcome = Some(OperationOutcome::Uncertain);
            Some(TurnState::Missing)
        }
    };
    Ok(())
}

fn apply_turn(
    operation: &LocalAgentOperation,
    observation: &LocalAgentObservation,
    mutation: &mut Mutation,
) -> AppResult<()> {
    let LocalEvidence::Turn {
        thread_id,
        turn_id,
        state,
        output,
        truncated,
        usage,
    } = &observation.evidence
    else {
        return Err(AppError::BadRequest("turn evidence required".into()));
    };

    if !operation.command().is_effect()
        && !matches!(operation.command(), LocalCommand::Inspect { .. })
    {
        return Err(missing());
    }
    if target_thread(operation.command()).is_some_and(|expected| expected != thread_id) {
        return Err(missing());
    }
    if let DispatchEvidence::NativeIdentity {
        thread_id: old_thread,
        turn_id: old_turn,
        ..
    } = &mutation.dispatch
        && (old_thread != thread_id || old_turn.as_ref().is_some_and(|old| old != turn_id))
    {
        return Err(AppError::Conflict("native turn identity changed".into()));
    }
    if !matches!(
        mutation.dispatch,
        DispatchEvidence::SubmissionIntent { .. } | DispatchEvidence::NativeIdentity { .. }
    ) {
        return Err(AppError::BadRequest(
            "native turn requires a submission intent".into(),
        ));
    }
    mutation.dispatch = DispatchEvidence::NativeIdentity {
        sequence: observation.sequence,
        thread_id: thread_id.clone(),
        turn_id: Some(turn_id.clone()),
    };
    mutation.state = Some(*state);
    mutation.outcome = match state {
        TurnState::Completed => Some(OperationOutcome::Succeeded),
        TurnState::Failed => Some(OperationOutcome::Failed),
        TurnState::Stopped => Some(OperationOutcome::Cancelled),
        TurnState::Missing => Some(OperationOutcome::Uncertain),
        _ => mutation.outcome,
    };
    if output.is_some()
        && !matches!(
            state,
            TurnState::Completed | TurnState::Failed | TurnState::Stopped
        )
    {
        return Err(AppError::BadRequest(
            "output requires a terminal turn".into(),
        ));
    }
    if usage
        .as_ref()
        .is_some_and(|u| u.input_tokens > MAX_SAFE_INTEGER || u.output_tokens > MAX_SAFE_INTEGER)
    {
        return Err(AppError::BadRequest(
            "usage exceeds protocol precision".into(),
        ));
    }
    mutation.output = output.clone();
    mutation.truncated = *truncated;
    mutation.usage = usage.clone();
    Ok(())
}

async fn validate_terminal(
    db: &mut PgConnection,
    operation: &LocalAgentOperation,
    mutation: &mut Mutation,
) -> AppResult<()> {
    if operation
        .outcome()
        .is_some_and(|outcome| outcome != OperationOutcome::Uncertain)
    {
        mutation.outcome = operation.outcome();
        mutation.output = None;
        mutation.state = None;
        mutation.usage = None;
    } else {
        let expired: bool = sqlx::query_scalar("SELECT $1::timestamptz<=clock_timestamp()")
            .bind(operation.deadline())
            .fetch_one(db)
            .await?;
        if expired && !mutation.definitive_refusal {
            mutation.outcome = Some(OperationOutcome::Uncertain);
        }
    }
    Ok(())
}

async fn persist_evidence(
    db: &mut PgConnection,
    fence: ConnectionFence,
    row: &OperationDb,
    observation: &JournalObservation,
    mutation: &Mutation,
) -> AppResult<()> {
    if let DispatchEvidence::NativeIdentity { thread_id, .. } = &mutation.dispatch
        && let Some(session) = row.session_id
    {
        let updated=sqlx::query("UPDATE local_agent_sessions SET thread_id=$5 WHERE company_id=$1 AND machine_id=$2 AND installation_id=$3 AND id=$4 AND (thread_id IS NULL OR thread_id=$5)")
                .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid()).bind(session).bind(thread_id.as_str()).execute(&mut *db).await?;
        if updated.rows_affected() != 1 {
            return Err(AppError::Conflict("session thread identity changed".into()));
        }
    }
    if let JournalObservation::Catalog {
        sequence, catalog, ..
    } = observation
    {
        sqlx::query("INSERT INTO local_machine_capabilities(company_id,machine_id,installation_id,sequence,catalog) VALUES($1,$2,$3,$4,$5)")
            .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
            .bind(sequence.get() as i64).bind(encode(catalog)?).execute(&mut *db).await?;
        sqlx::query(
            "UPDATE local_machines SET capabilities_sequence=$3 WHERE company_id=$1 AND id=$2",
        )
        .bind(fence.machine.company_id)
        .bind(fence.machine.machine_id.as_uuid())
        .bind(sequence.get() as i64)
        .execute(&mut *db)
        .await?;
    }
    sqlx::query("UPDATE local_agent_operations SET dispatch=$3,outcome=$4,output=COALESCE($5,output),output_truncated=CASE WHEN $5::text IS NULL THEN output_truncated ELSE $6 END, \
        observed_turn_state=COALESCE($7,observed_turn_state),measured_usage=COALESCE($8,measured_usage) WHERE company_id=$1 AND id=$2")
        .bind(fence.machine.company_id).bind(row.id).bind(encode(&mutation.dispatch)?).bind(mutation.outcome.map(|o|encode(&o)).transpose()?.and_then(|v|v.as_str().map(str::to_owned)))
        .bind(mutation.output.as_ref().map(LocalOutput::as_str)).bind(mutation.truncated)
        .bind(mutation.state.map(|s|encode(&s)).transpose()?.and_then(|v|v.as_str().map(str::to_owned)))
        .bind(mutation.usage.as_ref().map(encode).transpose()?).execute(db).await?;
    Ok(())
}
