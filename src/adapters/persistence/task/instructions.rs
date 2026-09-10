//! Atomic ask-agent and start-agent operations for selected internal notes.

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::queue::insert_task;
use crate::{
    adapters::persistence::thread::insert_message_on,
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        internal_note::{AgentInstructionNote, AskOwnerOutcome, AskOwnerToAct, StartAgentTask},
        message::{CanonicalMessageId, MessageDirection, MessageRole, ThreadEntryKind},
        task::{BackgroundTask, NewTask, TaskLeaseRef, TaskSource},
        transport::PrincipalId,
    },
    transport::{BoundedVec, InboundTaskPayload, InboundTaskPayloadV1, ReplyDelivery},
    use_cases::thread::{MessageAuthorWrite, MessageWrite},
};

fn fingerprint(value: serde_json::Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

async fn advisory_lock(
    tx: &mut Transaction<'_, Postgres>,
    scope: Uuid,
    key: Uuid,
) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 0))")
        .bind(scope)
        .bind(key)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

fn outcome_from_db(value: &str) -> AppResult<AskOwnerOutcome> {
    match value {
        "queued" => Ok(AskOwnerOutcome::Queued),
        "requeued" => Ok(AskOwnerOutcome::Requeued),
        "parked" => Ok(AskOwnerOutcome::Parked),
        other => Err(AppError::Internal(format!(
            "Unknown task instruction wake outcome: {other}"
        ))),
    }
}

async fn selected_active_notes(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    note_ids: &[Uuid],
) -> AppResult<()> {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
             FROM internal_notes AS note
            WHERE note.company_id = $1 AND note.channel_id = $2 AND note.thread_id = $3
              AND note.id = ANY($4)
              AND NOT EXISTS (
                  SELECT 1 FROM internal_notes AS successor
                   WHERE successor.supersedes_note_id = note.id
              )
              AND NOT EXISTS (
                  SELECT 1 FROM internal_note_tombstones AS tombstone
                   WHERE tombstone.note_id = note.id
              )"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(thread_id)
    .bind(note_ids)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if count == note_ids.len() as i64 {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "Every selected note must be active and belong to this readable thread.".into(),
        ))
    }
}

async fn actor_in_scope(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    actor: PrincipalId,
) -> AppResult<()> {
    let valid: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM threads AS thread
               JOIN principals AS principal ON principal.company_id = thread.company_id
                WHERE thread.company_id = $1 AND thread.channel_id = $2 AND thread.id = $3
                  AND principal.id = $4 AND principal.kind <> 'external'
           )"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(thread_id)
    .bind(actor.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if valid {
        Ok(())
    } else {
        Err(AppError::NotFound("Thread not found.".into()))
    }
}

#[derive(sqlx::FromRow)]
struct LockedTask {
    status: String,
    owner_principal_kind: Option<String>,
    ownership_version: i64,
    execution_generation: Option<Uuid>,
}

