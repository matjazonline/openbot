//! A channel policy stores a duration; each response owns one immutable review deadline.
use crate::app_error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct ReplyReviewTimeout(u32);

impl TryFrom<u32> for ReplyReviewTimeout {
    type Error = &'static str;
    fn try_from(seconds: u32) -> Result<Self, Self::Error> {
        (1..=86_400)
            .contains(&seconds)
            .then_some(Self(seconds))
            .ok_or("Final reply review timeout must be 1..86400 seconds")
    }
}
impl From<ReplyReviewTimeout> for u32 {
    fn from(value: ReplyReviewTimeout) -> Self {
        value.0
    }
}
impl ReplyReviewTimeout {
    pub fn seconds(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ApprovalWire", into = "ApprovalWire")]
pub struct ReplyApprovalPolicy {
    reviewers: Vec<Uuid>,
    timeout: ReplyReviewTimeout,
}

impl ReplyApprovalPolicy {
    pub fn new(reviewers: Vec<Uuid>, review_timeout_seconds: u32) -> AppResult<Self> {
        if reviewers.is_empty()
            || reviewers.len() > super::super::registry::MAX_REVIEWERS
            || reviewers.iter().any(Uuid::is_nil)
            || reviewers.iter().collect::<BTreeSet<_>>().len() != reviewers.len()
        {
            return Err(invalid());
        }
        Ok(Self {
            reviewers,
            timeout: review_timeout_seconds.try_into().map_err(|_| invalid())?,
        })
    }
    pub fn reviewers(&self) -> &[Uuid] {
        &self.reviewers
    }
    pub fn timeout(&self) -> ReplyReviewTimeout {
        self.timeout
    }

    /// Persist the returned deadline with the first proposal; replay must read that value.
    pub fn materialize(&self, now: DateTime<Utc>, run_deadline: DateTime<Utc>) -> AppResult<Value> {
        let deadline = now
            .checked_add_signed(chrono::Duration::seconds(i64::from(self.timeout.0)))
            .ok_or_else(invalid)?
            .min(run_deadline);
        if deadline <= now {
            return Err(invalid());
        }
        Ok(
            json!({"reviewers":self.reviewers.iter().map(|id|json!({"kind":"user","id":id})).collect::<Vec<_>>(),
            "deadline":deadline.to_rfc3339()}),
        )
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalWire {
    reviewers: Vec<Reviewer>,
    review_timeout_seconds: ReplyReviewTimeout,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Reviewer {
    User { id: Uuid },
}
impl TryFrom<ApprovalWire> for ReplyApprovalPolicy {
    type Error = AppError;
    fn try_from(value: ApprovalWire) -> AppResult<Self> {
        Self::new(
            value
                .reviewers
                .into_iter()
                .map(|Reviewer::User { id }| id)
                .collect(),
            value.review_timeout_seconds.seconds(),
        )
    }
}
impl From<ReplyApprovalPolicy> for ApprovalWire {
    fn from(value: ReplyApprovalPolicy) -> Self {
        Self {
            reviewers: value
                .reviewers
                .into_iter()
                .map(|id| Reviewer::User { id })
                .collect(),
            review_timeout_seconds: value.timeout,
        }
    }
}
fn invalid() -> AppError {
    AppError::BadRequest("Invalid final reply approval policy".into())
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
