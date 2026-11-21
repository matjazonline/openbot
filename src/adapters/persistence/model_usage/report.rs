use super::*;
use crate::{
    adapters::persistence::{PostgresPersistence, workflow::authority::authorize_company},
    application::workflow::WorkflowActor,
    domain::workflow::CompanyId,
    services::model_usage::*,
};
use async_trait::async_trait;
use sqlx::FromRow;
use uuid::Uuid;

const MAX_CALLS: usize = 10_000;
const MAX_PRICES: usize = 200;

#[derive(FromRow)]
struct PriceRow {
    provider: String,
    model: String,
    input: i64,
    output: i64,
    revision: i64,
}
impl TryFrom<PriceRow> for ModelPrice {
    type Error = AppError;
    fn try_from(row: PriceRow) -> AppResult<Self> {
        Ok(Self {
            provider: row.provider.into(),
            model: row.model.into(),
            input: TokenPrice::from_micros(row.input)?,
            output: TokenPrice::from_micros(row.output)?,
            revision: row.revision,
        })
    }
}
#[derive(FromRow)]
struct CallRow {
    provider: String,
    model: String,
    workflow_id: Uuid,
    agent_id: Option<Uuid>,
    department_id: Option<Uuid>,
    department_name: Option<String>,
    desk_id: Option<Uuid>,
    desk_name: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    usage_source: Option<String>,
    input_price: Option<i64>,
    output_price: Option<i64>,
}

/// Which calls a viewer may read (Plan07 02C.2): `None` for root managers (all of them), else
/// the units they manage (a led unit and everything below it), computed once. No company row
/// lock: this is a read. Anyone managing nothing gets not-found.
async fn report_scope(tx: &mut PgConnection, scope: UsageScope) -> AppResult<Option<Vec<Uuid>>> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    let root: bool = sqlx::query_scalar("SELECT unit_management_role($1, $2, NULL) IS NOT NULL")
        .bind(scope.actor)
        .bind(scope.company)
        .fetch_one(&mut *tx)
        .await?;
    if root {
        return Ok(None);
    }
    let managed: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM departments WHERE company_id = $1 \
         AND unit_management_role($2, $1, id) IS NOT NULL",
    )
    .bind(scope.company)
    .bind(scope.actor)
    .fetch_all(&mut *tx)
    .await?;
    if managed.is_empty() {
        return Err(AppError::NotFound("Company not found".into()));
    }
    Ok(Some(managed))
}

/// Price writes stay with root management: a price changes every unit's counted spend.
async fn authorize(tx: &mut PgConnection, scope: UsageScope) -> AppResult<()> {
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await?;
    authorize_company(
        tx,
        CompanyId::new(scope.company),
        WorkflowActor::authenticated(scope.actor)?,
    )
    .await
    .map(|_| ())
}

