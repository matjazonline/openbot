//! Third-party outreach: the stored row and the quorum arithmetic that decides when enough
//! targets have answered.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::outreach::{OutreachProgress, OutreachStatus};

#[derive(sqlx::FromRow, Debug)]
pub(crate) struct OutreachDb {
    pub(crate) id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) status: String,
    pub(crate) required_threshold_percent: f64,
    pub(crate) expires_at: DateTime<Utc>,
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
