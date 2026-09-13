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
            actor_is_manager, command_fingerprint, lock_attention_source,
            require_channel_principal, require_human_principal,
        },
    },
    app_error::{AppError, AppResult},
    application::thread_handoff::ThreadHandoffPolicyPersistence,
    entities::{
        attention::BusinessPriority,
        message::CanonicalMessageId,
        thread_handoff::{
            ExternalReplyHandling, ExternalReplyHandlingPolicy, HandoffRunRequest, ThreadHandoff,
            ThreadHandoffCommand, ThreadHandoffDismiss, ThreadHandoffDraft, ThreadHandoffOperation,
            ThreadHandoffState, thread_handoff_operation_name, truncate_failure_reason,
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

    // Any drafting run for the generation this reply just replaced is now answering a message that
    // is no longer the newest one. Marking it here, in the transaction that moves the generation,
    // is what makes the run's own outcome legible without joining the task: its completion is
    // refused by the generation predicate in `complete_handoff_run_on` either way.
    sqlx::query(
        r#"UPDATE thread_handoff_runs
           SET state = 'superseded', updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND handoff_id = $2 AND generation <> $3 AND state = 'running'"#,
    )
    .bind(open.company_id)
    .bind(handoff_id)
    .bind(generation)
    .execute(&mut **tx)
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

/// One row of the audit log, with every optional column stated rather than defaulted.
///
/// A struct because the insert below binds sixteen values of which eleven are `uuid` or `text`:
/// every transposed pair in a positional call would compile and then record a lie.
struct HandoffEventWrite<'a> {
    company_id: Uuid,
    handoff_id: Uuid,
    generation: Uuid,
    command_id: Uuid,
    command_fingerprint: &'a str,
    operation: &'a str,
    actor_kind: &'a str,
    actor_principal_id: Option<Uuid>,
    from_state: &'a str,
    to_state: &'a str,
    from_version: i64,
    to_version: i64,
    previous_priority: Option<&'a str>,
    new_priority: Option<&'a str>,
    previous_due_at: Option<DateTime<Utc>>,
    new_due_at: Option<DateTime<Utc>>,
    previous_responsible_principal_id: Option<Uuid>,
    new_responsible_principal_id: Option<Uuid>,
    task_id: Option<Uuid>,
    draft_id: Option<Uuid>,
    draft_version: Option<i32>,
    failure_reason: Option<&'a str>,
}

impl<'a> HandoffEventWrite<'a> {
    /// A state transition with no attribute or responsibility change, which is every event this
    /// phase adds.
    #[allow(clippy::too_many_arguments)]
    const fn transition(
        company_id: Uuid,
        handoff_id: Uuid,
        generation: Uuid,
        command_id: Uuid,
        command_fingerprint: &'a str,
        operation: &'a str,
        from_state: &'a str,
        to_state: &'a str,
        to_version: i64,
    ) -> Self {
        Self {
            company_id,
            handoff_id,
            generation,
            command_id,
            command_fingerprint,
            operation,
            actor_kind: "system",
            actor_principal_id: None,
            from_state,
            to_state,
            from_version: to_version - 1,
            to_version,
            previous_priority: None,
            new_priority: None,
            previous_due_at: None,
            new_due_at: None,
            previous_responsible_principal_id: None,
            new_responsible_principal_id: None,
            task_id: None,
            draft_id: None,
            draft_version: None,
            failure_reason: None,
        }
    }

    const fn by(mut self, actor_kind: &'a str, actor: PrincipalId) -> Self {
        self.actor_kind = actor_kind;
        self.actor_principal_id = Some(actor.as_uuid());
        self
    }
}

/// Append one immutable audit row. Every transition in this module goes through here, so a new
/// state change cannot quietly skip the trail.
async fn insert_handoff_event(
    tx: &mut Transaction<'_, Postgres>,
    event: HandoffEventWrite<'_>,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO thread_handoff_events (
               company_id, handoff_id, generation, command_id, command_fingerprint, operation,
               actor_kind, actor_principal_id, from_state, to_state, from_version, to_version,
               previous_priority, new_priority, previous_due_at, new_due_at,
               previous_responsible_principal_id, new_responsible_principal_id, task_id,
               draft_id, draft_version, failure_reason
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17,
                     $18, $19, $20, $21, $22)"#,
    )
    .bind(event.company_id)
    .bind(event.handoff_id)
    .bind(event.generation)
    .bind(event.command_id)
    .bind(event.command_fingerprint)
    .bind(event.operation)
    .bind(event.actor_kind)
    .bind(event.actor_principal_id)
    .bind(event.from_state)
    .bind(event.to_state)
    .bind(event.from_version)
    .bind(event.to_version)
    .bind(event.previous_priority)
    .bind(event.new_priority)
    .bind(event.previous_due_at)
    .bind(event.new_due_at)
    .bind(event.previous_responsible_principal_id)
    .bind(event.new_responsible_principal_id)
    .bind(event.task_id)
    .bind(event.draft_id)
    .bind(event.draft_version)
    .bind(event.failure_reason)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// The identity and both fences of one command, shared by every operation that takes them.
