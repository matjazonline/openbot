//! The simulation's binding outcomes per inbound message (p08 D4.6).
use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::*;
use crate::{
    application::message_outcomes::MessageOutcomeReader,
    domain::entities::message_outcome::{
        AdmittedRun, BindingOutcome, MessageBindingOutcome, RUN_EXECUTION_LIMIT,
        RunExecutionSummary, RunModelUsage,
    },
};

/// Every intent binding of the messages' intents in this thread, through
/// `workflow_message_intents_association_key` and the intent bindings' key. A message has one
/// intent per channel and an intent one outcome per binding.
pub(crate) const MESSAGE_OUTCOMES_SQL: &str = r#"
SELECT intent.message_id, outcome.binding_id, version.title AS workflow_title, outcome.outcome,
       outcome.reason, outcome.run_id, run.state, run.waiting_reason
  FROM workflow_message_intents AS intent
  JOIN workflow_message_intent_bindings AS outcome
    ON (outcome.company_id, outcome.intent_id) = (intent.company_id, intent.id)
  JOIN workflow_binding_revisions AS revision
    ON (revision.company_id, revision.binding_id, revision.revision)
     = (outcome.company_id, outcome.binding_id, outcome.binding_revision)
  JOIN workflow_versions AS version
    ON (version.company_id, version.id) = (revision.company_id, revision.version_id)
  LEFT JOIN workflow_runs AS run
    ON (run.company_id, run.id) = (outcome.company_id, outcome.run_id)
 WHERE intent.company_id = $1 AND intent.thread_id = $2 AND intent.message_id = ANY($3)
 ORDER BY intent.message_id, outcome.binding_id
"#;

/// Each run's newest executions, through `workflow_executions_company_id_run_id_id_key`.
pub(crate) const RUN_EXECUTIONS_SQL: &str = r#"
SELECT admitted.run_id, execution.step_id, execution.activation, execution.committed_route,
       execution.created_at, execution.activated_at, execution.completed_at
  FROM unnest($2::uuid[]) AS admitted (run_id)
 CROSS JOIN LATERAL (
       SELECT step.step_id, step.activation, step.committed_route, step.created_at,
              step.activated_at, step.completed_at
         FROM workflow_executions AS step
        WHERE step.company_id = $1 AND step.run_id = admitted.run_id
        ORDER BY step.created_at DESC, step.id DESC
        LIMIT $3
       ) AS execution
 ORDER BY admitted.run_id, execution.created_at, execution.activation
"#;

/// Each run's model calls by provider and model, through `model_usage_calls_run`.
pub(crate) const RUN_MODEL_USAGE_SQL: &str = r#"
SELECT usage.run_id, usage.provider, usage.model, count(*) AS calls,
       COALESCE(sum(usage.input_tokens), 0)::bigint AS input_tokens,
       COALESCE(sum(usage.output_tokens), 0)::bigint AS output_tokens
  FROM model_usage_calls AS usage
 WHERE usage.company_id = $1 AND usage.run_id = ANY($2)
 GROUP BY usage.run_id, usage.provider, usage.model
 ORDER BY usage.run_id, usage.provider, usage.model
"#;

#[derive(sqlx::FromRow)]
struct OutcomeRow {
    message_id: Uuid,
    binding_id: Uuid,
    workflow_title: String,
    outcome: String,
    reason: Option<String>,
    run_id: Option<Uuid>,
    state: Option<String>,
    waiting_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ExecutionRow {
    run_id: Uuid,
    step_id: String,
    activation: i64,
    committed_route: Option<String>,
    created_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow)]
struct UsageRow {
    run_id: Uuid,
    provider: String,
    model: String,
    calls: i64,
    input_tokens: i64,
    output_tokens: i64,
}

#[async_trait]
impl MessageOutcomeReader for PostgresPersistence {
    async fn message_outcomes(
        &self,
        company_id: Uuid,
        thread_id: Uuid,
        message_ids: &[Uuid],
    ) -> AppResult<Vec<MessageBindingOutcome>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, OutcomeRow>(MESSAGE_OUTCOMES_SQL)
            .bind(company_id)
            .bind(thread_id)
            .bind(message_ids)
            .fetch_all(&self.pool)
            .await?;
        let run_ids: Vec<Uuid> = rows.iter().filter_map(|row| row.run_id).collect();
        let (mut executions, mut usage) = if run_ids.is_empty() {
            (HashMap::new(), HashMap::new())
        } else {
            (
                run_executions(&self.pool, company_id, &run_ids).await?,
                run_model_usage(&self.pool, company_id, &run_ids).await?,
            )
        };
        rows.into_iter()
            .map(|row| {
                let outcome = match (row.outcome.as_str(), row.run_id, row.reason) {
                    ("admitted", Some(run_id), None) => BindingOutcome::Admitted(AdmittedRun {
                        run_id,
                        state: inspection::run_state(
                            row.state.as_deref().ok_or_else(invalid)?,
                            row.waiting_reason.as_deref(),
                        )?,
                        executions: executions.remove(&run_id).unwrap_or_default(),
                        model_usage: usage.remove(&run_id).unwrap_or_default(),
                    }),
                    ("refused", None, Some(reason)) => BindingOutcome::Refused { reason },
                    ("failed", None, Some(reason)) => BindingOutcome::Failed { reason },
                    _ => return Err(invalid()),
                };
                Ok(MessageBindingOutcome {
                    message_id: row.message_id,
                    binding_id: row.binding_id,
                    workflow_title: row.workflow_title,
                    outcome,
                })
            })
            .collect()
    }
}

async fn run_executions(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    run_ids: &[Uuid],
) -> AppResult<HashMap<Uuid, Vec<RunExecutionSummary>>> {
    let rows = sqlx::query_as::<_, ExecutionRow>(RUN_EXECUTIONS_SQL)
        .bind(company_id)
        .bind(run_ids)
        .bind(RUN_EXECUTION_LIMIT as i64)
        .fetch_all(pool)
        .await?;
    let mut by_run: HashMap<Uuid, Vec<RunExecutionSummary>> = HashMap::new();
    for row in rows {
        by_run
            .entry(row.run_id)
            .or_default()
            .push(RunExecutionSummary {
                step_id: row.step_id,
                activation: row.activation,
                route: row.committed_route,
                created_at: row.created_at,
                activated_at: row.activated_at,
                completed_at: row.completed_at,
            });
    }
    Ok(by_run)
}

async fn run_model_usage(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    run_ids: &[Uuid],
) -> AppResult<HashMap<Uuid, Vec<RunModelUsage>>> {
    let rows = sqlx::query_as::<_, UsageRow>(RUN_MODEL_USAGE_SQL)
        .bind(company_id)
        .bind(run_ids)
        .fetch_all(pool)
        .await?;
    let mut by_run: HashMap<Uuid, Vec<RunModelUsage>> = HashMap::new();
    for row in rows {
        by_run.entry(row.run_id).or_default().push(RunModelUsage {
            provider: row.provider,
            model: row.model,
            calls: row.calls,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
        });
    }
    Ok(by_run)
}
