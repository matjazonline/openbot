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
    workflow_id: Option<Uuid>,
    agent_id: Uuid,
    input_tokens: i64,
    output_tokens: i64,
    usage_source: Option<String>,
    input_price: Option<i64>,
    output_price: Option<i64>,
}

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
}

#[async_trait]
impl ModelUsageStore for PostgresPersistence {
    async fn usage_report(&self, query: UsageQuery) -> AppResult<UsageReport> {
        query.validate()?;
        let mut tx = self.pool.begin().await?;
        authorize(&mut tx, query.scope).await?;
        validate_filters(&mut tx, &query).await?;
        let mut calls = load_calls(&mut tx, &query).await?;
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
        tx.commit().await?;
        Ok(UsageReport {
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

async fn load_calls(tx: &mut PgConnection, query: &UsageQuery) -> AppResult<Vec<CallRow>> {
    // Bound the scanned company/time working set before optional filters and grouping.
    // The extra row makes every incomplete report explicit, including selective filters.
    Ok(sqlx::query_as(
        "WITH calls AS MATERIALIZED (
           SELECT requested_at,request_id,provider,model,workflow_id,agent_id,reserved_input,reserved_output,input_tokens,output_tokens,usage_source
           FROM model_usage_calls WHERE company_id=$1 AND requested_at >= $2 AND requested_at < $3
           ORDER BY requested_at,request_id LIMIT $4)
         SELECT calls.provider,calls.model,calls.workflow_id,calls.agent_id,
           COALESCE(calls.input_tokens,calls.reserved_input) AS input_tokens,
           COALESCE(calls.output_tokens,calls.reserved_output) AS output_tokens,calls.usage_source,
           prices.input_micro_usd_per_million AS input_price,prices.output_micro_usd_per_million AS output_price
         FROM calls LEFT JOIN model_prices AS prices ON prices.company_id=$1
           AND prices.provider=calls.provider AND prices.model=calls.model ORDER BY calls.requested_at,calls.request_id",
    ).bind(query.scope.company).bind(query.start).bind(query.end).bind((MAX_CALLS + 1) as i64)
        .fetch_all(tx).await?)
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
            input: u64::try_from(row.input_tokens)
                .map_err(|_| AppError::Internal("Invalid input usage".into()))?,
            output: u64::try_from(row.output_tokens)
                .map_err(|_| AppError::Internal("Invalid output usage".into()))?,
            source,
            prices,
        })
    }
}
