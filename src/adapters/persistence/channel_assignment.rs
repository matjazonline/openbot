//! Saving a channel edit together with everything removing an agent from the channel has to stop.
//!
//! One transaction, in this order, so a failure anywhere leaves the channel, its tasks and their
//! deliveries exactly as they were:
//!
//! 1. the channel's assignment gate, exclusively ([`crate::adapters::persistence::channel_gate`]);
//! 2. the channel row, re-checked against the tenant, and its current assignments;
//! 3. when the edit removes an agent: the affected agents (shared, the order the task triggers take
//!    them in), the bounded working set of affected tasks, their outreaches, then the tasks
//!    themselves -- revalidated under their locks -- then everything they could still publish;
//! 4. the channel settings and the assignment diff.
//!
//! Only the *diff* is applied. Rewriting every assignment would momentarily remove the agents the
//! edit keeps, and the removal semantics hang on which agents actually left.
//!
//! The whole operation runs under an explicit budget. Atomicity over an arbitrary history cannot
//! promise constant work, so the edit either finishes inside the budget or is refused with nothing
//! changed; it never stops "the first N" tasks and reports success.

use std::time::{Duration, Instant};

use sqlx::{PgPool, Postgres, Transaction};
use tracing::{info, warn};
use uuid::Uuid;

use super::{
    channel::write_channel_settings_on,
    channel_gate::{ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on},
    task::{lock_task_outreaches_on, stop_tasks_on, withdraw_pending_work_on},
};
use crate::{
    app_error::{AppError, AppResult},
    entities::task::StopActor,
    use_cases::channel::{AssignmentRemoval, ChannelUpdate, channel_not_found},
};

/// The bounds one channel edit must finish inside.
///
/// `max_tasks` and `max_dependent_rows` are the first two numbers to calibrate against production
/// data; raising either must keep a boundary test failing just past the new value, per the root
/// `AGENTS.md`. A removal that genuinely needs more than this wants a staged, resumable protocol
/// with a visible "removing" state -- not a larger atomic one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemovalBudget {
    pub(crate) max_tasks: usize,
    /// Deliveries, parts, outreaches, targets, drafts, approvals, waits and harness runs together.
    pub(crate) max_dependent_rows: i64,
    pub(crate) lock_timeout: Duration,
    /// For the whole transaction, not per statement: a long series of individually quick
    /// statements must not be able to run on indefinitely.
    pub(crate) deadline: Duration,
}

impl RemovalBudget {
    pub(crate) const DEFAULT: Self = Self {
        max_tasks: 10_000,
        max_dependent_rows: 100_000,
        lock_timeout: Duration::from_secs(2),
        deadline: Duration::from_secs(30),
    };
}

/// Save the edit and commit its removal cleanup with it, inside `budget`.
pub(crate) async fn update_channel_within(
    pool: &PgPool,
    request: ChannelUpdate,
    budget: RemovalBudget,
) -> AppResult<AssignmentRemoval> {
    let started = Instant::now();
    let (company_id, channel_id) = (request.company_id, request.channel_id);
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let applied = tokio::time::timeout(
        budget.deadline,
        update_channel_on(&mut tx, &request, budget),
    )
    .await;
    let duration_ms = started.elapsed().as_millis() as u64;

    let error = match applied {
        Ok(Ok(removal)) => {
            tx.commit().await.map_err(AppError::from)?;
            if !removal.removed_agent_ids.is_empty() {
                info!(
                    %company_id,
                    %channel_id,
                    removed_agents = removal.removed_agent_ids.len(),
                    stopped_tasks = removal.stopped_tasks,
                    cancelled_deliveries = removal.cancelled_deliveries,
                    superseded_reviews = removal.superseded_reviews,
                    potentially_sent_deliveries = removal.potentially_sent_delivery_ids.len(),
                    duration_ms,
                    "Committed a channel edit that removed agents"
                );
            }
            return Ok(removal);
        }
        // A statement or lock wait reaching the bounds `bound_statements_on` set.
        Ok(Err(AppError::DatabaseTimeout(detail))) => AppError::Timeout(format!(
            "Saving this channel waited too long for work already in progress, so nothing was \
             changed. Try again shortly. ({detail})"
        )),
        Ok(Err(error)) => error,
        Err(_elapsed) => AppError::Timeout(format!(
            "Saving this channel took longer than {} seconds, so nothing was changed. \
             Try again when fewer tasks are running.",
            budget.deadline.as_secs()
        )),
    };
    // Explicit, rather than left to the drop: the caller is told the edit did not happen, and a
    // rollback that itself fails is worth knowing about.
    if let Err(rollback) = tx.rollback().await {
        warn!(%company_id, %channel_id, error = %rollback, "Could not roll back a failed channel edit");
    }
    warn!(
        %company_id,
        %channel_id,
        outcome = removal_failure_kind(&error),
        duration_ms,
        "A channel edit was refused and rolled back"
    );
    Err(error)
}

