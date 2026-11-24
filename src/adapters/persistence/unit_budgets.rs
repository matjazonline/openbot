//! Unit spend caps (Plan07 02B): who sets them, and the admission check against the running
//! totals the settlement trigger keeps (`unit_spend`, decision j).
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult, BudgetExhausted},
    application::{ports::administration::ManagementTarget, workflow::WorkflowActor},
    domain::workflow::{CompanyId, RunId},
    entities::department::DepartmentId,
};

/// `day` or `month` (UTC, decision i), or a goal's `total` (Plan08 03C.1).
pub(crate) fn period(value: &str) -> AppResult<&'static str> {
    match value {
        "day" => Ok("day"),
        "month" => Ok("month"),
        "total" => Ok("total"),
        _ => Err(AppError::BadRequest(
            "A budget period is day, month, or total for a goal.".into(),
        )),
    }
}

/// Whether the unit is a goal, under the share lock its `total` cap needs.
async fn is_goal_on(
    db: &mut PgConnection,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT kind = 'goal' FROM departments WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .unwrap_or(false))
}

/// The unit's manager may set its cap (decision c: its lead or anyone above), under the person
/// and company locks; anyone else gets not-found.
async fn authorize_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<()> {
    super::workflow::authority::authorize_person_with_locks(
        db,
        company,
        actor,
        super::workflow::row_locks::RowLocks::Wait,
    )
    .await?;
    super::administration::management_role_on(
        db,
        actor.user_id(),
        company,
        ManagementTarget::Unit(unit),
    )
    .await?
    .ok_or_else(|| AppError::NotFound("Department not found".into()))?;
    // Plan08 02C.3: an archived unit keeps its caps and usage; changing a cap is configuration.
    super::department::require_active_unit_on(db, company, unit).await
}