struct HandoffCommandScope<'a> {
    company_id: Uuid,
    handoff_id: Uuid,
    command_id: Uuid,
    expected_version: u64,
    expected_generation: Uuid,
    actor: PrincipalId,
    visible_channel_ids: &'a [Uuid],
}

/// What the shared preamble concluded: this command already ran, or the row is locked and fenced.
enum AcceptedCommand {
    Replayed(u64),
    Fenced {
        locked: LockedHandoff,
        manager: bool,
    },
}

/// Lock, replay, load, fence -- the half of every handoff command that does not depend on which
/// command it is.
///
/// One copy rather than one per operation: the fence is the whole safety argument of this feature,
/// and three transcriptions of it would be three places for it to drift.
async fn accept_handoff_command(
    tx: &mut Transaction<'_, Postgres>,
    scope: &HandoffCommandScope<'_>,
    fingerprint: &str,
) -> AppResult<AcceptedCommand> {
    // The advisory key carries the kind, so a `thread_handoffs` row and a `manual_handoffs` row
    // that happened to share a UUID cannot serialise against each other.
    lock_attention_source(tx, scope.company_id, "thread_handoff", scope.handoff_id).await?;
    if let Some(version) = existing_handoff_event(
        tx,
        scope.company_id,
        scope.handoff_id,
        scope.command_id,
        fingerprint,
    )
    .await?
    {
        return Ok(AcceptedCommand::Replayed(version));
    }
    require_human_principal(tx, scope.company_id, scope.actor).await?;
    let manager = actor_is_manager(tx, scope.company_id, scope.actor).await?;

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
    .bind(scope.company_id)
    .bind(scope.handoff_id)
    .bind(scope.visible_channel_ids)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let locked = locked.ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    fence_handoff(
        locked.generation,
        locked.version,
        scope.expected_generation,
        scope.expected_version,
    )?;
    Ok(AcceptedCommand::Fenced { locked, manager })
}

/// Both fences, in the order that gives the most useful answer when both are stale.
///
/// Generation first: it says *why* the version moved, which is the sentence the user needs.
fn fence_handoff(
    generation: Uuid,
    version: i64,
    expected_generation: Uuid,
    expected_version: u64,
) -> AppResult<()> {
    if generation != expected_generation {
        return Err(AppError::Conflict(format!(
            "This thread received a newer reply; the current handoff generation is {generation}. \
             Refresh and try again."
        )));
    }
    let current = u64::try_from(version)
        .map_err(|_| AppError::Internal("Invalid thread handoff version".into()))?;
    if current != expected_version {
        return Err(AppError::Conflict(format!(
            "Thread handoff changed from version {expected_version} to {current}; refresh and \
             try again."
        )));
    }
    Ok(())
}

/// Who may act on this row, as one rule over already-loaded values.
///
/// `NotFound`, never `Forbidden`: a teammate must not learn that a handoff they cannot act on
/// exists on a channel they cannot see. `unclaimed_is_open` is the self-claim exception -- an
/// unclaimed item is takeable by any teammate who can see the channel, which is what makes the
/// unassigned queue work at all.
fn authorize_handoff_actor(
    manager: bool,
    responsible: Option<Uuid>,
    actor: PrincipalId,
    unclaimed_is_open: bool,
) -> AppResult<()> {
    if manager
        || responsible == Some(actor.as_uuid())
        || (unclaimed_is_open && responsible.is_none())
    {
        return Ok(());
    }
    Err(AppError::NotFound("Thread handoff not found.".into()))
}