/// The metric label for a refused edit.
fn removal_failure_kind(error: &AppError) -> &'static str {
    match error {
        AppError::Timeout(_) => "timeout",
        AppError::Conflict(_) => "refused",
        AppError::NotFound(_) => "not_found",
        _ => "error",
    }
}

/// The edit itself, on the caller's transaction. Commits nothing.
pub(crate) async fn update_channel_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
    budget: RemovalBudget,
) -> AppResult<AssignmentRemoval> {
    bound_statements_on(tx, budget).await?;
    acquire_channel_gate_on(
        tx,
        ChannelGateKey::new(request.company_id, request.channel_id),
        ChannelGateAccess::ChangeAssignments,
    )
    .await?;

    let owner_agent_id = lock_channel_on(tx, request).await?;
    let current = current_assignments_on(tx, request).await?;
    // An omitted list keeps the assignments exactly as they are, so it removes nobody.
    let requested = match request.write.agent_ids.as_deref() {
        Some(agent_ids) => requested_assignments(agent_ids),
        None => current.clone(),
    };
    let plan = AssignmentPlan::between(&current, &requested);
    if let Some(owner) = owner_agent_id
        && plan.removed.contains(&owner)
    {
        return Err(AppError::Conflict(
            "An agent-owned channel must keep its owner agent.".into(),
        ));
    }

    let removal = if plan.removed.is_empty() {
        AssignmentRemoval::default()
    } else {
        withdraw_removed_agents_work_on(tx, request, &current, &requested, &plan.removed, budget)
            .await?
    };

    write_channel_settings_on(tx, request.company_id, request.channel_id, &request.write).await?;
    apply_assignment_plan_on(tx, request, &plan).await?;
    Ok(removal)
}

/// Bound every statement and lock wait in this transaction.
///
/// `set_config(.., true)` is `SET LOCAL`, so the bounds end with the transaction. The statement
/// bound is the whole deadline: the transaction-wide deadline is enforced around the future, and
/// this makes the server stop a statement that outlives it rather than finish it for nobody.
async fn bound_statements_on(
    tx: &mut Transaction<'_, Postgres>,
    budget: RemovalBudget,
) -> AppResult<()> {
    sqlx::query(
        "SELECT set_config('lock_timeout', $1, true), set_config('statement_timeout', $2, true)",
    )
    .bind(format!("{}ms", budget.lock_timeout.as_millis()))
    .bind(format!("{}ms", budget.deadline.as_millis()))
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Lock the channel row within its tenant and return its owner agent, if it is agent-owned.
async fn lock_channel_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
) -> AppResult<Option<Uuid>> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as(
        "SELECT owner_agent_id FROM channels WHERE company_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(request.company_id)
    .bind(request.channel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    row.map(|(owner,)| owner).ok_or_else(channel_not_found)
}

/// The channel's assignments as they stand, in position order, locked.
async fn current_assignments_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT agent_id FROM channel_agents
            WHERE company_id = $1 AND channel_id = $2
            ORDER BY position
              FOR UPDATE"#,
    )
    .bind(request.company_id)
    .bind(request.channel_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// The requested assignments in order, each agent once at its first position.
fn requested_assignments(agent_ids: &[Uuid]) -> Vec<Uuid> {
    let mut seen = std::collections::HashSet::with_capacity(agent_ids.len());
    agent_ids
        .iter()
        .copied()
        .filter(|agent_id| seen.insert(*agent_id))
        .collect()
}

/// One agent at the position it should end up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placement {
    agent_id: Uuid,
    position: i32,
}

/// How to get from the current assignments to the requested ones by their difference.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AssignmentPlan {
    /// Assigned now and not requested. The only agents whose work the edit stops.
    removed: Vec<Uuid>,
    /// Kept, at a different position.
    moved: Vec<Placement>,
    added: Vec<Placement>,
    /// An offset that lifts every moved row clear of every final position, so no intermediate
    /// state collides on `channel_agents_channel_position_key`.
    offset: i32,
}

