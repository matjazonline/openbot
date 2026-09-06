//! PostgreSQL implementation of immutable internal-note lifecycle operations.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::{message::insert_message_on, views};
use crate::{
    app_error::{AppError, AppResult},
    entities::{
        correlation::CorrelationId,
        internal_note::{
            AddInternalNote, InternalNoteProvenance, InternalNoteView, TombstoneInternalNote,
        },
        message::{MessageDirection, MessageRole},
        transport::PrincipalId,
    },
    use_cases::thread::{MessageAuthorWrite, MessageWrite},
};

#[derive(sqlx::FromRow)]
struct InternalNoteDb {
    id: Uuid,
    author_principal_id: Uuid,
    provenance: String,
    supersedes_note_id: Option<Uuid>,
    superseded_by_note_id: Option<Uuid>,
    tombstoned_at: Option<DateTime<Utc>>,
}

impl TryFrom<InternalNoteDb> for InternalNoteView {
    type Error = AppError;

    fn try_from(row: InternalNoteDb) -> AppResult<Self> {
        Ok(Self {
            id: row.id,
            author_principal_id: PrincipalId::new(row.author_principal_id),
            provenance: InternalNoteProvenance::from_str(&row.provenance)
                .map_err(AppError::Internal)?,
            supersedes_note_id: row.supersedes_note_id,
            superseded_by_note_id: row.superseded_by_note_id,
            tombstoned_at: row.tombstoned_at,
        })
    }
}

const NOTE_COLUMNS: &str = r#"note.id, note.author_principal_id, note.provenance,
    note.supersedes_note_id,
    successor.id AS superseded_by_note_id,
    tombstone.created_at AS tombstoned_at"#;

fn fingerprint(value: serde_json::Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

async fn lock_command(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    command_id: Uuid,
) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 0))")
        .bind(company_id)
        .bind(command_id)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    Ok(())
}

