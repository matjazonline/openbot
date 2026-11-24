//! A goal's status (Plan08 03D, gate 08G-P03-D2 r2): one read over the goal's newest runs, its
//! decisions, escalations, calls and spend counters. Counts, states, closures, spend, unit and
//! reviewer names and times only; never content.
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    application::ports::department::{
        GOAL_ROLLUP_WAITING_LIMIT, GOALS_LIST_LIMIT, GOALS_LIST_RUN_LIMIT, GoalCalls, GoalList,
        GoalRollup, GoalSpend, GoalSummary, GoalUnitRollup, GoalWait,
    },
    domain::workflow::CompanyId,
    entities::department::DepartmentId,
};

fn not_found() -> AppError {
    AppError::NotFound("Goal not found".into())
}

/// Whether `user` may see the goal's status (D2.5): a person who may view one of the goal's
/// desks, or who manages the goal. A unit that is not a goal is never visible as one.
pub(crate) async fn goal_visible_on(
    db: &mut PgConnection,
    user: Uuid,
    company: CompanyId,
    goal: DepartmentId,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(&format!(
        "SELECT {VISIBLE} FROM departments AS goal \
          WHERE goal.company_id = $1 AND goal.id = $3 AND goal.kind = 'goal'"
    ))
    .bind(company.as_uuid())
    .bind(user)
    .bind(goal.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .unwrap_or(false))
}

/// `goal` may be seen by user `$2` in company `$1`: management of the goal, or the view of one
/// of its desks under the rule notifications use (`Channel::viewer_access` in SQL).
const VISIBLE: &str = "(unit_management_role($2, $1, goal.id) IS NOT NULL OR EXISTS ( \
    SELECT 1 FROM channels AS desk \
      JOIN principals AS viewer ON viewer.company_id = desk.company_id \
       AND viewer.user_id = $2 AND viewer.kind = 'person' \
     WHERE desk.company_id = $1 AND desk.department_id = goal.id \
       AND notification_principal_can_view($1, desk.id, viewer.id)))";

/// The goal's newest `$3` runs, each with its working unit (its write scope, else the root).
const BASE: &str = "base AS ( \
    SELECT run.id, run.state, run.created_at, run.finished_at, \
           COALESCE((SELECT scope.unit_id FROM workflow_run_scopes AS scope \
                      WHERE scope.company_id = run.company_id AND scope.run_id = run.id \
                        AND scope.acting_principal_id IS NULL AND scope.access = 'write' LIMIT 1), \
                    (SELECT root.id FROM departments AS root \
                      WHERE root.company_id = run.company_id AND root.is_root)) AS unit \
      FROM workflow_runs AS run \
     WHERE run.company_id = $1 AND run.goal_unit_id = $2 \
     ORDER BY run.created_at DESC, run.id DESC LIMIT $3)";

/// A waiting decision: run, working unit, reviewer names, since, and how many wait in all.
type WaitRow = (Uuid, Uuid, Vec<String>, chrono::DateTime<chrono::Utc>, i64);

#[derive(sqlx::FromRow)]
struct Header {
    name: String,
    archived: bool,
    target_date: Option<chrono::NaiveDate>,
}

#[derive(sqlx::FromRow)]
struct Times {
    counted: i64,
    last_finished_at: Option<chrono::DateTime<chrono::Utc>>,
    last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
    waiting_on_people_seconds: i64,
    escalations_open: i64,
}

