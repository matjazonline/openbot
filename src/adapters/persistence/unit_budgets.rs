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

/// `day` or `month` (UTC, decision i).
pub(crate) fn period(value: &str) -> AppResult<&'static str> {
    match value {
        "day" => Ok("day"),
        "month" => Ok("month"),
        _ => Err(AppError::BadRequest(
            "A budget period is either day or month.".into(),
        )),
    }
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
    Ok(())
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
    match exhausted {
        Some(row) => Err(AppError::BudgetExhausted(BudgetExhausted {
            unit: row.unit,
            name: row.name,
            period: period(&row.period)?,
        })),
        None => Ok(()),
    }
}
