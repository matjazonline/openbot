use super::*;
use std::collections::HashMap;

const ROUND_LIMIT: i64 = 20;
const HISTORY_LIMIT: i64 = 40;

pub(super) struct History {
    pub items: Vec<DecisionHistoryEntry>,
    pub truncated: bool,
}

/// Revision rounds are siblings of the same parent execution, not arbitrary runs on a thread.
/// The child writer inherits the parent's channel/thread; the final query checks it again.
pub(super) async fn read(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    context: &ContextRow,
) -> AppResult<History> {
    let mut rounds:Vec<(Uuid,i32)>=match (context.parent_run_id,context.parent_execution_id) {
        (Some(parent),Some(execution))=>sqlx::query_as(
            "SELECT child_run_id,child_ordinal FROM workflow_waits WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 \
             AND reason='child_run' AND child_ordinal IS NOT NULL ORDER BY child_ordinal DESC LIMIT $4")
            .bind(scope.company.as_uuid()).bind(parent).bind(execution).bind(ROUND_LIMIT+1).fetch_all(&mut **tx).await?,
        _=>Vec::new(),
    };
    let more_rounds = rounds.len() > ROUND_LIMIT as usize;
    rounds.truncate(ROUND_LIMIT as usize);
    let rounds: HashMap<Uuid, i32> = rounds.into_iter().collect();
    let mut runs: Vec<_> = rounds.keys().copied().collect();
    runs.push(scope.run.as_uuid());
    if let Some(parent) = context.parent_run_id {
        runs.push(parent);
    }
    runs.sort();
    runs.dedup();
    let mut rows: Vec<Row> = sqlx::query_as(HISTORY_SQL)
        .bind(scope.company.as_uuid())
        .bind(runs)
        .bind(context.channel_id)
        .bind(context.thread_id)
        .bind(HISTORY_LIMIT + 1)
        .fetch_all(&mut **tx)
        .await?;
    let truncated = more_rounds || rows.len() > HISTORY_LIMIT as usize;
    rows.truncate(HISTORY_LIMIT as usize);
    rows.reverse();
    let items = rows
        .into_iter()
        .map(|row| {
            Ok(DecisionHistoryEntry {
                decision: DecisionId(row.id),
                run: RunId::new(row.run_id),
                step: StepId::parse(row.step_id).map_err(|_| invalid())?,
                round: rounds.get(&row.run_id).copied(),
                artifact_digest: row.proposal_digest,
                subject: subject(&row.subject_kind)?,
                state: row.state,
                choice: row.choice,
                feedback: row.feedback,
                created_at: row.created_at,
            })
        })
        .collect::<AppResult<_>>()?;
    Ok(History { items, truncated })
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    run_id: Uuid,
    step_id: String,
    proposal_digest: String,
    subject_kind: String,
    state: String,
    choice: Option<String>,
    feedback: Option<String>,
    created_at: DateTime<Utc>,
}

// Limit cheap identities per selected run before reading feedback or joining its execution/wait.
// workflow_decisions_history serves each newest-first probe. The working set is <=22 *41 heads.
const HISTORY_SQL: &str = r#"
WITH eligible AS MATERIALIZED (
    SELECT run.id AS run_id FROM unnest($2::uuid[]) AS related(run_id)
    JOIN workflow_runs AS run ON run.company_id=$1 AND run.id=related.run_id
    WHERE run.channel_id IS NOT DISTINCT FROM $3::uuid AND run.thread_id IS NOT DISTINCT FROM $4::uuid
), heads AS MATERIALIZED (
    SELECT head.id,head.run_id,head.execution_id,head.wait_id,head.created_at FROM eligible
    CROSS JOIN LATERAL (
        SELECT id,run_id,execution_id,wait_id,created_at FROM workflow_decisions
        WHERE company_id=$1 AND run_id=eligible.run_id ORDER BY created_at DESC,id DESC LIMIT $5
    ) AS head
    ORDER BY head.created_at DESC,head.id DESC LIMIT $5
)
SELECT decision.id,decision.run_id,execution.step_id,decision.proposal_digest,decision.subject_kind,
       wait.state,decision.choice,decision.feedback,heads.created_at
FROM heads
JOIN workflow_decisions AS decision ON decision.company_id=$1 AND decision.id=heads.id
JOIN workflow_executions AS execution ON execution.company_id=$1 AND execution.id=heads.execution_id
JOIN workflow_waits AS wait ON wait.company_id=$1 AND wait.id=heads.wait_id
ORDER BY heads.created_at DESC,heads.id DESC
"#;
