//! The `channel.call` part of run inspection (gate AWC-P05-D2 r2, D1): ids, states and codes
//! only, never arguments, fields, results or desk configuration. The route authorizes every
//! run, desk and responder reference before it renders one.
use super::*;
use crate::application::workflow::channel_call_views::{
    CallPhaseFacts, CallRunState, CallWait, ChannelCallItem, ChannelCallOrigin, ChannelCallPage,
    STALLED_AFTER_SECONDS, StoredCallResult, call_phase,
};
use chrono::{DateTime, Utc};

/// A job of `execution` that has not progressed: `processing` past its lease, or `pending`
/// and due for longer than the stall bound ($STALL seconds).
macro_rules! stalled_job {
    ($execution:literal) => {
        concat!(
            "EXISTS(SELECT 1 FROM background_tasks AS job WHERE job.company_id=call.company_id \
               AND job.workflow_execution_id=",
            $execution,
            " AND ((job.status='processing' AND job.lock_expires_at<=clock_timestamp()) \
                OR (job.status='pending' AND job.run_at<=clock_timestamp()-make_interval(secs=>$4))))"
        )
    };
}

const CALLS_SQL: &str = concat!(
    "SELECT call.call_id, call.execution_id, call.agent_id, call.agent_revision_id, call.channel_id, \
            desk.name AS desk_name, call.binding_id, call.binding_revision, call.child_run_id, \
            child.channel_id AS child_channel_id, child.principal_id AS child_principal_id, \
            child.thread_id AS child_thread_id, \
            call.scope_manifest_id, call.created_at, wait.deadline, wait.state AS wait_state, \
            wait.state='waiting' AND wait.deadline<=clock_timestamp() AS past_deadline, \
            result.closure, COALESCE(result.result->>'code', result.result#>>'{error,code}') AS code, \
            child.state AS child_state, child.waiting_reason AS child_waiting_reason, \
            (SELECT decision.id FROM workflow_waits AS review \
               JOIN workflow_decisions AS decision ON decision.company_id=review.company_id AND decision.wait_id=review.id \
              WHERE review.company_id=child.company_id AND review.run_id=child.id AND review.state='waiting' \
                AND decision.reply_proposal_id IS NOT NULL ORDER BY decision.id LIMIT 1) AS final_review, \
            EXISTS(SELECT 1 FROM workflow_executions AS step WHERE step.company_id=child.company_id \
                     AND step.run_id=child.id AND step.completed_at IS NULL AND ",
    stalled_job!("step.id"),
    ") AS child_stalled, \
            caller.state AS caller_state, execution.completed_at IS NOT NULL AS caller_execution_completed, \
            EXISTS(SELECT 1 FROM task_harness_runs AS checkpoint \
                     CROSS JOIN LATERAL jsonb_array_elements(COALESCE(checkpoint.checkpoint->'invocations','[]'::jsonb)) AS saved(invocation) \
                    WHERE checkpoint.company_id=call.company_id AND checkpoint.workflow_run_id=call.run_id \
                      AND checkpoint.workflow_execution_id=call.execution_id \
                      AND saved.invocation->>'state'='completed' \
                      AND saved.invocation->'call'->>'invocation_id'=call.call_id::text) AS caller_consumed, ",
    stalled_job!("call.execution_id"),
    " AS caller_stalled \
       FROM workflow_channel_calls AS call \
       JOIN channels AS desk ON desk.company_id=call.company_id AND desk.id=call.channel_id \
       JOIN workflow_runs AS child ON child.company_id=call.company_id AND child.id=call.child_run_id \
       JOIN workflow_runs AS caller ON caller.company_id=call.company_id AND caller.id=call.run_id \
       JOIN workflow_executions AS execution ON execution.company_id=call.company_id \
            AND execution.run_id=call.run_id AND execution.id=call.execution_id \
       CROSS JOIN LATERAL (SELECT parked.deadline, parked.state FROM workflow_waits AS parked \
            WHERE parked.company_id=call.company_id AND parked.run_id=call.run_id \
              AND parked.execution_id=call.execution_id AND parked.child_tool_call_id=call.call_id \
              AND parked.child_run_id=call.child_run_id \
            ORDER BY parked.created_at DESC, parked.id DESC LIMIT 1) AS wait \
       LEFT JOIN workflow_channel_call_results AS result ON result.company_id=call.company_id \
            AND result.run_id=call.run_id AND result.execution_id=call.execution_id AND result.call_id=call.call_id \
      WHERE call.company_id=$1 AND call.run_id=$2 AND ($3::uuid IS NULL OR call.execution_id=$3) \
      ORDER BY call.created_at DESC, call.call_id DESC LIMIT $5"
);

#[derive(sqlx::FromRow)]
struct CallRow {
    call_id: Uuid,
    execution_id: Uuid,
    agent_id: Uuid,
    agent_revision_id: Uuid,
    channel_id: Uuid,
    desk_name: String,
    binding_id: Uuid,
    binding_revision: i64,
    child_run_id: Uuid,
    child_channel_id: Option<Uuid>,
    child_principal_id: Option<Uuid>,
    child_thread_id: Option<Uuid>,
    scope_manifest_id: Uuid,
    created_at: DateTime<Utc>,
    deadline: DateTime<Utc>,
    wait_state: String,
    past_deadline: bool,
    closure: Option<String>,
    code: Option<String>,
    child_state: String,
    child_waiting_reason: Option<String>,
    final_review: Option<Uuid>,
    child_stalled: bool,
    caller_state: String,
    caller_execution_completed: bool,
    caller_consumed: bool,
    caller_stalled: bool,
}

