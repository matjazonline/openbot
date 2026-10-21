//! `context.history` (06-D6 A): the run's thread history frozen at admission, as typed data.
//! Selection is pure: newest first, stopping at the first message that does not fit both
//! `max_tokens` (one serialized byte per token, the Rig reservation rule) and the run's
//! result byte cap, then returned oldest first. `omitted` and `truncated` report every cut.
use super::{
    lease::{ClaimedWorkflow, WorkflowFailure, WorkflowHandlerOutcome},
    polling::WorkflowStepKind,
    worker::WorkflowHandler,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};
use crate::entities::message::{MessageAudience, MessageRole, ThreadEntryKind};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

pub const KIND: &str = "context.history";
/// The registry's item bound: at most this many newest messages are read.
pub const MAX_HISTORY_ITEMS: usize = 256;
/// The registry's text bound, in characters; longer bodies are cut and marked.
pub const MAX_HISTORY_TEXT: usize = 16_384;

pub struct HistoryMessage {
    pub message_id: Uuid,
    pub at: DateTime<Utc>,
    pub role: MessageRole,
    pub audience: MessageAudience,
    pub entry_kind: ThreadEntryKind,
    pub author: String,
    pub subject: String,
    pub body: String,
}

/// The admission snapshot read for one claimed `context.history` step.
pub struct FrozenHistory {
    pub max_tokens: u64,
    /// Newest first, at most `MAX_HISTORY_ITEMS` eligible messages.
    pub newest: Vec<HistoryMessage>,
    /// Every eligible member of the snapshot (internal notes are never eligible).
    pub eligible: u64,
}

#[async_trait]
pub trait WorkflowHistory: Send + Sync {
    /// The claimed step's frozen `max_tokens` and the run's frozen thread history. A
    /// malformed stored message is `BadRequest`; nothing outside the run is read.
    async fn frozen_history(&self, claim: &ClaimedWorkflow) -> AppResult<FrozenHistory>;
}

fn cut(text: &str) -> (String, bool) {
    match text.char_indices().nth(MAX_HISTORY_TEXT) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

fn item(message: &HistoryMessage) -> Value {
    let (body, truncated) = cut(&message.body);
    json!({
        "message_id": message.message_id.to_string(),
        "at": message.at.to_rfc3339_opts(SecondsFormat::Micros, true),
        "role": message.role.as_str(),
        "audience": message.audience.as_str(),
        "entry_kind": message.entry_kind.as_str(),
        "author": cut(&message.author).0,
        "subject": cut(&message.subject).0,
        "body": body,
        "truncated": truncated,
    })
}

fn size(value: &Value) -> AppResult<u64> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .map_err(|_| AppError::BadRequest("Invalid workflow history".into()))
}

/// The step output. Items are added newest first while the items total stays within
/// `max_tokens` and the whole output within `max_result_bytes`; the first message that
/// does not fit ends the selection, so the result is always a contiguous recent suffix.
pub fn select(history: &FrozenHistory, max_result_bytes: usize) -> AppResult<Value> {
    let output = |items: &[Value], tokens: u64| {
        let omitted = history.eligible.saturating_sub(items.len() as u64);
        let mut ordered = items.to_vec();
        ordered.reverse();
        json!({"items": ordered, "token_count": tokens, "omitted": omitted})
    };
    let mut items: Vec<Value> = Vec::new();
    let mut tokens = 0_u64;
    for message in history.newest.iter().take(MAX_HISTORY_ITEMS) {
        let next = item(message);
        let total = tokens.saturating_add(size(&next)?);
        if total > history.max_tokens {
            break;
        }
        items.push(next);
        if size(&output(&items, total))? > max_result_bytes as u64 {
            items.pop();
            break;
        }
        tokens = total;
    }
    Ok(output(&items, tokens))
}

/// The `context.history` handler: one read, no model call and no effect.
pub struct WorkflowHistoryRuntime<P> {
    port: P,
}

impl<P: WorkflowHistory> WorkflowHistoryRuntime<P> {
    pub fn new(port: P) -> Self {
        Self { port }
    }
}

#[async_trait]
impl<P: WorkflowHistory> WorkflowHandler for WorkflowHistoryRuntime<P> {
    fn supports(&self, kind: &WorkflowStepKind) -> bool {
        kind.0 == KIND
    }

    async fn execute(
        &self,
        _: &WorkflowStepKind,
        claim: &ClaimedWorkflow,
    ) -> WorkflowHandlerOutcome {
        let result = match self.port.frozen_history(claim).await {
            Ok(history) => select(&history, claim.max_result_bytes),
            Err(error) => Err(error),
        };
        WorkflowHandlerOutcome::Completed(result.map_err(|error| failure(&error)))
    }
}

/// A malformed stored message or a refused run authority cannot heal on retry (the same
/// classes the model steps treat as terminal); storage trouble can.
fn failure(error: &AppError) -> WorkflowFailure {
    let (class, code) = match error {
        AppError::BadRequest(_) => (FailureClass::Terminal, "context.invalid_history"),
        AppError::NotFound(_) | AppError::InvalidCredentials => {
            (FailureClass::Terminal, "context.rejected")
        }
        _ => (FailureClass::Retryable, "context.unavailable"),
    };
    WorkflowFailure {
        failure: StepFailure::new(
            class,
            FailureCode::parse(code).expect("static history failure code"),
            None,
        )
        .expect("bounded history failure"),
        safety: RetrySafety::SafeToRetry,
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