async fn lock_instruction_task(
    tx: &mut Transaction<'_, Postgres>,
    command: &AskOwnerToAct,
) -> AppResult<LockedTask> {
    sqlx::query_as::<_, LockedTask>(
        r#"SELECT status, owner_principal_kind, ownership_version, execution_generation
             FROM background_tasks
            WHERE company_id = $1 AND channel_id = $2 AND thread_id = $3 AND id = $4
            FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(command.task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Active task not found.".into()))
}

fn instruction_outcome(command: &AskOwnerToAct, task: &LockedTask) -> AppResult<AskOwnerOutcome> {
    if task.ownership_version != command.expected_ownership_version as i64 {
        return Err(AppError::Conflict(
            "Task ownership changed; refresh and try again.".into(),
        ));
    }
    if task.owner_principal_kind.as_deref() != Some("agent") {
        return Err(AppError::Conflict(
            "Claim or transfer this task to an agent before asking it to act.".into(),
        ));
    }
    match task.status.as_str() {
        "pending" => Ok(AskOwnerOutcome::Queued),
        "processing" => Ok(AskOwnerOutcome::Requeued),
        "pending_approval" | "waiting_for_third_party_reply" => Ok(AskOwnerOutcome::Parked),
        _ => Err(AppError::Conflict(
            "This task is no longer active; start a new agent task instead.".into(),
        )),
    }
}

async fn insert_instruction(
    tx: &mut Transaction<'_, Postgres>,
    command: &AskOwnerToAct,
    actor: PrincipalId,
    command_fingerprint: &str,
    ownership_version: i64,
    outcome: AskOwnerOutcome,
) -> AppResult<()> {
    let outcome_db = match outcome {
        AskOwnerOutcome::Queued => "queued",
        AskOwnerOutcome::Requeued => "requeued",
        AskOwnerOutcome::Parked => "parked",
    };
    let instruction_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_agent_instructions (
                id, company_id, channel_id, thread_id, task_id, command_id,
                command_fingerprint, requested_by_principal_id,
                requested_ownership_version, wake_outcome
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(instruction_id)
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(command.task_id)
    .bind(command.command_id)
    .bind(command_fingerprint)
    .bind(actor.as_uuid())
    .bind(ownership_version)
    .bind(outcome_db)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    insert_instruction_notes(tx, instruction_id, command.company_id, &command.note_ids).await
}

async fn requeue_processing_task(
    tx: &mut Transaction<'_, Postgres>,
    command: &AskOwnerToAct,
    actor: PrincipalId,
    task: &LockedTask,
) -> AppResult<()> {
    let task_update = sqlx::query(
        r#"UPDATE background_tasks
              SET status = 'pending', worker_id = NULL, execution_generation = NULL,
                  locked_at = NULL, lock_expires_at = NULL, run_at = CURRENT_TIMESTAMP,
                  transition_reason = 'agent_instruction', transition_actor_kind = 'human',
                  transition_actor_id = $5, transition_approval_id = NULL,
                  transition_outreach_id = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE company_id = $1 AND id = $2 AND status = 'processing'
              AND ownership_version = $3 AND execution_generation = $4"#,
    )
    .bind(command.company_id)
    .bind(command.task_id)
    .bind(task.ownership_version)
    .bind(task.execution_generation)
    .bind(actor.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if task_update.rows_affected() != 1 {
        return Err(AppError::Conflict(
            "The task execution changed before it could be requeued.".into(),
        ));
    }
    // The instruction trigger observed the task while it was still processing. Announce the newly
    // pending generation explicitly; PostgreSQL releases this only when the transaction commits.
    sqlx::query("SELECT pg_notify('task_ready', json_build_object('task_id', $1::uuid)::text)")
        .bind(command.task_id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    // Batch workers create an attempt row, while the narrow operator/test claim path may not. The
    // task generation above is the execution fence; close its audit row when one exists.
    sqlx::query(
        r#"UPDATE task_attempts
              SET status = 'failed', stop_reason = 'agent_instruction',
                  error = 'A collaborator added explicit internal-note context',
                  finished_at = CURRENT_TIMESTAMP
            WHERE task_id = $1 AND execution_generation = $2 AND status = 'processing'"#,
    )
    .bind(command.task_id)
    .bind(task.execution_generation)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

pub(crate) async fn ask_owner_to_act(
    pool: &PgPool,
    command: &AskOwnerToAct,
    actor: PrincipalId,
) -> AppResult<AskOwnerOutcome> {
    command.validate().map_err(AppError::BadRequest)?;
    let command_fingerprint = fingerprint(serde_json::json!({
        "company_id": command.company_id,
        "channel_id": command.channel_id,
        "thread_id": command.thread_id,
        "task_id": command.task_id,
        "expected_ownership_version": command.expected_ownership_version,
        "note_ids": command.note_ids,
        "actor": actor.as_uuid(),
    }));
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    advisory_lock(&mut tx, command.company_id, command.command_id).await?;
    if let Some((stored_fingerprint, outcome)) = sqlx::query_as::<_, (String, String)>(
        "SELECT command_fingerprint, wake_outcome FROM task_agent_instructions WHERE company_id = $1 AND command_id = $2",
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?
    {
        if stored_fingerprint != command_fingerprint {
            return Err(AppError::Conflict(
                "Ask-agent command id was already used with different parameters.".into(),
            ));
        }
        return outcome_from_db(&outcome);
    }

    actor_in_scope(
        &mut tx,
        command.company_id,
        command.channel_id,
        command.thread_id,
        actor,
    )
    .await?;
    selected_active_notes(
        &mut tx,
        command.company_id,
        command.channel_id,
        command.thread_id,
        &command.note_ids,
    )
    .await?;
    let task = lock_instruction_task(&mut tx, command).await?;
    let outcome = instruction_outcome(command, &task)?;
    insert_instruction(
        &mut tx,
        command,
        actor,
        &command_fingerprint,
        task.ownership_version,
        outcome,
    )
    .await?;
    if outcome == AskOwnerOutcome::Requeued {
        requeue_processing_task(&mut tx, command, actor, &task).await?;
    }
    tx.commit().await.map_err(AppError::from)?;
    Ok(outcome)
}

async fn insert_instruction_notes(
    tx: &mut Transaction<'_, Postgres>,
    instruction_id: Uuid,
    company_id: Uuid,
    note_ids: &[Uuid],
) -> AppResult<()> {
    for (position, note_id) in note_ids.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO task_agent_instruction_notes
                   (instruction_id, company_id, note_id, position)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(instruction_id)
        .bind(company_id)
        .bind(note_id)
        .bind(position as i32)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
}

async fn ensure_thread_has_no_active_task(
    tx: &mut Transaction<'_, Postgres>,
    command: &StartAgentTask,
) -> AppResult<()> {
    let active = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id FROM background_tasks
            WHERE company_id = $1 AND channel_id = $2 AND thread_id = $3
              AND status IN ('pending', 'processing', 'pending_approval', 'waiting_for_third_party_reply')
            ORDER BY created_at, id LIMIT 1
            FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if active.is_some() {
        Err(AppError::Conflict(
            "This thread already has active work; refresh and ask its owner instead.".into(),
        ))
    } else {
        Ok(())
    }
}

async fn create_selected_note_task(
    tx: &mut Transaction<'_, Postgres>,
    command: &StartAgentTask,
) -> AppResult<BackgroundTask> {
    let correlation_id = CorrelationId::new();
    let source = MessageWrite::internal(
        command.thread_id,
        MessageAuthorWrite::Platform,
        "Internal note request",
        "Use the selected private notes to continue this thread.",
        MessageDirection::Inbound,
        MessageRole::System,
        correlation_id,
    )
    .with_entry_kind(ThreadEntryKind::SystemEvent);
    let inserted = insert_message_on(tx, &source).await?;
    let payload = InboundTaskPayload::v1(InboundTaskPayloadV1 {
        company_id: command.company_id,
        channel_id: command.channel_id,
        thread_id: command.thread_id,
        source_message_id: inserted.canonical_id,
        correlation_id,
        hop_count: 0,
        trace_channels: BoundedVec::empty(),
        is_forwarded: false,
        reply_delivery: ReplyDelivery::InAppOnly,
    })
    .encode()?;
    insert_task(
        tx,
        NewTask::starting_new_chain(
            command.company_id,
            command.channel_id,
            Some(command.thread_id),
            "email_agent_dispatch",
            payload,
        )
        .caused_by_source(TaskSource::Message(inserted.canonical_id)),
    )
    .await
}

async fn record_started_task_instruction(
    tx: &mut Transaction<'_, Postgres>,
    command: &StartAgentTask,
    actor: PrincipalId,
    command_fingerprint: &str,
    task: &BackgroundTask,
) -> AppResult<()> {
    let instruction_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO task_agent_instructions (
                id, company_id, channel_id, thread_id, task_id, command_id,
                command_fingerprint, requested_by_principal_id,
                requested_ownership_version, wake_outcome
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'queued')"#,
    )
    .bind(instruction_id)
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(task.id)
    .bind(command.command_id)
    .bind(command_fingerprint)
    .bind(actor.as_uuid())
    .bind(task.ownership.version as i64)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    insert_instruction_notes(tx, instruction_id, command.company_id, &command.note_ids).await?;
    sqlx::query(
        r#"INSERT INTO start_agent_task_commands
               (company_id, command_id, command_fingerprint, task_id)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .bind(command_fingerprint)
    .bind(task.id)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

pub(crate) async fn start_agent_task(
    pool: &PgPool,
    command: &StartAgentTask,
    actor: PrincipalId,
) -> AppResult<BackgroundTask> {
    command.validate().map_err(AppError::BadRequest)?;
    let command_fingerprint = fingerprint(serde_json::json!({
        "company_id": command.company_id,
        "channel_id": command.channel_id,
        "thread_id": command.thread_id,
        "note_ids": command.note_ids,
        "actor": actor.as_uuid(),
    }));
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    advisory_lock(&mut tx, command.company_id, command.command_id).await?;
    advisory_lock(&mut tx, command.company_id, command.thread_id).await?;
    if let Some((stored_fingerprint, task_id)) = sqlx::query_as::<_, (String, Uuid)>(
        "SELECT command_fingerprint, task_id FROM start_agent_task_commands WHERE company_id = $1 AND command_id = $2",
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?
    {
        if stored_fingerprint != command_fingerprint {
            return Err(AppError::Conflict(
                "Start-agent command id was already used with different parameters.".into(),
            ));
        }
        tx.commit().await.map_err(AppError::from)?;
        return super::operations::get_task_by_id_on(pool, task_id)
            .await?
            .ok_or_else(|| AppError::Internal("Started agent task is missing.".into()));
    }
    actor_in_scope(
        &mut tx,
        command.company_id,
        command.channel_id,
        command.thread_id,
        actor,
    )
    .await?;
    selected_active_notes(
        &mut tx,
        command.company_id,
        command.channel_id,
        command.thread_id,
        &command.note_ids,
    )
    .await?;
    ensure_thread_has_no_active_task(&mut tx, command).await?;
    let task = create_selected_note_task(&mut tx, command).await?;
    record_started_task_instruction(&mut tx, command, actor, &command_fingerprint, &task).await?;
    tx.commit().await.map_err(AppError::from)?;
    Ok(task)
}

#[derive(sqlx::FromRow)]
struct InstructionNoteDb {
    note_id: Uuid,
    message_id: Uuid,
    author_display: String,
    created_at: DateTime<Utc>,
    supersedes_note_id: Option<Uuid>,
    body: String,
}

pub(crate) async fn claim_agent_instruction_notes(
    pool: &PgPool,
    company_id: Uuid,
    thread_id: Uuid,
    lease: TaskLeaseRef,
) -> AppResult<Vec<AgentInstructionNote>> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let fenced = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT id FROM background_tasks
            WHERE company_id = $1 AND thread_id = $2 AND id = $3
              AND status = 'processing' AND worker_id = $4
              AND execution_generation = $5 AND owner_principal_id = $6
              AND ownership_version = $7 AND lock_expires_at > CURRENT_TIMESTAMP
            FOR UPDATE"#,
    )
    .bind(company_id)
    .bind(thread_id)
    .bind(lease.task_id)
    .bind(lease.worker_id)
    .bind(lease.execution_generation)
    .bind(lease.claimed_owner.principal_id().map(PrincipalId::as_uuid))
    .bind(lease.ownership_version as i64)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    if fenced.is_none() {
        return Err(AppError::Conflict(
            "Task ownership changed before internal notes were claimed.".into(),
        ));
    }
    let rows = sqlx::query_as::<_, InstructionNoteDb>(
        r#"SELECT DISTINCT ON (note.id)
                  note.id AS note_id, note.message_id, author.display_label AS author_display,
                  note.created_at, note.supersedes_note_id, message.clean_text_body AS body
             FROM task_agent_instructions AS instruction
             JOIN task_agent_instruction_notes AS selected ON selected.instruction_id = instruction.id
             JOIN internal_notes AS note
               ON (note.company_id, note.id) = (selected.company_id, selected.note_id)
             JOIN messages AS message
               ON (message.company_id, message.id) = (note.company_id, note.message_id)
             JOIN principals AS author
               ON (author.company_id, author.id) = (note.company_id, note.author_principal_id)
            WHERE instruction.company_id = $1 AND instruction.thread_id = $2
              AND instruction.task_id = $3 AND instruction.consumed_execution_generation IS NULL
              AND NOT EXISTS (SELECT 1 FROM internal_notes AS successor WHERE successor.supersedes_note_id = note.id)
              AND NOT EXISTS (SELECT 1 FROM internal_note_tombstones AS tombstone WHERE tombstone.note_id = note.id)
            ORDER BY note.id, instruction.created_at, selected.position"#,
    )
    .bind(company_id)
    .bind(thread_id)
    .bind(lease.task_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE task_agent_instructions
              SET consumed_execution_generation = $4, consumed_at = CURRENT_TIMESTAMP
            WHERE company_id = $1 AND thread_id = $2 AND task_id = $3
              AND consumed_execution_generation IS NULL"#,
    )
    .bind(company_id)
    .bind(thread_id)
    .bind(lease.task_id)
    .bind(lease.execution_generation)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;
    tx.commit().await.map_err(AppError::from)?;
    Ok(rows
        .into_iter()
        .map(|row| AgentInstructionNote {
            note_id: row.note_id,
            message_id: CanonicalMessageId::new(row.message_id),
            author_display: row.author_display,
            created_at: row.created_at,
            supersedes_note_id: row.supersedes_note_id,
            body: row.body,
        })
        .collect())
}
