//! Third-party outreach: the stored row and the quorum arithmetic that decides when enough
//! targets have answered.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::outreach::{OutreachProgress, OutreachStatus},
};

#[derive(sqlx::FromRow, Debug)]
pub(crate) struct OutreachDb {
    pub(crate) id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) status: String,
    pub(crate) required_threshold_percent: f64,
    pub(crate) expires_at: DateTime<Utc>,
}

/// Targets that still count toward the threshold, and how many of them have answered.
///
/// The only place this pair is derived. All three transition paths -- a reply landing, a control
/// command, and the timeout sweep -- must weigh identical numbers, and three copies of the
/// statement is three chances for them not to.
pub(crate) async fn tally_outreach_targets(
    executor: impl sqlx::PgExecutor<'_>,
    company_id: Uuid,
    outreach_id: Uuid,
) -> AppResult<(i64, i64)> {
    sqlx::query_as(
        r#"SELECT COUNT(*) FILTER (WHERE status IN ('active', 'responded'))::bigint,
                  COUNT(*) FILTER (WHERE status = 'responded')::bigint
             FROM task_outreach_targets
            WHERE company_id = $1 AND outreach_id = $2"#,
    )
    .bind(company_id)
    .bind(outreach_id)
    .fetch_one(executor)
    .await
    .map_err(AppError::from)
}

pub(crate) fn required_response_count(target_count: i64, threshold_percent: f64) -> usize {
    ((target_count as f64 * threshold_percent / 100.0).ceil() as usize).max(1)
}

pub(crate) fn outreach_progress(
    outreach: &OutreachDb,
    status: OutreachStatus,
    target_count: i64,
    response_count: i64,
    suspended: bool,
) -> OutreachProgress {
    OutreachProgress {
        id: outreach.id,
        task_id: outreach.task_id,
        status,
        required_threshold_percent: outreach.required_threshold_percent,
        target_count: target_count as usize,
        response_count: response_count as usize,
        required_response_count: required_response_count(
            target_count,
            outreach.required_threshold_percent,
        ),
        expires_at: outreach.expires_at,
        suspended,
    }
}