impl CallRow {
    fn item(self) -> AppResult<ChannelCallItem> {
        let state = |value: &str| CallRunState::parse(value).ok_or_else(invalid);
        let facts = CallPhaseFacts {
            stored: self.closure.map(|closure| StoredCallResult {
                closure,
                code: self.code,
            }),
            wait: CallWait::parse(&self.wait_state).ok_or_else(invalid)?,
            child: state(&self.child_state)?,
            child_waiting_reason: self.child_waiting_reason,
            final_review: self.final_review,
            child_stalled: self.child_stalled,
            caller: state(&self.caller_state)?,
            caller_execution_completed: self.caller_execution_completed,
            caller_consumed: self.caller_consumed,
            caller_stalled: self.caller_stalled,
        };
        Ok(ChannelCallItem {
            call: self.call_id,
            execution: self.execution_id,
            agent: self.agent_id,
            agent_revision: self.agent_revision_id,
            desk: self.channel_id,
            desk_name: self.desk_name,
            responder: self.binding_id,
            responder_revision: self.binding_revision,
            child: RunId::new(self.child_run_id),
            child_association: super::association::association_from_columns(
                super::association::AssociationColumns {
                    channel_id: self.child_channel_id,
                    principal_id: self.child_principal_id,
                    thread_id: self.child_thread_id,
                },
            )?,
            child_readable: false,
            scope_manifest: self.scope_manifest_id,
            deadline: self.deadline,
            past_deadline: self.past_deadline,
            created_at: self.created_at,
            phase: call_phase(&facts),
        })
    }
}

#[derive(sqlx::FromRow)]
struct OriginRow {
    call_id: Uuid,
    run_id: Uuid,
    caller_channel_id: Option<Uuid>,
    caller_principal_id: Option<Uuid>,
    caller_thread_id: Option<Uuid>,
    execution_id: Uuid,
    agent_id: Uuid,
    channel_id: Uuid,
    desk_name: String,
    agent_manifest_id: Uuid,
}

impl PostgresPersistence {
    /// `run`'s calls (of one execution, when given), newest first, at most `limit`.
    pub(super) async fn channel_call_page(
        &self,
        company: CompanyId,
        run: RunId,
        viewer: WorkflowActor,
        execution: Option<Uuid>,
        limit: i64,
    ) -> AppResult<ChannelCallPage> {
        let rows: Vec<CallRow> = sqlx::query_as(CALLS_SQL)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(execution)
            .bind(STALLED_AFTER_SECONDS as f64)
            .bind(limit + 1)
            .fetch_all(self.pool())
            .await?;
        let limit = usize::try_from(limit).map_err(|_| invalid())?;
        let truncated = rows.len() > limit;
        let mut items = rows
            .into_iter()
            .take(limit)
            .map(CallRow::item)
            .collect::<AppResult<Vec<_>>>()?;
        let children: Vec<Uuid> = items.iter().map(|item| item.child.as_uuid()).collect();
        let readable = crate::adapters::persistence::view_visibility::run_levels_on(
            self.pool(),
            company,
            viewer,
            &children,
        )
        .await?;
        // Every run has an entry; only a visible one is readable at the viewer's level.
        for item in &mut items {
            item.child_readable = readable
                .get(&item.child.as_uuid())
                .is_some_and(|level| level.is_visible());
        }
        Ok(ChannelCallPage { items, truncated })
    }

    /// The call whose child `run` is, if any.
    pub(super) async fn channel_call_origin(
        &self,
        company: CompanyId,
        viewer: WorkflowActor,
        run: RunId,
    ) -> AppResult<Option<ChannelCallOrigin>> {
        let row: Option<OriginRow> = sqlx::query_as(
            "SELECT call.call_id, call.run_id, call.execution_id, call.agent_id, call.channel_id, \
                    desk.name AS desk_name, call.agent_manifest_id, caller.channel_id AS caller_channel_id, \
                    caller.principal_id AS caller_principal_id, caller.thread_id AS caller_thread_id \
               FROM workflow_channel_calls AS call \
               JOIN channels AS desk ON desk.company_id=call.company_id AND desk.id=call.channel_id \
               JOIN workflow_runs AS caller ON caller.company_id=call.company_id AND caller.id=call.run_id \
              WHERE call.company_id=$1 AND call.child_run_id=$2",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else { return Ok(None) };
        let caller_readable = crate::adapters::persistence::view_visibility::run_levels_on(
            self.pool(),
            company,
            viewer,
            &[row.run_id],
        )
        .await?
        .get(&row.run_id)
        .is_some_and(|level| level.is_visible());
        Ok(Some(ChannelCallOrigin {
            call: row.call_id,
            caller: RunId::new(row.run_id),
            caller_association: super::association::association_from_columns(
                super::association::AssociationColumns {
                    channel_id: row.caller_channel_id,
                    principal_id: row.caller_principal_id,
                    thread_id: row.caller_thread_id,
                },
            )?,
            caller_readable,
            caller_execution: row.execution_id,
            agent: row.agent_id,
            desk: row.channel_id,
            desk_name: row.desk_name,
            caller_manifest: row.agent_manifest_id,
        }))
    }
}