async fn load_note_by_command(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    command_id: Uuid,
) -> AppResult<Option<(InternalNoteView, String, Uuid, Uuid)>> {
    #[derive(sqlx::FromRow)]
    struct Existing {
        #[sqlx(flatten)]
        note: InternalNoteDb,
        command_fingerprint: String,
        message_id: Uuid,
        thread_id: Uuid,
    }
    let query = format!(
        r#"SELECT {NOTE_COLUMNS}, note.command_fingerprint, note.message_id, note.thread_id
             FROM internal_notes AS note
             LEFT JOIN internal_notes AS successor ON successor.supersedes_note_id = note.id
             LEFT JOIN internal_note_tombstones AS tombstone ON tombstone.note_id = note.id
            WHERE note.company_id = $1 AND note.command_id = $2"#
    );
    sqlx::query_as::<_, Existing>(&query)
        .bind(company_id)
        .bind(command_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(AppError::from)?
        .map(|row| {
            Ok((
                row.note.try_into()?,
                row.command_fingerprint,
                row.message_id,
                row.thread_id,
            ))
        })
        .transpose()
}

pub(super) async fn create_internal_note(
    pool: &PgPool,
    command: &AddInternalNote,
    actor: PrincipalId,
) -> AppResult<crate::entities::message_view::ThreadMessageView> {
    command.validate().map_err(AppError::BadRequest)?;
    let command_fingerprint = fingerprint(serde_json::json!({
        "company_id": command.company_id,
        "channel_id": command.channel_id,
        "thread_id": command.thread_id,
        "text": command.text.trim(),
        "supersedes_note_id": command.supersedes_note_id,
        "provenance": command.provenance.as_str(),
        "actor": actor.as_uuid(),
    }));
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    lock_command(&mut tx, command.company_id, command.command_id).await?;
    if let Some((_, existing_fingerprint, message_id, thread_id)) =
        load_note_by_command(&mut tx, command.company_id, command.command_id).await?
    {
        if existing_fingerprint != command_fingerprint {
            return Err(AppError::Conflict(
                "Internal-note command id was already used with different parameters.".into(),
            ));
        }
        tx.commit().await.map_err(AppError::from)?;
        return views::get_thread_message(
            pool,
            thread_id,
            crate::entities::message::CanonicalMessageId::new(message_id),
        )
        .await?
        .ok_or_else(|| AppError::Internal("Stored internal note message is missing.".into()));
    }

    let actor_role = note_actor_role(&mut tx, command, actor).await?;
    if let Some(supersedes) = command.supersedes_note_id {
        lock_active_note(&mut tx, command, supersedes).await?;
    }

    let write = MessageWrite::internal(
        command.thread_id,
        MessageAuthorWrite::Principal(actor),
        "Internal note",
        command.text.trim(),
        MessageDirection::Inbound,
        actor_role,
        CorrelationId::new(),
    );
    let inserted = insert_message_on(&mut tx, &write).await?;
    let note_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO internal_notes (
                id, company_id, channel_id, thread_id, message_id, command_id,
                command_fingerprint, author_principal_id, provenance, supersedes_note_id
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(note_id)
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(inserted.canonical_id.as_uuid())
    .bind(command.command_id)
    .bind(command_fingerprint)
    .bind(actor.as_uuid())
    .bind(command.provenance.as_str())
    .bind(command.supersedes_note_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => AppError::Conflict(
            "That internal note was already corrected; refresh before correcting it again.".into(),
        ),
        _ => AppError::from(error),
    })?;
    tx.commit().await.map_err(AppError::from)?;
    views::get_thread_message(pool, command.thread_id, inserted.canonical_id)
        .await?
        .ok_or_else(|| AppError::Internal("New internal note message is missing.".into()))
}

async fn note_actor_role(
    tx: &mut Transaction<'_, Postgres>,
    command: &AddInternalNote,
    actor: PrincipalId,
) -> AppResult<MessageRole> {
    let kind = sqlx::query_scalar::<_, String>(
        r#"SELECT principal.kind
             FROM threads AS thread
             JOIN principals AS principal ON principal.company_id = thread.company_id
            WHERE thread.company_id = $1 AND thread.channel_id = $2 AND thread.id = $3
              AND principal.id = $4 AND principal.kind <> 'external'"#,
    )
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(actor.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?
    .ok_or_else(|| AppError::NotFound("Thread not found.".into()))?;
    match kind.as_str() {
        "person" => Ok(MessageRole::Human),
        "agent" => Ok(MessageRole::Agent),
        "system" => Ok(MessageRole::System),
        other => Err(AppError::Internal(format!(
            "Unsupported internal-note principal kind: {other}"
        ))),
    }
}

async fn lock_active_note(
    tx: &mut Transaction<'_, Postgres>,
    command: &AddInternalNote,
    note_id: Uuid,
) -> AppResult<()> {
    let active = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT note.id FROM internal_notes AS note
            WHERE note.company_id = $1 AND note.channel_id = $2 AND note.thread_id = $3
              AND note.id = $4
              AND NOT EXISTS (SELECT 1 FROM internal_notes AS next WHERE next.supersedes_note_id = note.id)
              AND NOT EXISTS (SELECT 1 FROM internal_note_tombstones AS gone WHERE gone.note_id = note.id)
            FOR UPDATE OF note"#,
    )
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(note_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if active.is_some() {
        Ok(())
    } else {
        Err(AppError::Conflict(
            "Only an active note in this thread can be corrected.".into(),
        ))
    }
}

pub(super) async fn tombstone_internal_note(
    pool: &PgPool,
    command: &TombstoneInternalNote,
    actor: PrincipalId,
) -> AppResult<InternalNoteView> {
    let command_fingerprint = fingerprint(serde_json::json!({
        "company_id": command.company_id,
        "channel_id": command.channel_id,
        "thread_id": command.thread_id,
        "note_id": command.note_id,
        "actor": actor.as_uuid(),
    }));
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    lock_command(&mut tx, command.company_id, command.command_id).await?;
    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT note_id, command_fingerprint FROM internal_note_tombstones WHERE company_id = $1 AND command_id = $2",
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    if let Some((note_id, fingerprint)) = existing {
        if fingerprint != command_fingerprint || note_id != command.note_id {
            return Err(AppError::Conflict(
                "Tombstone command id was already used with different parameters.".into(),
            ));
        }
        tx.commit().await.map_err(AppError::from)?;
        return load_note(pool, command.company_id, command.note_id).await;
    }

    let note_exists = sqlx::query_scalar::<_, Uuid>(
        r#"SELECT note.id FROM internal_notes AS note
             JOIN principals AS actor ON actor.company_id = note.company_id AND actor.id = $5
            WHERE note.company_id = $1 AND note.channel_id = $2 AND note.thread_id = $3
              AND note.id = $4 AND actor.kind <> 'external'
              AND NOT EXISTS (SELECT 1 FROM internal_notes AS next WHERE next.supersedes_note_id = note.id)
              AND NOT EXISTS (SELECT 1 FROM internal_note_tombstones AS gone WHERE gone.note_id = note.id)
            FOR UPDATE OF note"#,
    )
    .bind(command.company_id)
    .bind(command.channel_id)
    .bind(command.thread_id)
    .bind(command.note_id)
    .bind(actor.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::from)?;
    if note_exists.is_none() {
        return Err(AppError::Conflict(
            "Only an active note in this thread can be removed.".into(),
        ));
    }
    sqlx::query(
        r#"INSERT INTO internal_note_tombstones (
                note_id, company_id, command_id, command_fingerprint, actor_principal_id
           ) VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(command.note_id)
    .bind(command.company_id)
    .bind(command.command_id)
    .bind(command_fingerprint)
    .bind(actor.as_uuid())
    .execute(&mut *tx)
    .await
    .map_err(|error| match &error {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict("That internal note has already been removed.".into())
        }
        _ => AppError::from(error),
    })?;
    tx.commit().await.map_err(AppError::from)?;
    load_note(pool, command.company_id, command.note_id).await
}

async fn load_note(pool: &PgPool, company_id: Uuid, note_id: Uuid) -> AppResult<InternalNoteView> {
    let query = format!(
        r#"SELECT {NOTE_COLUMNS}
             FROM internal_notes AS note
             LEFT JOIN internal_notes AS successor ON successor.supersedes_note_id = note.id
             LEFT JOIN internal_note_tombstones AS tombstone ON tombstone.note_id = note.id
            WHERE note.company_id = $1 AND note.id = $2"#
    );
    sqlx::query_as::<_, InternalNoteDb>(&query)
        .bind(company_id)
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::NotFound("Internal note not found.".into()))?
        .try_into()
}