/// One **Generate draft**, as the instruction command states it.
///
/// A named struct rather than five positional `Uuid`s, for the reason [`OpenHandoffGeneration`]
/// gives: every one of these is a `uuid` and a transposed pair would compile.
pub(crate) struct StartHandoffRun {
    pub company_id: Uuid,
    pub thread_id: Uuid,
    pub task_id: Uuid,
    pub command_id: Uuid,
    pub actor: PrincipalId,
    pub request: HandoffRunRequest,
}

/// Fence the handoff, record the run, and move the handoff to `drafting` -- all inside the
/// instruction command's own transaction.
///
/// Sharing that transaction is the whole point: the task row, the run row and the handoff state
/// commit together or not at all, so the existing idempotency records (`task_agent_instructions`
/// and `start_agent_task_commands`) cover this write too and a replay cannot produce a second run.
///
/// The claim precondition is not ceremony. `create_review_draft_on` fixes the assigned reviewer at
/// draft time, and the review machinery authorizes only that reviewer, so pinning the reviewer to
/// the responsible principal here is exactly what lets **Send** work later with no change to the
/// review path at all.
pub(crate) async fn start_handoff_run_on(
    tx: &mut Transaction<'_, Postgres>,
    start: StartHandoffRun,
) -> AppResult<()> {
    let locked = sqlx::query_as::<_, LockedHandoff>(
        r#"SELECT state, generation, responsible_principal_id, business_priority,
                  business_due_at, version, channel_id
           FROM thread_handoffs
           WHERE company_id = $1 AND id = $2 AND thread_id = $3
             FOR UPDATE"#,
    )
    .bind(start.company_id)
    .bind(start.request.handoff_id)
    .bind(start.thread_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let locked = locked.ok_or_else(|| AppError::NotFound("Thread handoff not found.".into()))?;
    fence_handoff(
        locked.generation,
        locked.version,
        start.request.generation,
        start.request.expected_version,
    )?;
    if locked.state != ThreadHandoffState::NeedsInstruction.as_str() {
        return Err(AppError::Conflict(format!(
            "This reply is {}, not waiting for an instruction; refresh and try again.",
            locked.state
        )));
    }
    // An unclaimed handoff is refused rather than claimed implicitly: the reviewer of the draft
    // this run will write is the responsible principal, so "who owns the answer" must already have
    // an answer before the agent starts writing one.
    if locked.responsible_principal_id.is_none() {
        // Answered before the authorization check, and as a `Conflict` rather than a `NotFound`:
        // an unclaimed reply is already visible to everyone who can see the channel, so the useful
        // answer is the next step -- claim it -- not a pretence that it does not exist.
        return Err(AppError::Conflict(
            "Claim this reply before asking the agent to draft an answer.".into(),
        ));
    }
    let manager = actor_is_manager(tx, start.company_id, start.actor).await?;
    authorize_handoff_actor(manager, locked.responsible_principal_id, start.actor, false)?;

    let inserted = sqlx::query(
        r#"INSERT INTO thread_handoff_runs (
               company_id, task_id, handoff_id, generation, requested_by_principal_id, command_id
           ) VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(start.company_id)
    .bind(start.task_id)
    .bind(start.request.handoff_id)
    .bind(start.request.generation)
    .bind(start.actor.as_uuid())
    .bind(start.command_id)
    .execute(&mut **tx)
    .await;
    if let Err(error) = inserted {
        // Mapped on the constraint name rather than absorbed with `ON CONFLICT DO NOTHING`, which
        // would leave a task running for a handoff that never learned about it. The loser's whole
        // transaction -- task row included -- rolls back.
        if error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint)
            == Some("thread_handoff_runs_generation_key")
        {
            return Err(AppError::Conflict(
                "A draft is already being prepared for this reply.".into(),
            ));
        }
        return Err(AppError::from(error));
    }

    let new_version = locked
        .version
        .checked_add(1)
        .ok_or_else(|| AppError::Conflict("Thread handoff version exhausted.".into()))?;
    let written = sqlx::query(
        r#"UPDATE thread_handoffs
           SET state = 'drafting', version = $5, updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND version = $3 AND generation = $4
             AND state = 'needs_instruction'"#,
    )
    .bind(start.company_id)
    .bind(start.request.handoff_id)
    .bind(locked.version)
    .bind(locked.generation)
    .bind(new_version)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if written.rows_affected() != 1 {
        // Unreachable behind the `FOR UPDATE` above, and checked anyway: an unreachable fence that
        // fires is how a lost one gets discovered rather than tolerated.
        return Err(AppError::Conflict(
            "Thread handoff changed while the draft was being requested; refresh and try again."
                .into(),
        ));
    }
    let fingerprint = handoff_command_fingerprint(&start.command_id)?;
    insert_handoff_event(
        tx,
        HandoffEventWrite {
            task_id: Some(start.task_id),
            ..HandoffEventWrite::transition(
                start.company_id,
                start.request.handoff_id,
                locked.generation,
                start.command_id,
                &fingerprint,
                "draft_requested",
                ThreadHandoffState::NeedsInstruction.as_str(),
                ThreadHandoffState::Drafting.as_str(),
                new_version,
            )
            .by("human", start.actor)
        },
    )
    .await
}