#[async_trait]
impl ModelUsageStore for PostgresPersistence {
    async fn usage_report(&self, query: UsageQuery) -> AppResult<UsageReport> {
        query.validate()?;
        let mut tx = self.pool.begin().await?;
        let managed = report_scope(&mut tx, query.scope).await?;
        validate_filters(&mut tx, &query).await?;
        let mut calls = load_calls(&mut tx, &query, managed.as_deref()).await?;
        let truncated = calls.len() > MAX_CALLS;
        calls.truncate(MAX_CALLS);
        let scanned_calls = calls.len();
        let mut prices: Vec<PriceRow> = sqlx::query_as(
            "SELECT provider,model,input_micro_usd_per_million AS input,
             output_micro_usd_per_million AS output,revision FROM model_prices
             WHERE company_id=$1 ORDER BY provider,model LIMIT $2",
        )
        .bind(query.scope.company)
        .bind((MAX_PRICES + 1) as i64)
        .fetch_all(&mut *tx)
        .await?;
        let prices_truncated = prices.len() > MAX_PRICES;
        prices.truncate(MAX_PRICES);
        let (groups, groups_truncated) = aggregate(
            calls
                .into_iter()
                .map(TryInto::try_into)
                .collect::<AppResult<Vec<_>>>()?,
            &query,
        )?;
        let (budgets, budgets_truncated) =
            unit_budgets_on(&mut tx, query.scope.company, managed.as_deref()).await?;
        tx.commit().await?;
        Ok(UsageReport {
            budgets,
            budgets_truncated,
            prices_editable: managed.is_none(),
            start: query.start,
            end: query.end,
            grouping: query.grouping,
            groups,
            scanned_calls,
            truncated: truncated || groups_truncated,
            prices: prices
                .into_iter()
                .map(TryInto::try_into)
                .collect::<AppResult<_>>()?,
            prices_truncated,
        })
    }
    async fn unit_spend(&self, scope: UsageScope) -> AppResult<(Vec<UnitBudgetStatus>, bool)> {
        let mut tx = self.pool.begin().await?;
        let managed = report_scope(&mut tx, scope).await?;
        let spend = unit_budgets_on(&mut tx, scope.company, managed.as_deref()).await?;
        tx.commit().await?;
        Ok(spend)
    }
    async fn save_model_price(&self, write: PriceWrite) -> AppResult<()> {
        write.validate()?;
        let mut tx = self.pool.begin().await?;
        authorize(&mut tx, write.scope).await?;
        let changed = match (write.expected_revision, write.rates) {
            (None, Some(PriceRates { input, output })) => sqlx::query(
                "INSERT INTO model_prices(company_id,provider,model,input_micro_usd_per_million,output_micro_usd_per_million)
                 VALUES($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
            ).bind(write.scope.company).bind(write.provider.as_str()).bind(write.model.as_str())
                .bind(input.micros()).bind(output.micros()).execute(&mut *tx).await?.rows_affected(),
            (Some(revision), Some(PriceRates { input, output })) => sqlx::query(
                "UPDATE model_prices SET input_micro_usd_per_million=$5,output_micro_usd_per_million=$6,
                 revision=nextval('model_price_revision') WHERE company_id=$1 AND provider=$2 AND model=$3 AND revision=$4",
            ).bind(write.scope.company).bind(write.provider.as_str()).bind(write.model.as_str()).bind(revision)
                .bind(input.micros()).bind(output.micros()).execute(&mut *tx).await?.rows_affected(),
            (Some(revision), None) => sqlx::query(
                "DELETE FROM model_prices WHERE company_id=$1 AND provider=$2 AND model=$3 AND revision=$4",
            ).bind(write.scope.company).bind(write.provider.as_str()).bind(write.model.as_str()).bind(revision)
                .execute(&mut *tx).await?.rows_affected(),
            (None, None) => return Err(AppError::BadRequest("Missing price revision".into())),
        };
        if changed != 1 {
            return Err(AppError::Conflict(
                "Price changed; refresh and try again".into(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }
}

async fn validate_filters(tx: &mut PgConnection, query: &UsageQuery) -> AppResult<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT ($2::uuid IS NULL OR EXISTS(SELECT 1 FROM agents WHERE company_id=$1 AND id=$2))
         AND ($3::uuid IS NULL OR EXISTS(SELECT 1 FROM workflow_definitions WHERE company_id=$1 AND id=$3))",
    ).bind(query.scope.company).bind(query.agent).bind(query.workflow).fetch_one(tx).await?;
    if !valid {
        return Err(AppError::NotFound("Usage filter".into()));
    }
    Ok(())
}

async fn load_calls(
    tx: &mut PgConnection,
    query: &UsageQuery,
    managed: Option<&[Uuid]>,
) -> AppResult<Vec<CallRow>> {
    // Bound the scanned company/time working set before optional filters and grouping, with the
    // viewer's units and a drill-down unit applied first so they never truncate a lead's report.
    // The extra row makes every incomplete report explicit, including selective filters.
    Ok(sqlx::query_as(
        "WITH calls AS MATERIALIZED (
           SELECT requested_at,request_id,provider,model,workflow_id,agent_id,run_id,unit_id,reserved_input,reserved_output,input_tokens,output_tokens,usage_source
           FROM (
             SELECT requested_at,request_id,provider,model,workflow_id,agent_id,run_id,unit_id,reserved_input,reserved_output,input_tokens,output_tokens,usage_source
             FROM model_usage_calls WHERE company_id=$1 AND requested_at >= $2 AND requested_at < $3
               AND ($5::uuid[] IS NULL OR unit_id = ANY($5)) AND ($6::uuid IS NULL OR unit_id = $6)
             UNION ALL
             SELECT requested_at,request_id,provider,model,workflow_id,agent_id,NULL::uuid,unit_id,reserved_input,reserved_output,input_tokens,output_tokens,usage_source
             FROM model_usage_history WHERE company_id=$1 AND requested_at >= $2 AND requested_at < $3
               AND ($5::uuid[] IS NULL OR unit_id = ANY($5)) AND ($6::uuid IS NULL OR unit_id = $6)
           ) AS sources ORDER BY requested_at,request_id LIMIT $4)
         SELECT calls.provider,calls.model,calls.workflow_id,calls.agent_id,
           calls.unit_id AS department_id,
           COALESCE(department.name, 'Deleted department') AS department_name,
           desk.id AS desk_id,desk.name AS desk_name,
           COALESCE(calls.input_tokens,calls.reserved_input) AS input_tokens,
           COALESCE(calls.output_tokens,calls.reserved_output) AS output_tokens,calls.usage_source,
           prices.input_micro_usd_per_million AS input_price,prices.output_micro_usd_per_million AS output_price
         FROM calls LEFT JOIN model_prices AS prices ON prices.company_id=$1
           AND prices.provider=calls.provider AND prices.model=calls.model
         -- The department is the one the call was attributed to (Plan07 02C); its desk is the
         -- run's, which archived usage no longer has.
         LEFT JOIN departments AS department ON department.company_id=$1
           AND department.id=calls.unit_id
         LEFT JOIN workflow_runs AS run ON run.company_id=$1 AND run.id=calls.run_id
         LEFT JOIN channels AS desk ON desk.company_id=$1 AND desk.id=run.channel_id
         ORDER BY calls.requested_at,calls.request_id",
    ).bind(query.scope.company).bind(query.start).bind(query.end).bind((MAX_CALLS + 1) as i64)
        .bind(managed).bind(query.unit)
        .fetch_all(tx).await?)
}

const MAX_BUDGET_ROWS: usize = 200;

#[derive(FromRow)]
struct BudgetRow {
    unit: Uuid,
    name: String,
    period: String,
    cap: Option<i64>,
    spent: i64,
    unpriced: i64,
}

/// The viewer's departments with a cap or spend in the current UTC day or month, from the
/// running totals admission reads (Plan07 02C.1, decision j).
async fn unit_budgets_on(
    tx: &mut PgConnection,
    company: Uuid,
    managed: Option<&[Uuid]>,
) -> AppResult<(Vec<UnitBudgetStatus>, bool)> {
    let mut rows: Vec<BudgetRow> = sqlx::query_as(
        "SELECT unit.id AS unit, unit.name, period.name AS period, budget.cap_micros AS cap,
                COALESCE(spend.spent_micros, 0) AS spent,
                COALESCE(spend.unpriced_tokens, 0) AS unpriced
           FROM departments AS unit
          CROSS JOIN (VALUES ('day'), ('month')) AS period(name)
           LEFT JOIN unit_budgets AS budget ON budget.company_id = unit.company_id
            AND budget.unit_id = unit.id AND budget.period = period.name
           LEFT JOIN unit_spend AS spend ON spend.company_id = unit.company_id
            AND spend.unit_id = unit.id AND spend.period = period.name
            AND spend.period_start = date_trunc(period.name, clock_timestamp() AT TIME ZONE 'UTC')
                AT TIME ZONE 'UTC'
          WHERE unit.company_id = $1 AND ($2::uuid[] IS NULL OR unit.id = ANY($2))
            AND (budget.cap_micros IS NOT NULL OR spend.unit_id IS NOT NULL)
          ORDER BY unit.name, unit.id, period.name
          LIMIT $3",
    )
    .bind(company)
    .bind(managed)
    .bind((MAX_BUDGET_ROWS + 1) as i64)
    .fetch_all(&mut *tx)
    .await?;
    let truncated = rows.len() > MAX_BUDGET_ROWS;
    rows.truncate(MAX_BUDGET_ROWS);
    Ok((
        rows.into_iter()
            .map(|row| UnitBudgetStatus {
                unit: row.unit,
                name: row.name,
                period: row.period,
                cap_usd: row.cap.map(usd),
                spent_usd: usd(row.spent),
                remaining_usd: row.cap.map(|cap| usd((cap - row.spent).max(0))),
                unpriced_tokens: u64::try_from(row.unpriced).unwrap_or(0),
            })
            .collect(),
        truncated,
    ))
}

impl TryFrom<CallRow> for UsageSample {
    type Error = AppError;
    fn try_from(row: CallRow) -> AppResult<Self> {
        let source = match row.usage_source.as_deref() {
            None => None,
            Some("reported") => Some(TokenUsageSource::Reported),
            Some("estimated") => Some(TokenUsageSource::Estimated),
            Some("mixed") => Some(TokenUsageSource::Mixed),
            _ => return Err(AppError::Internal("Invalid usage source".into())),
        };
        let prices = match (row.input_price, row.output_price) {
            (None, None) => None,
            (Some(input), Some(output)) => Some(PriceRates {
                input: TokenPrice::from_micros(input)?,
                output: TokenPrice::from_micros(output)?,
            }),
            _ => return Err(AppError::Internal("Incomplete model price".into())),
        };
        Ok(Self {
            provider: row.provider.into(),
            model: row.model.into(),
            workflow: row.workflow_id,
            agent: row.agent_id,
            department: row.department_id.zip(row.department_name),
            desk: row.desk_id.zip(row.desk_name),
            input: u64::try_from(row.input_tokens)
                .map_err(|_| AppError::Internal("Invalid input usage".into()))?,
            output: u64::try_from(row.output_tokens)
                .map_err(|_| AppError::Internal("Invalid output usage".into()))?,
            source,
            prices,
        })
    }
}