impl AssignmentPlan {
    fn between(current: &[Uuid], requested: &[Uuid]) -> Self {
        let removed = current
            .iter()
            .copied()
            .filter(|agent_id| !requested.contains(agent_id))
            .collect();
        let mut moved = Vec::new();
        let mut added = Vec::new();
        for (position, agent_id) in requested.iter().copied().enumerate() {
            let placement = Placement {
                agent_id,
                position: position as i32,
            };
            match current
                .iter()
                .position(|current_id| *current_id == agent_id)
            {
                Some(existing) if existing == position => {}
                Some(_) => moved.push(placement),
                None => added.push(placement),
            }
        }
        Self {
            removed,
            moved,
            added,
            offset: (current.len() + requested.len()) as i32,
        }
    }
}

/// Stop the removed agents' work in this channel and withdraw what it could still publish.
async fn withdraw_removed_agents_work_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
    current: &[Uuid],
    requested: &[Uuid],
    removed: &[Uuid],
    budget: RemovalBudget,
) -> AppResult<AssignmentRemoval> {
    let mut removal = AssignmentRemoval {
        removed_agent_ids: removed.to_vec(),
        ..AssignmentRemoval::default()
    };

    // Every agent a task update's `lock_task_agent_harnesses` trigger may share-lock, taken first
    // and in the trigger's order, so an agent lifecycle change holding one of them cannot deadlock
    // this edit halfway through its task locks.
    let mut agents: Vec<Uuid> = current.iter().chain(requested).copied().collect();
    agents.sort_unstable();
    agents.dedup();
    sqlx::query("SELECT id FROM agents WHERE id = ANY($1) ORDER BY id FOR SHARE")
        .bind(&agents)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;

    let principals: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM principals
            WHERE company_id = $1 AND kind = 'agent' AND agent_id = ANY($2)"#,
    )
    .bind(request.company_id)
    .bind(removed)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if principals.is_empty() {
        return Ok(removal);
    }

    let candidates = affected_tasks_on(tx, request, &principals, budget.max_tasks + 1).await?;
    if candidates.len() > budget.max_tasks {
        return Err(AppError::Conflict(format!(
            "Removing this agent would stop more than {} tasks, which is more than one save may \
             change. The channel was not changed.",
            budget.max_tasks
        )));
    }
    if candidates.is_empty() {
        return Ok(removal);
    }
    ensure_dependent_work_within_on(tx, request.company_id, &candidates, budget).await?;

    // Outreach before task, the order every workflow writer takes. The candidates were read
    // without locks; the task lock below re-checks every fact the selection relied on, so work a
    // transfer or a completion moved out of scope in the meantime is left alone.
    lock_task_outreaches_on(tx, &candidates).await?;
    let affected: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM background_tasks
            WHERE id = ANY($1) AND company_id = $2 AND channel_id = $3
              AND owner_principal_kind = 'agent' AND owner_principal_id = ANY($4)
            ORDER BY id
              FOR UPDATE"#,
    )
    .bind(&candidates)
    .bind(request.company_id)
    .bind(request.channel_id)
    .bind(&principals)
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;

    let actor = StopActor::ChannelAgentRemoved(request.actor_user_id);
    let stopped = stop_tasks_on(tx, request.company_id, &affected, actor).await?;
    let withdrawn = withdraw_pending_work_on(
        tx,
        request.company_id,
        &affected,
        actor.delivery_cancellation(),
    )
    .await?;

    removal.stopped_tasks = stopped.len() as u64;
    removal.cancelled_deliveries = withdrawn.deliveries.superseded;
    removal.superseded_reviews = withdrawn.superseded_reviews;
    removal.potentially_sent_delivery_ids = withdrawn
        .deliveries
        .potentially_sent
        .iter()
        .map(|id| id.as_uuid())
        .collect();
    Ok(removal)
}