/// The drafting run this task is executing, in whatever state it has reached.
///
/// Keyed by task because that is the only identifier a durable payload carries -- nothing about a
/// handoff goes into `InboundTaskPayloadV1`, precisely so a worker cannot hold a snapshotted
/// generation and fail to notice that a newer customer message replaced it. The responsible
/// principal comes back in the same read because the completion needs it as the draft's reviewer.
///
/// **Deliberately not filtered to `running`.** A run the inbound path marked `superseded`, or one
/// that already produced a draft, must still be *found* here: a task that was ever a drafting run
/// answers a customer message somebody was asked to instruct, so if its outcome can no longer be
/// stored it has to be refused. Filtering to `running` would instead route it to the publish
/// branch and send that answer as mail. `complete_handoff_run_on` is what refuses the ones that
/// are no longer current, and its `state = 'running'` predicate is the single place that decides.
pub(crate) async fn handoff_run_for_task_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    task_id: Uuid,
) -> AppResult<Option<RunningHandoff>> {
    sqlx::query_as::<_, RunningHandoff>(
        r#"SELECT run.handoff_id, run.generation, handoff.responsible_principal_id
           FROM thread_handoff_runs AS run
           JOIN thread_handoffs AS handoff
             ON (handoff.company_id, handoff.id) = (run.company_id, run.handoff_id)
           WHERE run.company_id = $1 AND run.task_id = $2"#,
    )
    .bind(company_id)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// A drafting run and the handoff it answers, as the dispatch commit needs them.
#[derive(Debug, Clone, Copy, sqlx::FromRow)]
pub(crate) struct RunningHandoff {
    pub handoff_id: Uuid,
    pub generation: Uuid,
    /// `NULL` when a manager released the handoff while the agent worked. The draft then falls
    /// back to `resolve_reviewer_on`, exactly as an ordinary review draft does.
    pub responsible_principal_id: Option<Uuid>,
}

/// The draft this run produced, in the transaction that produced it.
pub(crate) struct DraftedHandoffRun {
    pub company_id: Uuid,
    pub task_id: Uuid,
    pub run: RunningHandoff,
    pub agent: PrincipalId,
    pub draft_id: Uuid,
    pub draft_version: i32,
}

/// Mark the run `drafted` and the handoff `draft_ready`, or refuse the whole commit.
///
/// The generation predicate on the handoff update is the fence that matters: a zero-row update
/// means a newer customer reply replaced the generation while the agent was writing, so the reply
/// in hand answers a message that is no longer the latest one. That is a `Conflict` which rolls
/// the entire dispatch transaction back -- no draft, no delivery, no message -- and leaves the new
/// generation exactly where the inbound commit put it.
pub(crate) async fn complete_handoff_run_on(
    tx: &mut Transaction<'_, Postgres>,
    drafted: DraftedHandoffRun,
) -> AppResult<()> {
    let written = sqlx::query(
        r#"UPDATE thread_handoff_runs
           SET state = 'drafted', draft_id = $3, draft_version = $4,
               updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND task_id = $2 AND state = 'running'"#,
    )
    .bind(drafted.company_id)
    .bind(drafted.task_id)
    .bind(drafted.draft_id)
    .bind(drafted.draft_version)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if written.rows_affected() != 1 {
        // Superseded by a newer customer reply, or already drafted by an earlier execution of this
        // task. Either way the answer in hand cannot be stored, and the `Conflict` rolls the whole
        // dispatch back rather than letting it publish.
        return Err(AppError::Conflict(
            "This drafting run is no longer the one this reply is waiting for.".into(),
        ));
    }

    let version: Option<i64> = sqlx::query_scalar(
        r#"UPDATE thread_handoffs
           SET state = 'draft_ready', version = version + 1, updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND generation = $3 AND state = 'drafting'
           RETURNING version"#,
    )
    .bind(drafted.company_id)
    .bind(drafted.run.handoff_id)
    .bind(drafted.run.generation)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some(version) = version else {
        return Err(AppError::Conflict(
            "This thread received a newer reply while the draft was being written; the draft was \
             discarded."
                .into(),
        ));
    };
    let fingerprint = handoff_command_fingerprint(&drafted.draft_id)?;
    insert_handoff_event(
        tx,
        HandoffEventWrite {
            task_id: Some(drafted.task_id),
            draft_id: Some(drafted.draft_id),
            draft_version: Some(drafted.draft_version),
            ..HandoffEventWrite::transition(
                drafted.company_id,
                drafted.run.handoff_id,
                drafted.run.generation,
                // The draft is the command: one draft per run, one run per generation, so this is
                // unique under `thread_handoff_events_command_key` without inventing an id.
                drafted.draft_id,
                &fingerprint,
                "draft_ready",
                ThreadHandoffState::Drafting.as_str(),
                ThreadHandoffState::DraftReady.as_str(),
                version,
            )
            .by("agent", drafted.agent)
        },
    )
    .await
}

/// Why a drafting run is ending with no reply sent.
///
/// An enum rather than a state parameter because the two cases catch a run at different points of
/// its life, and matching the wrong pair of states would silently do nothing -- which is exactly
/// what a `failed` write that quietly matched no row looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HandoffRunEnd {
    /// The task died before the run produced a draft.
    TaskFailed,
    /// The run produced a draft, and the review holding it expired before anybody sent it.
    DraftExpired,
}

