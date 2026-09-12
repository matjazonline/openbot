//! Reply-handling policy storage: a company default and a channel override that may be cleared.
//!
//! Not to be confused with `manual_handoffs`, whose storage lives in `attention.rs`.

use std::{collections::HashMap, str::FromStr};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence,
        attention::{
            actor_is_manager, command_fingerprint, lock_attention_source, require_channel_principal,
            require_human_principal,
        },
    },
    app_error::{AppError, AppResult},
    application::thread_handoff::ThreadHandoffPolicyPersistence,
    entities::{
        attention::BusinessPriority,
        message::CanonicalMessageId,
        thread_handoff::{
            ExternalReplyHandling, ExternalReplyHandlingPolicy, ThreadHandoff, ThreadHandoffCommand,
            ThreadHandoffOperation, ThreadHandoffState, thread_handoff_operation_name,
        },
        transport::PrincipalId,
    },
};

/// One held message, as the inbound commit states it.
///
/// A named struct rather than six positional `Uuid`s, which is the argument-swap bug
/// `src/AGENTS.md` names: every one of these is a `uuid` and a transposed pair would compile.
pub(crate) struct OpenHandoffGeneration {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub thread_id: Uuid,
    /// Used only when this thread has no handoff yet; an existing row keeps its own id.
    pub handoff_id: Uuid,
    pub generation: Uuid,
    pub source_message_id: CanonicalMessageId,
}

/// The semantic fields of an opening command, hashed so a replay with different parameters is a
/// conflict rather than a silent overwrite.
///
/// Authorization context is deliberately absent, as `AttentionSourceCommand` already establishes:
/// who was allowed to ask is not part of what was asked.
#[derive(Serialize)]
struct OpenedFingerprint {
    handoff_id: Uuid,
    generation: Uuid,
    source_message_id: Uuid,
}

/// Open a fresh generation on this thread's handoff, creating the row if it has none.
///
/// Two statements and no read. The unique key `(company_id, thread_id)` is what serialises two
/// commits racing on one thread -- they contend on the row rather than on an advisory lock, and
/// the loser sees the winner's version -- so this is an upsert rather than a select-then-branch,
/// which would have to invent a lock for the row that does not exist yet.
///
/// `responsible_principal_id`, `business_priority` and `business_due_at` are deliberately left
/// alone: a thread somebody claimed and then received a second reply on stays theirs. The
/// *generation* moves; the responsibility does not.
pub(crate) async fn open_handoff_generation_on(
    tx: &mut Transaction<'_, Postgres>,
    open: OpenHandoffGeneration,
) -> AppResult<Uuid> {
    let (handoff_id, generation, version, opened): (Uuid, Uuid, i64, bool) = sqlx::query_as(
        r#"INSERT INTO thread_handoffs (
                   id, company_id, channel_id, thread_id, generation, state, source_message_id
           ) VALUES ($1, $2, $3, $4, $5, 'needs_instruction', $6)
           ON CONFLICT (company_id, thread_id) DO UPDATE
              SET generation = EXCLUDED.generation,
                  state = 'needs_instruction',
                  source_message_id = EXCLUDED.source_message_id,
                  version = thread_handoffs.version + 1,
                  generation_opened_at = CURRENT_TIMESTAMP,
                  closed_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
           RETURNING id, generation, version, (version = 1) AS opened"#,
    )
    .bind(open.handoff_id)
    .bind(open.company_id)
    .bind(open.channel_id)
    .bind(open.thread_id)
    .bind(open.generation)
    .bind(open.source_message_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;

    let fingerprint = handoff_command_fingerprint(&OpenedFingerprint {
        handoff_id,
        generation,
        source_message_id: open.source_message_id.as_uuid(),
    })?;
    sqlx::query(
        r#"INSERT INTO thread_handoff_events (
                   company_id, handoff_id, generation, command_id, command_fingerprint, operation,
                   actor_kind, from_state, to_state, from_version, to_version
           ) VALUES ($1, $2, $3, $4, $5, $6, 'system', $7, 'needs_instruction', $8, $9)"#,
    )
    .bind(open.company_id)
    .bind(handoff_id)
    .bind(generation)
    // The canonical message id, not a fresh UUID: the unique key
    // `(company_id, handoff_id, command_id)` is what refuses a second event for the same arriving
    // message, so the whole write is idempotent even if a second call ever reaches this path.
    .bind(open.source_message_id.as_uuid())
    .bind(fingerprint)
    .bind(if opened { "opened" } else { "regenerated" })
    // An inbound message is not a person acting, and the actor check makes `'system'` with a
    // principal -- or a human with none -- unrepresentable.
    //
    // `from_state` is `NULL` because this write does no read: the state a replacement came from is
    // whatever the previous event on this handoff recorded as its `to_state`, and paying for a
    // locking read to restate it would buy nothing the trail does not already hold.
    .bind(Option::<&str>::None)
    .bind(version - 1)
    .bind(version)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    Ok(handoff_id)
}