/// The tasks a removal acts on, at most `limit` of them, in id order.
///
/// Three arms, each driven from the index that bounds it rather than from the channel's history:
///
/// - unsettled work the removed agents own here, through `background_tasks_unsettled_owner_idx` --
///   a `stopped` task only while it still holds something cancellable, so a repeated save is a
///   no-op rather than a second pass over every task ever stopped;
/// - completed tasks with output still queued, found from the live deliveries
///   (`message_deliveries_company_status_idx`) rather than by loading completed tasks;
/// - completed tasks with a review draft still pending, from the pending-draft partial index.
///
/// Ownership is required to be an agent's and the task's *primary* channel to be this one. A task
/// anchored elsewhere that merely targets this channel is not affected, and neither is an agent
/// that only appears in a task's correlation chain.
async fn affected_tasks_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
    principals: &[Uuid],
    limit: usize,
) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT id FROM (
               SELECT task.id
                 FROM background_tasks AS task
                WHERE task.company_id = $1 AND task.owner_principal_id = ANY($3)
                  AND task.status IN ('pending', 'processing', 'pending_approval',
                                      'waiting_for_third_party_reply', 'stopped', 'failed',
                                      'dead_letter')
                  AND task.channel_id = $2 AND task.owner_principal_kind = 'agent'
                  AND (
                      task.status <> 'stopped'
                      OR EXISTS (
                          SELECT 1 FROM message_deliveries AS delivery
                           WHERE delivery.task_id = task.id
                             AND delivery.status IN ('pending', 'retryable', 'sending',
                                                     'outcome_unknown')
                             AND delivery.cancellation_requested_at IS NULL
                      )
                      OR EXISTS (
                          SELECT 1 FROM response_drafts AS draft
                           WHERE draft.company_id = task.company_id AND draft.task_id = task.id
                             AND draft.status = 'pending_review'
                      )
                      OR EXISTS (
                          SELECT 1 FROM human_approvals AS approval
                           WHERE approval.task_id = task.id AND approval.status = 'pending'
                      )
                  )
               UNION
               SELECT task.id
                 FROM message_deliveries AS delivery
                 JOIN background_tasks AS task ON task.id = delivery.task_id
                WHERE delivery.company_id = $1
                  AND delivery.status IN ('pending', 'retryable', 'sending', 'outcome_unknown')
                  AND delivery.cancellation_requested_at IS NULL
                  AND task.company_id = $1 AND task.channel_id = $2
                  AND task.status = 'completed'
                  AND task.owner_principal_kind = 'agent' AND task.owner_principal_id = ANY($3)
               UNION
               SELECT task.id
                 FROM response_drafts AS draft
                 JOIN background_tasks AS task
                   ON task.company_id = draft.company_id AND task.id = draft.task_id
                WHERE draft.company_id = $1 AND draft.status = 'pending_review'
                  AND task.channel_id = $2 AND task.status = 'completed'
                  AND task.owner_principal_kind = 'agent' AND task.owner_principal_id = ANY($3)
           ) AS affected
           ORDER BY id
           LIMIT $4"#,
    )
    .bind(request.company_id)
    .bind(request.channel_id)
    .bind(principals)
    .bind(i64::try_from(limit).unwrap_or(i64::MAX))
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)
}

/// Refuse a removal whose dependent rows would exceed the budget, before any of them is locked.
///
/// Each relation is counted with its own `LIMIT cap + 1`, so the check costs at most that many rows
/// per relation however large the history is. A task cap alone does not bound this: one task can
/// carry many deliveries, parts and runs.
async fn ensure_dependent_work_within_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    task_ids: &[Uuid],
    budget: RemovalBudget,
) -> AppResult<()> {
    let dependent: i64 = sqlx::query_scalar(
        r#"SELECT
             (SELECT count(*) FROM (
                 SELECT 1 FROM message_deliveries
                  WHERE company_id = $1 AND task_id = ANY($2)
                    AND status IN ('pending', 'retryable', 'sending', 'outcome_unknown')
                    AND cancellation_requested_at IS NULL
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM message_delivery_parts AS part
                   JOIN message_deliveries AS delivery ON delivery.id = part.delivery_id
                  WHERE delivery.company_id = $1 AND delivery.task_id = ANY($2)
                    AND delivery.status IN ('pending', 'retryable')
                    AND part.status IN ('prepared', 'retryable')
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM task_outreaches
                  WHERE task_id = ANY($2)
                    AND status IN ('waiting', 'threshold_met', 'timeout_pending_approval',
                                   'proceed_partial')
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM task_outreach_targets AS target
                   JOIN task_outreaches AS outreach ON outreach.id = target.outreach_id
                  WHERE outreach.task_id = ANY($2) AND target.status = 'active'
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM response_drafts
                  WHERE company_id = $1 AND task_id = ANY($2) AND status = 'pending_review'
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM human_approvals
                  WHERE company_id = $1 AND task_id = ANY($2) AND status = 'pending'
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM task_approval_waits
                  WHERE company_id = $1 AND task_id = ANY($2) AND state = 'waiting'
                  LIMIT $3) AS capped)
           + (SELECT count(*) FROM (
                 SELECT 1 FROM task_harness_runs
                  WHERE company_id = $1 AND task_id = ANY($2) AND state <> 'superseded'
                  LIMIT $3) AS capped)"#,
    )
    .bind(company_id)
    .bind(task_ids)
    .bind(budget.max_dependent_rows.saturating_add(1))
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    if dependent > budget.max_dependent_rows {
        return Err(AppError::Conflict(format!(
            "Removing this agent would change more than {} queued deliveries, reviews and other \
             pending work, which is more than one save may change. The channel was not changed.",
            budget.max_dependent_rows
        )));
    }
    Ok(())
}