impl HandoffRunEnd {
    /// The run state this ending may act on.
    const fn run_state(self) -> &'static str {
        match self {
            Self::TaskFailed => "running",
            Self::DraftExpired => "drafted",
        }
    }

    /// The handoff state this ending may act on, which is also the event's `from_state`.
    const fn handoff_state(self) -> &'static str {
        match self {
            Self::TaskFailed => "drafting",
            Self::DraftExpired => "draft_ready",
        }
    }
}

/// End a drafting run without a sent reply and hand the reply back to the team.
///
/// `AND generation = $3` is the whole point. An ending belonging to a generation the thread has
/// moved past must not drag a newer `needs_instruction` handoff backwards, nor overwrite a
/// `draft_ready` one that a later run produced. When it matches nothing the run is still recorded
/// as `failed` and no event is written, because nothing about the handoff changed.
pub(crate) async fn fail_handoff_run_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    task_id: Uuid,
    end: HandoffRunEnd,
    reason: &str,
) -> AppResult<()> {
    let failed: Option<(Uuid, Uuid)> = sqlx::query_as(&format!(
        r#"UPDATE thread_handoff_runs
           SET state = 'failed', updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND task_id = $2 AND state = '{run_state}'
           RETURNING handoff_id, generation"#,
        run_state = end.run_state(),
    ))
    .bind(company_id)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some((handoff_id, generation)) = failed else {
        return Ok(());
    };

    let version: Option<i64> = sqlx::query_scalar(&format!(
        r#"UPDATE thread_handoffs
           SET state = 'needs_instruction', version = version + 1,
               updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND generation = $3
             AND state = '{handoff_state}'
           RETURNING version"#,
        handoff_state = end.handoff_state(),
    ))
    .bind(company_id)
    .bind(handoff_id)
    .bind(generation)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some(version) = version else {
        return Ok(());
    };
    let fingerprint = handoff_command_fingerprint(&task_id)?;
    insert_handoff_event(
        tx,
        HandoffEventWrite {
            task_id: Some(task_id),
            // Truncated here rather than at the `CHECK`: letting the constraint do it is not
            // truncation, it is a failed insert inside the transaction recording a failure.
            failure_reason: Some(truncate_failure_reason(reason)).filter(|text| !text.is_empty()),
            ..HandoffEventWrite::transition(
                company_id,
                handoff_id,
                generation,
                // The task is the command: one drafting run per task, so a second failure write
                // for the same task is refused by `thread_handoff_events_command_key`.
                task_id,
                &fingerprint,
                "draft_failed",
                end.handoff_state(),
                ThreadHandoffState::NeedsInstruction.as_str(),
                version,
            )
        },
    )
    .await
}