/// The states a responsibility command may still act on.
///
/// Wider than the attention projection's `is_actionable` on purpose: `drafting` is out of the
/// queue but still on the thread, and a manager must be able to hand over work that is mid-run.
/// The two terminal states are gone for good and are refused as indistinguishably as a handoff in
/// another company.
const OPEN_STATES: &str = "('needs_instruction', 'drafting', 'draft_ready')";

/// Every column a [`ThreadHandoff`] is built from, in the order the reads below select them.
const HANDOFF_COLUMNS: &str = "id, company_id, channel_id, thread_id, generation, state, \
     source_message_id, responsible_principal_id, business_priority, business_due_at, version, \
     generation_opened_at, created_at, updated_at";

#[derive(sqlx::FromRow)]
struct ThreadHandoffRow {
    id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    generation: Uuid,
    state: String,
    source_message_id: Uuid,
    responsible_principal_id: Option<Uuid>,
    business_priority: String,
    business_due_at: Option<DateTime<Utc>>,
    version: i64,
    generation_opened_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ThreadHandoffRow> for ThreadHandoff {
    type Error = AppError;

    fn try_from(row: ThreadHandoffRow) -> AppResult<Self> {
        Ok(Self {
            id: row.id,
            company_id: row.company_id,
            channel_id: row.channel_id,
            thread_id: row.thread_id,
            generation: row.generation,
            // A stored state or priority the enum does not know decides whether a customer waits,
            // so it is an error rather than a default.
            state: ThreadHandoffState::from_str(&row.state).map_err(AppError::Internal)?,
            source_message_id: CanonicalMessageId::new(row.source_message_id),
            responsible_principal_id: row.responsible_principal_id.map(PrincipalId::new),
            priority: BusinessPriority::from_str(&row.business_priority)
                .map_err(AppError::Internal)?,
            due_at: row.business_due_at,
            version: u64::try_from(row.version)
                .map_err(|_| AppError::Internal("Invalid thread handoff version".into()))?,
            generation_opened_at: row.generation_opened_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// What the fences and the audit row need out of the locked handoff.
#[derive(sqlx::FromRow)]
struct LockedHandoff {
    state: String,
    generation: Uuid,
    responsible_principal_id: Option<Uuid>,
    business_priority: String,
    business_due_at: Option<DateTime<Utc>>,
    version: i64,
    channel_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct ExistingHandoffEvent {
    command_fingerprint: String,
    to_version: i64,
}

/// The recorded outcome of `command_id`, if this handoff has already accepted it.
///
/// `existing_event` (`attention.rs`) with one table changed: the two audit logs are deliberately
/// separate, because `thread_handoff_events` living outside `attention_source_events` is what
/// keeps the notification triggers from firing for any of this.
async fn existing_handoff_event(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    handoff_id: Uuid,
    command_id: Uuid,
    fingerprint: &str,
) -> AppResult<Option<u64>> {
    let existing = sqlx::query_as::<_, ExistingHandoffEvent>(
        r#"SELECT command_fingerprint, to_version FROM thread_handoff_events
           WHERE company_id = $1 AND handoff_id = $2 AND command_id = $3"#,
    )
    .bind(company_id)
    .bind(handoff_id)
    .bind(command_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    if existing.command_fingerprint != fingerprint {
        return Err(AppError::Conflict(
            "Thread handoff command id was already used with different parameters.".into(),
        ));
    }
    Ok(Some(u64::try_from(existing.to_version).map_err(|_| {
        AppError::Internal("Invalid audited thread handoff version".into())
    })?))
}

fn handoff_command_fingerprint<T: Serialize>(value: &T) -> AppResult<String> {
    let encoded = serde_json::to_vec(value).map_err(|error| {
        AppError::Internal(format!("Could not encode thread handoff command: {error}"))
    })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

/// Resolve the effective policy inside a transaction, without copying it into any durable payload.
///
/// The transaction-scoped twin of `effective_review_required_on`. Phase 1 only stores the policy,
/// so nothing in the inbound path calls this yet; it is written, prepared and tested here so the
/// statement is never introduced untested.
// Phase 2 is the first production caller. Until then only the tests below execute it, so a
// non-test build sees it as unused.
#[allow(dead_code)]
pub(crate) async fn effective_reply_handling_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<ExternalReplyHandling> {
    let policy: Option<String> = sqlx::query_scalar(
        r#"SELECT COALESCE(channel.external_reply_handling_override,
                           company.external_reply_handling)
           FROM channels AS channel
           JOIN companies AS company ON company.id = channel.company_id
           WHERE channel.company_id = $1 AND channel.id = $2"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    match policy {
        // A stored value the enum does not know decides whether a customer's message is answered,
        // so it is an error rather than a default.
        Some(value) => value.parse().map_err(AppError::Internal),
        None => Err(AppError::NotFound("Channel not found.".into())),
    }
}

#[async_trait]
impl ThreadHandoffPolicyPersistence for PostgresPersistence {
    async fn reply_handling_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ExternalReplyHandlingPolicy>> {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            r#"SELECT company.external_reply_handling,
                      channel.external_reply_handling_override
               FROM companies AS company
               JOIN channels AS channel ON channel.company_id = company.id
               WHERE company.id = $1 AND channel.id = $2"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|(company, channel)| {
            let company_default = company.parse().map_err(AppError::Internal)?;
            let channel_override = channel
                .map(|value| value.parse().map_err(AppError::Internal))
                .transpose()?;
            Ok(ExternalReplyHandlingPolicy::resolve(
                company_default,
                channel_override,
            ))
        })
        .transpose()
    }

    async fn set_company_reply_handling(
        &self,
        company_id: Uuid,
        policy: ExternalReplyHandling,
    ) -> AppResult<()> {
        let changed =
            sqlx::query("UPDATE companies SET external_reply_handling = $2 WHERE id = $1")
                .bind(company_id)
                .bind(policy.as_str())
                .execute(&self.pool)
                .await
                .map_err(AppError::from)?;
        if changed.rows_affected() != 1 {
            return Err(AppError::NotFound("Company not found.".into()));
        }
        Ok(())
    }

    async fn set_channel_reply_handling_override(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<ExternalReplyHandling>,
    ) -> AppResult<()> {
        // Bound directly rather than through `COALESCE($3, external_reply_handling_override)`:
        // `None` means "inherit the company default from now on", and a `COALESCE` here would make
        // an override impossible to clear. See `plan/manual_handoff/phase1.md` §1.5.
        let changed = sqlx::query(
            r#"UPDATE channels
               SET external_reply_handling_override = $3
               WHERE company_id = $1 AND id = $2"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(policy_override.map(|policy| policy.as_str()))
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?;
        if changed.rows_affected() != 1 {
            return Err(AppError::NotFound("Channel not found.".into()));
        }
        Ok(())
    }

    async fn change_thread_handoff(&self, command: ThreadHandoffCommand) -> AppResult<u64> {
        command.validate().map_err(AppError::BadRequest)?;
        let fingerprint = command_fingerprint(&command)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        // The advisory key carries the kind, so a `thread_handoffs` row and a `manual_handoffs`
        // row that happened to share a UUID cannot serialise against each other.
        lock_attention_source(
            &mut tx,
            command.company_id,
            "thread_handoff",
            command.handoff_id,
        )
        .await?;
        if let Some(version) = existing_handoff_event(
            &mut tx,
            command.company_id,
            command.handoff_id,
            command.command_id,
            &fingerprint,
        )
        .await?
        {
            return Ok(version);
        }
        require_human_principal(&mut tx, command.company_id, command.actor_principal_id).await?;
        let manager =
            actor_is_manager(&mut tx, command.company_id, command.actor_principal_id).await?;

        // `visible_channel_ids` is in the predicate rather than checked afterwards, so a handoff
        // in another company, one on a channel the caller cannot view, and one that is already
        // terminal all answer with the same sentence. Never a message that distinguishes them.
        let locked = sqlx::query_as::<_, LockedHandoff>(&format!(
            r#"SELECT state, generation, responsible_principal_id, business_priority,
                      business_due_at, version, channel_id
               FROM thread_handoffs
               WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)
                 AND state IN {OPEN_STATES}
                 FOR UPDATE"#
        ))
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(&command.visible_channel_ids)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let locked = locked.ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;

        // Generation first: when both fences are stale it is the more useful answer, because it
        // says *why* the version moved.
        if locked.generation != command.expected_generation {
            return Err(AppError::Conflict(format!(
                "This thread received a newer reply; the current handoff generation is {}. \
                 Refresh and try again.",
                locked.generation
            )));
        }
        let old_version = u64::try_from(locked.version)
            .map_err(|_| AppError::Internal("Invalid thread handoff version".into()))?;
        if old_version != command.expected_version {
            return Err(AppError::Conflict(format!(
                "Thread handoff changed from version {} to {old_version}; refresh and try again.",
                command.expected_version
            )));
        }

        let actor_id = command.actor_principal_id.as_uuid();
        if !manager && locked.responsible_principal_id != Some(actor_id) {
            // The one exception to "only the responsible principal may act": an unclaimed handoff
            // is claimable by any teammate who can see the channel, which is what makes the
            // unassigned queue work at all.
            let self_claim = matches!(command.operation, ThreadHandoffOperation::Claim)
                && locked.responsible_principal_id.is_none();
            if !self_claim {
                // `NotFound`, never `Forbidden`: a teammate must not learn that a handoff they
                // cannot act on exists.
                return Err(AppError::NotFound("Thread handoff not found.".into()));
            }
        }
        if let ThreadHandoffOperation::Reassign { to } = command.operation {
            // A handoff cannot be assigned to somebody who could not open the thread.
            require_channel_principal(&mut tx, command.company_id, locked.channel_id, to).await?;
        }

        let new_responsible = match command.operation {
            ThreadHandoffOperation::Claim => Some(actor_id),
            ThreadHandoffOperation::Release => None,
            ThreadHandoffOperation::Reassign { to } => Some(to.as_uuid()),
            ThreadHandoffOperation::SetAttributes => locked.responsible_principal_id,
        };
        let new_version = locked
            .version
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("Thread handoff version exhausted.".into()))?;
        // `bump_handoff_version_for_responsibility_cleanup` only fires when the version did not
        // move, so this explicit bump wins and that trigger stays what it is for: the
        // `ON DELETE SET NULL` path.
        let written = sqlx::query(
            r#"UPDATE thread_handoffs
               SET responsible_principal_id = $3, business_priority = $4, business_due_at = $5,
                   version = $6, updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND id = $2 AND version = $7 AND generation = $8"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(new_responsible)
        .bind(command.priority.as_str())
        .bind(command.due_at)
        .bind(new_version)
        .bind(locked.version)
        .bind(locked.generation)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if written.rows_affected() != 1 {
            // Unreachable behind the `FOR UPDATE` above. Checked anyway: an unreachable fence that
            // fires is how a lost one gets discovered rather than tolerated.
            return Err(AppError::Conflict(
                "Thread handoff changed while it was being written; refresh and try again.".into(),
            ));
        }

        let operation = thread_handoff_operation_name(
            locked.responsible_principal_id.map(PrincipalId::new),
            new_responsible.map(PrincipalId::new),
            command.operation,
        );
        // `from_state` and `to_state` are both the row's state: a responsibility command never
        // changes it, and recording that explicitly is what makes a state change with no state
        // event detectable.
        sqlx::query(
            r#"INSERT INTO thread_handoff_events (
                   company_id, handoff_id, generation, command_id, command_fingerprint, operation,
                   actor_kind, actor_principal_id, from_state, to_state, from_version, to_version,
                   previous_priority, new_priority, previous_due_at, new_due_at,
                   previous_responsible_principal_id, new_responsible_principal_id
               ) VALUES ($1, $2, $3, $4, $5, $6, 'human', $7, $8, $8, $9, $10, $11, $12, $13, $14,
                         $15, $16)"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(locked.generation)
        .bind(command.command_id)
        .bind(fingerprint)
        .bind(operation)
        .bind(actor_id)
        .bind(&locked.state)
        .bind(locked.version)
        .bind(new_version)
        .bind(&locked.business_priority)
        .bind(command.priority.as_str())
        .bind(locked.business_due_at)
        .bind(command.due_at)
        .bind(locked.responsible_principal_id)
        .bind(new_responsible)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;
        u64::try_from(new_version)
            .map_err(|_| AppError::Internal("Invalid thread handoff version".into()))
    }

    async fn get_thread_handoff(
        &self,
        company_id: Uuid,
        handoff_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<Option<ThreadHandoff>> {
        // No state filter: a resolved handoff can still be read back by a route that has to
        // explain why a command was refused.
        sqlx::query_as::<_, ThreadHandoffRow>(&format!(
            r#"SELECT {HANDOFF_COLUMNS}
               FROM thread_handoffs
               WHERE company_id = $1 AND id = $2 AND channel_id = ANY($3)"#
        ))
        .bind(company_id)
        .bind(handoff_id)
        .bind(visible_channel_ids)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?
        .map(TryInto::try_into)
        .transpose()
    }

    async fn thread_handoffs_for_threads(
        &self,
        company_id: Uuid,
        thread_ids: &[Uuid],
        visible_channel_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, ThreadHandoff>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }
        // A map rather than a `Vec` the caller has to index: at most one handoff per thread is a
        // schema fact (`thread_handoffs_thread_key`), so the key is total and the caller cannot
        // mis-pair a row with a thread.
        sqlx::query_as::<_, ThreadHandoffRow>(&format!(
            r#"SELECT {HANDOFF_COLUMNS}
               FROM thread_handoffs
               WHERE company_id = $1 AND thread_id = ANY($2) AND channel_id = ANY($3)
                 AND state IN {OPEN_STATES}"#
        ))
        .bind(company_id)
        .bind(thread_ids)
        .bind(visible_channel_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .map(|row| {
            let thread_id = row.thread_id;
            ThreadHandoff::try_from(row).map(|handoff| (thread_id, handoff))
        })
        .collect()
    }
}

#[cfg(test)]
#[path = "thread_handoff_tests.rs"]
mod tests;
