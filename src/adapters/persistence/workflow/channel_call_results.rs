//! A channel call's disclosed result, written once by the consumption that first discloses it
//! (gate AWC-P04-D1 r2 D7). Replays return it; a hand-off answers with a `done` one.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::entities::access_level::AccessLevel;
use serde_json::Value;

/// The stored result of one saved call, with the level it carried into the caller.
pub(super) struct StoredResult {
    pub result: Value,
    pub level: AccessLevel,
    pub exact: bool,
}

#[derive(sqlx::FromRow)]
struct Row {
    result: Value,
    content_level: i16,
    content_exact: bool,
}

pub(super) async fn stored_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    call: crate::domain::workflow::ToolCallId,
) -> AppResult<Option<StoredResult>> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT result, content_level::smallint AS content_level, content_exact FROM workflow_channel_call_results \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.as_uuid())
    .fetch_optional(&mut *db)
    .await?;
    row.map(|row| {
        Ok(StoredResult {
            result: row.result,
            level: AccessLevel::try_from(row.content_level).map_err(|_| invalid())?,
            exact: row.content_exact,
        })
    })
    .transpose()
}

/// Inserts the first disclosed result; `false` when one already exists, so the caller's one-time
/// writes (the caller note) happen only on the inserting consumption.
pub(super) async fn insert_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    call: crate::domain::workflow::ToolCallId,
    closure: &str,
    stored: &StoredResult,
) -> AppResult<bool> {
    let inserted = sqlx::query(
        "INSERT INTO workflow_channel_call_results(company_id,run_id,execution_id,call_id,closure,result,content_level,content_exact) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call.as_uuid())
    .bind(closure)
    .bind(&stored.result)
    .bind(stored.level.as_i16())
    .bind(stored.exact)
    .execute(&mut *db)
    .await?;
    Ok(inserted.rows_affected() == 1)
}

/// The `done` result this run consumed from the call whose child is `child`: what a
/// `channel.hand_off` naming that child answers with. `None` for a call that was never made by
/// this run, never consumed, revoked, or closed otherwise.
pub(super) async fn handed_off_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
    child: RunId,
) -> AppResult<Option<Value>> {
    Ok(sqlx::query_scalar(
        "SELECT result.result FROM workflow_channel_call_results AS result \
         JOIN workflow_channel_calls AS call ON call.company_id=result.company_id AND call.run_id=result.run_id \
           AND call.execution_id=result.execution_id AND call.call_id=result.call_id \
         WHERE result.company_id=$1 AND result.run_id=$2 AND call.child_run_id=$3 AND result.closure='done'",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(child.as_uuid())
    .fetch_optional(&mut *db)
    .await?)
}