/// Resolve the handoff generation a published draft answered, inside the publication transaction.
///
/// Matches nothing -- and so changes nothing -- when the draft is not a handoff draft, because the
/// join finds no run: every pre-existing review approval is byte-for-byte unaffected. And it
/// matches only the generation the draft belongs to, so a draft written for a superseded
/// generation could not resolve the current one even if it could somehow be approved.
pub(crate) async fn resolve_handoff_for_draft_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    draft_id: Uuid,
    actor: PrincipalId,
    command_id: Uuid,
) -> AppResult<()> {
    let resolved: Option<(Uuid, Uuid, i64)> = sqlx::query_as(
        r#"UPDATE thread_handoffs AS handoff
           SET state = 'resolved', closed_at = CURRENT_TIMESTAMP, version = handoff.version + 1,
               updated_at = CURRENT_TIMESTAMP
           FROM thread_handoff_runs AS run
           WHERE run.company_id = handoff.company_id AND run.handoff_id = handoff.id
             AND run.generation = handoff.generation
             AND (run.company_id, run.draft_id) = ($1, $2)
             AND handoff.state = 'draft_ready'
           RETURNING handoff.id, handoff.generation, handoff.version"#,
    )
    .bind(company_id)
    .bind(draft_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some((handoff_id, generation, version)) = resolved else {
        return Ok(());
    };
    let fingerprint = handoff_command_fingerprint(&command_id)?;
    insert_handoff_event(
        tx,
        HandoffEventWrite {
            draft_id: Some(draft_id),
            ..HandoffEventWrite::transition(
                company_id,
                handoff_id,
                generation,
                command_id,
                &fingerprint,
                "resolved",
                ThreadHandoffState::DraftReady.as_str(),
                ThreadHandoffState::Resolved.as_str(),
                version,
            )
            .by("human", actor)
        },
    )
    .await
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
        let scope = HandoffCommandScope {
            company_id: command.company_id,
            handoff_id: command.handoff_id,
            command_id: command.command_id,
            expected_version: command.expected_version,
            expected_generation: command.expected_generation,
            actor: command.actor_principal_id,
            visible_channel_ids: &command.visible_channel_ids,
        };
        let (locked, manager) = match accept_handoff_command(&mut tx, &scope, &fingerprint).await? {
            AcceptedCommand::Replayed(version) => return Ok(version),
            AcceptedCommand::Fenced { locked, manager } => (locked, manager),
        };

        let actor_id = command.actor_principal_id.as_uuid();
        authorize_handoff_actor(
            manager,
            locked.responsible_principal_id,
            command.actor_principal_id,
            matches!(command.operation, ThreadHandoffOperation::Claim),
        )?;
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
        insert_handoff_event(
            &mut tx,
            HandoffEventWrite {
                previous_priority: Some(&locked.business_priority),
                new_priority: Some(command.priority.as_str()),
                previous_due_at: locked.business_due_at,
                new_due_at: command.due_at,
                previous_responsible_principal_id: locked.responsible_principal_id,
                new_responsible_principal_id: new_responsible,
                ..HandoffEventWrite::transition(
                    command.company_id,
                    command.handoff_id,
                    locked.generation,
                    command.command_id,
                    &fingerprint,
                    operation,
                    &locked.state,
                    &locked.state,
                    new_version,
                )
                .by("human", command.actor_principal_id)
            },
        )
        .await?;
        tx.commit().await.map_err(AppError::from)?;
        u64::try_from(new_version)
            .map_err(|_| AppError::Internal("Invalid thread handoff version".into()))
    }

    async fn dismiss_thread_handoff(&self, command: ThreadHandoffDismiss) -> AppResult<u64> {
        command.validate().map_err(AppError::BadRequest)?;
        let fingerprint = command_fingerprint(&command)?;
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let scope = HandoffCommandScope {
            company_id: command.company_id,
            handoff_id: command.handoff_id,
            command_id: command.command_id,
            expected_version: command.expected_version,
            expected_generation: command.expected_generation,
            actor: command.actor_principal_id,
            visible_channel_ids: &command.visible_channel_ids,
        };
        let (locked, manager) = match accept_handoff_command(&mut tx, &scope, &fingerprint).await? {
            AcceptedCommand::Replayed(version) => return Ok(version),
            AcceptedCommand::Fenced { locked, manager } => (locked, manager),
        };
        // Dismissing unclaimed work is open to the same teammates who could have claimed it:
        // requiring a claim first would make "not through this queue" a two-step ceremony.
        authorize_handoff_actor(
            manager,
            locked.responsible_principal_id,
            command.actor_principal_id,
            true,
        )?;

        let new_version = locked
            .version
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("Thread handoff version exhausted.".into()))?;
        // `state IN ('needs_instruction', 'draft_ready')` rather than the wider `OPEN_STATES` the
        // load used: a `drafting` handoff has a run in flight, and the answer to "stop it" is to
        // let it finish or let it fail, not to leave a run writing into a dismissed handoff.
        let written = sqlx::query(
            r#"UPDATE thread_handoffs
               SET state = 'dismissed', closed_at = CURRENT_TIMESTAMP, version = $5,
                   updated_at = CURRENT_TIMESTAMP
               WHERE company_id = $1 AND id = $2 AND version = $3 AND generation = $4
                 AND state IN ('needs_instruction', 'draft_ready')"#,
        )
        .bind(command.company_id)
        .bind(command.handoff_id)
        .bind(locked.version)
        .bind(locked.generation)
        .bind(new_version)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if written.rows_affected() != 1 {
            return Err(AppError::Conflict(
                "A drafting run is in flight for this reply; wait for it to finish.".into(),
            ));
        }
        insert_handoff_event(
            &mut tx,
            HandoffEventWrite::transition(
                command.company_id,
                command.handoff_id,
                locked.generation,
                command.command_id,
                &fingerprint,
                "dismissed",
                &locked.state,
                ThreadHandoffState::Dismissed.as_str(),
                new_version,
            )
            .by("human", command.actor_principal_id),
        )
        .await?;
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

    async fn thread_handoff_draft(
        &self,
        company_id: Uuid,
        handoff_id: Uuid,
        generation: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<Option<ThreadHandoffDraft>> {
        // The channel list is in the predicate, through the handoff, for the same reason every
        // other read here has it: an id from another company or an invisible channel must answer
        // exactly as an id that produced no draft.
        let row: Option<(Uuid, Uuid, i32)> = sqlx::query_as(
            r#"SELECT run.task_id, run.draft_id, run.draft_version
               FROM thread_handoff_runs AS run
               JOIN thread_handoffs AS handoff
                 ON (handoff.company_id, handoff.id) = (run.company_id, run.handoff_id)
               WHERE run.company_id = $1 AND run.handoff_id = $2 AND run.generation = $3
                 AND run.state = 'drafted' AND run.draft_id IS NOT NULL
                 AND handoff.channel_id = ANY($4)"#,
        )
        .bind(company_id)
        .bind(handoff_id)
        .bind(generation)
        .bind(visible_channel_ids)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|(task_id, draft_id, draft_version)| {
            Ok(ThreadHandoffDraft {
                task_id,
                draft_id,
                draft_version: u32::try_from(draft_version)
                    .map_err(|_| AppError::Internal("Invalid draft version".into()))?,
            })
        })
        .transpose()
    }

    async fn expire_thread_handoff_draft(
        &self,
        company_id: Uuid,
        task_id: Uuid,
        reason: &str,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        fail_handoff_run_on(
            &mut tx,
            company_id,
            task_id,
            HandoffRunEnd::DraftExpired,
            reason,
        )
        .await?;
        tx.commit().await.map_err(AppError::from)
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