/// Apply the plan's difference to `channel_agents`, never touching a row the edit keeps in place.
async fn apply_assignment_plan_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ChannelUpdate,
    plan: &AssignmentPlan,
) -> AppResult<()> {
    if !plan.removed.is_empty() {
        sqlx::query(
            "DELETE FROM channel_agents WHERE company_id = $1 AND channel_id = $2 AND agent_id = ANY($3)",
        )
        .bind(request.company_id)
        .bind(request.channel_id)
        .bind(&plan.removed)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    if !plan.moved.is_empty() {
        let (agent_ids, positions) = placements(&plan.moved);
        // Lift first, then place: the position key is not deferrable, so a direct swap of two
        // rows would collide on the first of them.
        sqlx::query(
            r#"UPDATE channel_agents SET position = position + $4
                WHERE company_id = $1 AND channel_id = $2 AND agent_id = ANY($3)"#,
        )
        .bind(request.company_id)
        .bind(request.channel_id)
        .bind(&agent_ids)
        .bind(plan.offset)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            r#"UPDATE channel_agents AS assignment SET position = placement.position
                 FROM unnest($3::uuid[], $4::int[]) AS placement(agent_id, position)
                WHERE assignment.company_id = $1 AND assignment.channel_id = $2
                  AND assignment.agent_id = placement.agent_id"#,
        )
        .bind(request.company_id)
        .bind(request.channel_id)
        .bind(&agent_ids)
        .bind(&positions)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    if !plan.added.is_empty() {
        let (agent_ids, positions) = placements(&plan.added);
        sqlx::query(
            r#"INSERT INTO channel_agents (company_id, channel_id, agent_id, position)
               SELECT $1, $2, placement.agent_id, placement.position
                 FROM unnest($3::uuid[], $4::int[]) AS placement(agent_id, position)"#,
        )
        .bind(request.company_id)
        .bind(request.channel_id)
        .bind(&agent_ids)
        .bind(&positions)
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
}

fn placements(placements: &[Placement]) -> (Vec<Uuid>, Vec<i32>) {
    placements
        .iter()
        .map(|placement| (placement.agent_id, placement.position))
        .unzip()
}

#[cfg(test)]
#[path = "channel_assignment_tests.rs"]
mod tests;

#[cfg(test)]
mod plan_tests {
    use super::*;

    fn ids(n: usize) -> Vec<Uuid> {
        (0..n).map(|_| Uuid::new_v4()).collect()
    }

    #[test]
    fn reordering_and_adding_remove_nobody() {
        let [a, b, c] = ids(3).try_into().unwrap();
        let plan = AssignmentPlan::between(&[a, b], &[b, a, c]);
        assert!(plan.removed.is_empty());
        assert_eq!(
            plan.moved,
            vec![
                Placement {
                    agent_id: b,
                    position: 0
                },
                Placement {
                    agent_id: a,
                    position: 1
                },
            ]
        );
        assert_eq!(
            plan.added,
            vec![Placement {
                agent_id: c,
                position: 2
            }]
        );
    }

    #[test]
    fn only_agents_missing_from_the_request_are_removed() {
        let [a, b, c] = ids(3).try_into().unwrap();
        let plan = AssignmentPlan::between(&[a, b, c], &[a, c]);
        assert_eq!(plan.removed, vec![b]);
        assert_eq!(
            plan.moved,
            vec![Placement {
                agent_id: c,
                position: 1
            }]
        );
        assert!(plan.added.is_empty());
    }

    #[test]
    fn an_unchanged_assignment_list_is_a_no_op() {
        let current = ids(3);
        assert_eq!(
            AssignmentPlan::between(&current, &current),
            AssignmentPlan {
                offset: 6,
                ..AssignmentPlan::default()
            }
        );
    }

    #[test]
    fn a_repeated_agent_keeps_its_first_position() {
        let [a, b] = ids(2).try_into().unwrap();
        assert_eq!(requested_assignments(&[a, b, a]), vec![a, b]);
    }

    #[test]
    fn the_lift_clears_every_final_position() {
        let current = ids(4);
        let mut requested = current.clone();
        requested.reverse();
        let plan = AssignmentPlan::between(&current, &requested);
        let highest_final = requested.len() as i32 - 1;
        assert!(plan.offset > highest_final);
    }
}