/// Set (or replace) one period's cap of a unit, in micro-USD.
pub(crate) async fn set_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    unit: DepartmentId,
    period: &'static str,
    cap_micros: i64,
) -> AppResult<()> {
    if cap_micros <= 0 {
        return Err(AppError::BadRequest(
            "A budget cap is a positive amount.".into(),
        ));
    }
    authorize_on(db, actor, company, unit).await?;
    if period == "total" && !is_goal_on(db, company, unit).await? {
        return Err(AppError::BadRequest(
            "budget.total_goal_only: only a goal has a total budget.".into(),
        ));
    }
    let provenance = serde_json::to_value(crate::entities::creation::CreationProvenance::user(
        actor.user_id(),
    ))
    .map_err(|_| AppError::Internal("Invalid budget provenance".into()))?;
    sqlx::query(
        "INSERT INTO unit_budgets (company_id, unit_id, period, cap_micros, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (company_id, unit_id, period) DO UPDATE \
         SET cap_micros = EXCLUDED.cap_micros, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .bind(period)
    .bind(cap_micros)
    .bind(provenance)
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// Remove one period's cap of a unit; removing an absent cap is not an error.
pub(crate) async fn clear_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    unit: DepartmentId,
    period: &'static str,
) -> AppResult<()> {
    authorize_on(db, actor, company, unit).await?;
    sqlx::query("DELETE FROM unit_budgets WHERE company_id = $1 AND unit_id = $2 AND period = $3")
        .bind(company.as_uuid())
        .bind(unit.as_uuid())
        .bind(period)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// A unit's caps and counted spend for the current UTC day and month, for its managers.
pub(crate) async fn states_on(
    db: &mut PgConnection,
    actor: WorkflowActor,
    company: CompanyId,
    unit: DepartmentId,
) -> AppResult<Vec<crate::application::ports::unit_resources::UnitBudgetState>> {
    super::administration::management_role_on(
        db,
        actor.user_id(),
        company,
        ManagementTarget::Unit(unit),
    )
    .await?
    .ok_or_else(|| AppError::NotFound("Department not found".into()))?;
    let rows: Vec<(String, Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT period.name, budget.cap_micros, COALESCE(spend.spent_micros, 0),
                COALESCE(spend.unpriced_tokens, 0)
           FROM (VALUES ('day'), ('month')) AS period(name)
           LEFT JOIN unit_budgets AS budget ON budget.company_id = $1 AND budget.unit_id = $2
            AND budget.period = period.name
           LEFT JOIN unit_spend AS spend ON spend.company_id = $1 AND spend.unit_id = $2
            AND spend.period = period.name
            AND spend.period_start = date_trunc(period.name, clock_timestamp() AT TIME ZONE 'UTC')
                AT TIME ZONE 'UTC'
          ORDER BY period.name",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .fetch_all(&mut *db)
    .await?;
    let mut states = rows
        .into_iter()
        .map(|(name, cap, spent, unpriced)| {
            Ok(crate::application::ports::unit_resources::UnitBudgetState {
                period: period(&name)?,
                cap_micros: cap,
                spent_micros: spent,
                unpriced_tokens: unpriced,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    // A goal's `total`: its cap and the work counted for it in every unit (Plan08 03C.1).
    let goal: Option<(Option<i64>, i64, i64)> = sqlx::query_as(
        "SELECT (SELECT cap_micros FROM unit_budgets WHERE company_id = $1 AND unit_id = $2
                   AND period = 'total'),
                COALESCE((SELECT LEAST(COALESCE(sum(spent_micros), 0), 9223372036854775807)::bigint
                            FROM goal_spend WHERE company_id = $1 AND goal_unit_id = $2), 0),
                COALESCE((SELECT LEAST(COALESCE(sum(unpriced_tokens), 0), 9223372036854775807)::bigint
                            FROM goal_spend WHERE company_id = $1 AND goal_unit_id = $2), 0)
           FROM departments WHERE company_id = $1 AND id = $2 AND kind = 'goal'",
    )
    .bind(company.as_uuid())
    .bind(unit.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if let Some((cap, spent, unpriced)) = goal {
        states.push(crate::application::ports::unit_resources::UnitBudgetState {
            period: "total",
            cap_micros: cap,
            spent_micros: spent,
            unpriced_tokens: unpriced,
        });
    }
    Ok(states)
}

#[derive(sqlx::FromRow)]
struct Exhausted {
    unit: Uuid,
    name: String,
    period: String,
}

/// Admission's cap check (Plan07 02B.2, D7/D8 soft caps): from the run's write unit (the root
/// when it has none) upward, the first unit whose counted spend for the current UTC period
/// reached a cap refuses the run, the day before the month at the same unit. A bounded read —
/// at most 8 units × 2 periods by key — under the company lock admission holds; settlement does
/// not take that lock, so this sees every charge committed before it.
pub(crate) async fn check_admission_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<()> {
    let exhausted: Option<Exhausted> = sqlx::query_as(
        "WITH RECURSIVE start AS (
            SELECT COALESCE(
                (SELECT unit_id FROM workflow_run_scopes WHERE company_id = $1 AND run_id = $2
                   AND acting_principal_id IS NULL AND access = 'write' LIMIT 1),
                (SELECT id FROM departments WHERE company_id = $1 AND is_root)) AS id
         ), chain(id, parent_id, depth) AS (
            SELECT department.id, department.parent_id, 1 FROM departments AS department, start
             WHERE department.company_id = $1 AND department.id = start.id
            UNION ALL
            SELECT department.id, department.parent_id, chain.depth + 1 FROM chain
              JOIN departments AS department
                ON department.company_id = $1 AND department.id = chain.parent_id
             WHERE chain.depth < 8
         )
         SELECT chain.id AS unit, unit.name, budget.period
           FROM chain
           JOIN departments AS unit ON unit.company_id = $1 AND unit.id = chain.id
           JOIN unit_budgets AS budget ON budget.company_id = $1 AND budget.unit_id = chain.id
            AND budget.period IN ('day', 'month')
           LEFT JOIN unit_spend AS spend ON spend.company_id = $1 AND spend.unit_id = chain.id
            AND spend.period = budget.period
            AND spend.period_start = date_trunc(budget.period, clock_timestamp() AT TIME ZONE 'UTC')
                AT TIME ZONE 'UTC'
          WHERE COALESCE(spend.spent_micros, 0) >= budget.cap_micros
          ORDER BY chain.depth, budget.period
          LIMIT 1",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(row) = exhausted {
        return Err(AppError::BudgetExhausted(BudgetExhausted {
            unit: row.unit,
            name: row.name,
            period: period(&row.period)?,
        }));
    }
    // Plan08 03C.2: then the goal the run serves, against its `total` cap, counting the work
    // done for it in every unit (one counter row per unit).
    let goal: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT goal.id, goal.name
           FROM workflow_runs AS run
           JOIN departments AS goal ON goal.company_id = run.company_id AND goal.id = run.goal_unit_id
           JOIN unit_budgets AS budget ON budget.company_id = goal.company_id
            AND budget.unit_id = goal.id AND budget.period = 'total'
          WHERE run.company_id = $1 AND run.id = $2
            AND (SELECT COALESCE(sum(spend.spent_micros), 0) FROM goal_spend AS spend
                  WHERE spend.company_id = goal.company_id AND spend.goal_unit_id = goal.id)
                >= budget.cap_micros",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    match goal {
        Some((unit, name)) => Err(AppError::BudgetExhausted(BudgetExhausted {
            unit,
            name,
            period: "total",
        })),
        None => Ok(()),
    }
}