/// The rollup of `goal` over its newest `limit` runs. The caller has authorized the read; a
/// unit that is not a goal is `NotFound`. Callers run it in one repeatable-read, read-only
/// transaction, so every count comes from one snapshot.
pub(crate) async fn goal_rollup_on(
    db: &mut PgConnection,
    company: CompanyId,
    goal: DepartmentId,
    limit: i64,
) -> AppResult<GoalRollup> {
    let header: Header = sqlx::query_as(
        "SELECT name, archived_at IS NOT NULL AS archived, target_date FROM departments \
          WHERE company_id = $1 AND id = $2 AND kind = 'goal'",
    )
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(not_found)?;
    // One more than counted, to tell a cut set from a whole one.
    let times: Times = sqlx::query_as(&format!(
        "WITH {BASE}, counted AS (SELECT * FROM base ORDER BY created_at DESC, id DESC LIMIT $4), \
         decisions AS ( \
           SELECT decision.created_at, decision.submitted_at, \
                  COALESCE(decision.submitted_at, wait.settled_at, clock_timestamp()) AS ended_at \
             FROM counted \
             JOIN workflow_decisions AS decision \
               ON decision.company_id = $1 AND decision.run_id = counted.id \
             JOIN workflow_waits AS wait \
               ON wait.company_id = decision.company_id AND wait.id = decision.wait_id \
            WHERE EXISTS (SELECT 1 FROM workflow_decision_reviewers AS reviewer \
                            JOIN principals AS person ON person.company_id = reviewer.company_id \
                             AND person.id = reviewer.principal_id AND person.kind = 'person' \
                           WHERE reviewer.company_id = $1 AND reviewer.decision_id = decision.id)) \
         SELECT (SELECT count(*) FROM base) AS counted, \
                (SELECT max(finished_at) FROM counted) AS last_finished_at, \
                GREATEST((SELECT max(created_at) FROM counted), (SELECT max(finished_at) FROM counted), \
                         (SELECT max(submitted_at) FROM decisions)) AS last_activity_at, \
                (SELECT LEAST(COALESCE(sum(extract(epoch FROM \
                            ended_at - created_at)), 0), \
                            9223372036854775807)::bigint FROM decisions) AS waiting_on_people_seconds, \
                (SELECT count(DISTINCT escalation.run_id) FROM counted \
                   JOIN workflow_escalation_cases AS escalation \
                     ON escalation.company_id = $1 AND escalation.run_id = counted.id \
                   JOIN workflow_escalation_progress AS progress \
                     ON progress.company_id = escalation.company_id AND progress.case_id = escalation.id \
                  WHERE progress.state IN ('waiting', 'manual', 'owner_waiting')) AS escalations_open"
    ))
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .bind(limit + 1)
    .bind(limit)
    .fetch_one(&mut *db)
    .await?;
    let truncated = times.counted > limit;
    let states: Vec<(Uuid, String, i64)> = sqlx::query_as(&format!(
        "WITH {BASE} SELECT unit, state, count(*) FROM \
           (SELECT * FROM base ORDER BY created_at DESC, id DESC LIMIT $4) AS counted \
          GROUP BY unit, state"
    ))
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .bind(limit + 1)
    .bind(limit)
    .fetch_all(&mut *db)
    .await?;
    let spend: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT unit_id, spent_micros, unpriced_tokens FROM goal_spend \
          WHERE company_id = $1 AND goal_unit_id = $2",
    )
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .fetch_all(&mut *db)
    .await?;
    let mut units: std::collections::BTreeMap<Uuid, GoalUnitRollup> = Default::default();
    for (unit, state, count) in states {
        let row = units.entry(unit).or_insert_with(|| GoalUnitRollup {
            unit,
            ..Default::default()
        });
        match state.as_str() {
            "queued" => row.queued += count,
            "running" => row.running += count,
            "waiting" => row.waiting += count,
            "succeeded" => row.succeeded += count,
            "failed" => row.failed += count,
            _ => row.cancelled += count,
        }
    }
    let mut total = GoalSpend::default();
    for (unit, spent, unpriced) in spend {
        let row = units.entry(unit).or_insert_with(|| GoalUnitRollup {
            unit,
            ..Default::default()
        });
        row.spent_micros = spent;
        row.unpriced_tokens = unpriced;
        total.spent_micros = total.spent_micros.saturating_add(spent);
        total.unpriced_tokens = total.unpriced_tokens.saturating_add(unpriced);
    }
    let ids: Vec<Uuid> = units.keys().copied().collect();
    let names: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM departments WHERE company_id = $1 AND id = ANY($2)")
            .bind(company.as_uuid())
            .bind(&ids)
            .fetch_all(&mut *db)
            .await?;
    let names: std::collections::HashMap<Uuid, String> = names.into_iter().collect();
    let mut units: Vec<GoalUnitRollup> = units
        .into_values()
        .map(|mut row| {
            row.name = names.get(&row.unit).cloned();
            row
        })
        .collect();
    units.sort_by(|a, b| a.name.cmp(&b.name).then(a.unit.cmp(&b.unit)));
    total.cap_micros = sqlx::query_scalar(
        "SELECT cap_micros FROM unit_budgets \
          WHERE company_id = $1 AND unit_id = $2 AND period = 'total'",
    )
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    // Open decisions waiting on a person, oldest first.
    let waiting: Vec<WaitRow> =
        sqlx::query_as(&format!(
            "WITH {BASE}, counted AS (SELECT * FROM base ORDER BY created_at DESC, id DESC LIMIT $4) \
             SELECT counted.id, counted.unit, \
                    array_agg(DISTINCT person.display_label ORDER BY person.display_label), \
                    decision.created_at, count(*) OVER () \
               FROM counted \
               JOIN workflow_decisions AS decision \
                 ON decision.company_id = $1 AND decision.run_id = counted.id \
               JOIN workflow_waits AS wait \
                 ON wait.company_id = decision.company_id AND wait.id = decision.wait_id \
               JOIN workflow_decision_reviewers AS reviewer \
                 ON reviewer.company_id = decision.company_id AND reviewer.decision_id = decision.id \
               JOIN principals AS person ON person.company_id = reviewer.company_id \
                AND person.id = reviewer.principal_id AND person.kind = 'person' \
              WHERE decision.submitted_at IS NULL AND wait.state = 'waiting' \
              GROUP BY decision.id, counted.id, counted.unit, decision.created_at \
              ORDER BY decision.created_at, decision.id LIMIT $5"
        ))
        .bind(company.as_uuid())
        .bind(goal.as_uuid())
        .bind(limit + 1)
        .bind(limit)
        .bind(GOAL_ROLLUP_WAITING_LIMIT)
        .fetch_all(&mut *db)
        .await?;
    let waiting_total = waiting.first().map_or(0, |row| row.4);
    let unit_name = |unit: Uuid| {
        units
            .iter()
            .find(|row| row.unit == unit)
            .and_then(|row| row.name.clone())
    };
    let waiting = waiting
        .into_iter()
        .map(|(run_id, unit, reviewers, since, _)| GoalWait {
            run_id,
            unit,
            unit_name: unit_name(unit),
            reviewers,
            since,
        })
        .collect();
    // Calls by closure: launched calls by their result, open while the caller can still take
    // its answer, and refusals before launch whose key never launched, as `denied`. A call whose
    // caller ended without taking an answer has no closure and is not counted.
    let closures: Vec<(String, i64)> = sqlx::query_as(&format!(
        "WITH {BASE}, counted AS (SELECT * FROM base ORDER BY created_at DESC, id DESC LIMIT $4) \
         SELECT COALESCE(result.closure, 'open'), count(*) FROM counted \
           JOIN workflow_channel_calls AS call ON call.company_id = $1 AND call.run_id = counted.id \
           LEFT JOIN workflow_channel_call_results AS result \
             ON result.company_id = call.company_id AND result.run_id = call.run_id \
            AND result.execution_id = call.execution_id AND result.call_id = call.call_id \
          WHERE result.closure IS NOT NULL OR counted.state IN ('queued', 'running', 'waiting') \
          GROUP BY 1 \
         UNION ALL \
         SELECT 'denied', count(*) FROM counted \
           JOIN workflow_channel_call_refusals AS refusal \
             ON refusal.company_id = $1 AND refusal.run_id = counted.id \
          WHERE NOT EXISTS (SELECT 1 FROM workflow_channel_calls AS call \
                             WHERE call.company_id = refusal.company_id AND call.run_id = refusal.run_id \
                               AND call.execution_id = refusal.execution_id AND call.call_id = refusal.call_id)"
    ))
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .bind(limit + 1)
    .bind(limit)
    .fetch_all(&mut *db)
    .await?;
    let mut calls = GoalCalls::default();
    for (closure, count) in closures {
        match closure.as_str() {
            "done" => calls.done += count,
            "denied" => calls.denied += count,
            "handed_off" => calls.handed_off += count,
            "no_reply" => calls.no_reply += count,
            "error" => calls.error += count,
            _ => calls.open += count,
        }
    }
    // The first run of the whole goal, whatever the cut.
    let first_run_at = sqlx::query_scalar(
        "SELECT created_at FROM workflow_runs WHERE company_id = $1 AND goal_unit_id = $2 \
          ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .bind(company.as_uuid())
    .bind(goal.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    Ok(GoalRollup {
        goal: goal.as_uuid(),
        name: header.name,
        archived: header.archived,
        target_date: header.target_date,
        runs_counted: times.counted.min(limit),
        truncated,
        units,
        waiting,
        waiting_total,
        escalations_open: times.escalations_open,
        calls,
        spend: total,
        first_run_at,
        last_finished_at: times.last_finished_at,
        last_activity_at: times.last_activity_at,
        waiting_on_people_seconds: times.waiting_on_people_seconds,
    })
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    goal: Uuid,
    name: String,
    archived: bool,
    target_date: Option<chrono::NaiveDate>,
    lead: Option<String>,
    decider: Option<String>,
    open: i64,
    waiting: i64,
    failed: i64,
    counted: i64,
    cap_micros: Option<i64>,
    spent_micros: i64,
    unpriced_tokens: i64,
}

/// The goals `user` may see (D2.5), active or archived, at most `GOALS_LIST_LIMIT`, each counted
/// over its newest `GOALS_LIST_RUN_LIMIT` runs.
pub(crate) async fn visible_goals_on(
    db: &mut PgConnection,
    user: Uuid,
    company: CompanyId,
    archived: bool,
) -> AppResult<GoalList> {
    let mut rows: Vec<SummaryRow> = sqlx::query_as(&format!(
        "SELECT goal.id AS goal, goal.name, goal.archived_at IS NOT NULL AS archived, goal.target_date, \
                (SELECT holder.display_label FROM department_roles AS role \
                   JOIN principals AS holder ON holder.company_id = role.company_id \
                    AND holder.id = role.principal_id \
                  WHERE role.company_id = $1 AND role.department_id = goal.id AND role.role = 'lead' \
                  LIMIT 1) AS lead, \
                (SELECT holder.display_label FROM department_roles AS role \
                   JOIN principals AS holder ON holder.company_id = role.company_id \
                    AND holder.id = role.principal_id \
                  WHERE role.company_id = $1 AND role.department_id = goal.id \
                    AND role.role = 'sponsor' AND role.decider LIMIT 1) AS decider, \
                counts.open, counts.waiting, counts.failed, counts.counted, \
                (SELECT cap_micros FROM unit_budgets WHERE company_id = $1 AND unit_id = goal.id \
                    AND period = 'total') AS cap_micros, \
                COALESCE((SELECT LEAST(COALESCE(sum(spent_micros), 0), 9223372036854775807)::bigint FROM goal_spend \
                           WHERE company_id = $1 AND goal_unit_id = goal.id), 0) AS spent_micros, \
                COALESCE((SELECT LEAST(COALESCE(sum(unpriced_tokens), 0), 9223372036854775807)::bigint FROM goal_spend \
                           WHERE company_id = $1 AND goal_unit_id = goal.id), 0) AS unpriced_tokens \
           FROM departments AS goal \
           CROSS JOIN LATERAL ( \
             SELECT count(*) FILTER (WHERE run.n < $4 AND run.state IN ('queued', 'running', 'waiting')) AS open, \
                    count(*) FILTER (WHERE run.n < $4 AND run.state = 'waiting') AS waiting, \
                    count(*) FILTER (WHERE run.n < $4 AND run.state = 'failed') AS failed, \
                    count(*) AS counted \
               FROM (SELECT state, row_number() OVER (ORDER BY run.created_at DESC, run.id DESC) AS n \
                       FROM workflow_runs AS run \
                      WHERE run.company_id = $1 AND run.goal_unit_id = goal.id \
                      ORDER BY run.created_at DESC, run.id DESC LIMIT $4) AS run) AS counts \
          WHERE goal.company_id = $1 AND goal.kind = 'goal' \
            AND (goal.archived_at IS NOT NULL) = $3 AND {VISIBLE} \
          ORDER BY goal.created_at DESC, goal.id DESC LIMIT $5"
    ))
    .bind(company.as_uuid())
    .bind(user)
    .bind(archived)
    .bind(GOALS_LIST_RUN_LIMIT + 1)
    .bind(GOALS_LIST_LIMIT + 1)
    .fetch_all(&mut *db)
    .await?;
    let cut = rows.len() as i64 > GOALS_LIST_LIMIT;
    rows.truncate(GOALS_LIST_LIMIT as usize);
    let goals = rows
        .into_iter()
        .map(|row| {
            let truncated = row.counted > GOALS_LIST_RUN_LIMIT;
            GoalSummary {
                goal: row.goal,
                name: row.name,
                archived: row.archived,
                target_date: row.target_date,
                lead: row.lead,
                decider: row.decider,
                open: row.open,
                waiting: row.waiting,
                failed: row.failed,
                truncated,
                spend: GoalSpend {
                    cap_micros: row.cap_micros,
                    spent_micros: row.spent_micros,
                    unpriced_tokens: row.unpriced_tokens,
                },
            }
        })
        .collect();
    Ok(GoalList { goals, cut })
}

/// The active goals `user` may see (D2.5), by name, for the board's select: at most
/// `GOALS_LIST_LIMIT` and whether there were more. Names only, so a board load stays cheap.
pub(crate) async fn goal_choices_on(
    db: &mut PgConnection,
    user: Uuid,
    company: CompanyId,
) -> AppResult<(Vec<(Uuid, String)>, bool)> {
    let mut rows: Vec<(Uuid, String)> = sqlx::query_as(&format!(
        "SELECT goal.id, goal.name FROM departments AS goal \
          WHERE goal.company_id = $1 AND goal.kind = 'goal' AND goal.archived_at IS NULL \
            AND {VISIBLE} ORDER BY goal.name, goal.id LIMIT $3"
    ))
    .bind(company.as_uuid())
    .bind(user)
    .bind(GOALS_LIST_LIMIT + 1)
    .fetch_all(&mut *db)
    .await?;
    let cut = rows.len() as i64 > GOALS_LIST_LIMIT;
    rows.truncate(GOALS_LIST_LIMIT as usize);
    Ok((rows, cut))
}
