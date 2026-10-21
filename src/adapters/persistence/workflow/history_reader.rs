//! Reader of the run's frozen thread history for `context.history` (06-D6 A). One read in
//! the claim's transaction: the admission snapshot (`workflow_history_members`), internal
//! notes excluded, counted without bodies; only the newest 256 members join their messages,
//! so a long thread never materializes bodies it cannot return.
use super::http::{begin_http, runtime};
use super::*;
use crate::application::workflow::{
    actions::ActionScope,
    history::{FrozenHistory, HistoryMessage, KIND, MAX_HISTORY_ITEMS, MAX_HISTORY_TEXT},
    lease::ClaimedWorkflow,
    polling::WorkflowStepKind,
};
use crate::entities::message::{MessageAudience, MessageRole, ThreadEntryKind};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;

#[derive(sqlx::FromRow)]
struct HistoryRow {
    message_id: Uuid,
    at: DateTime<Utc>,
    role: String,
    audience: String,
    entry_kind: String,
    author_label: String,
    author_subject: Option<String>,
    subject: String,
    body: String,
    eligible: i64,
}

impl HistoryRow {
    fn message(self) -> AppResult<HistoryMessage> {
        let malformed = || AppError::BadRequest("Invalid stored workflow history".into());
        Ok(HistoryMessage {
            message_id: self.message_id,
            at: self.at,
            role: MessageRole::from_str(&self.role).map_err(|_| malformed())?,
            audience: MessageAudience::from_str(&self.audience).map_err(|_| malformed())?,
            entry_kind: ThreadEntryKind::from_str(&self.entry_kind).map_err(|_| malformed())?,
            // The legacy display rule: the label, else the identity handle.
            author: match self.author_label.trim() {
                "" => self.author_subject.unwrap_or_else(|| "Unknown".into()),
                _ => self.author_label,
            },
            subject: self.subject,
            body: self.body,
        })
    }
}

#[async_trait]
impl crate::application::workflow::history::WorkflowHistory for PostgresPersistence {
    async fn frozen_history(&self, claim: &ClaimedWorkflow) -> AppResult<FrozenHistory> {
        let scope = claim.fence.scope;
        let mut tx = begin_http(self).await?;
        let facts = runtime::facts(
            &mut tx,
            ActionScope {
                company: scope.company,
                run: scope.run,
                execution: scope.execution,
            },
        )
        .await?;
        let inputs =
            runtime::frozen_inputs(&mut tx, &facts, claim, &WorkflowStepKind(KIND.into())).await?;
        let max_tokens = inputs
            .get("max_tokens")
            .and_then(Value::as_u64)
            .ok_or_else(invalid)?;
        // Texts are cut one character past the bound so the handler can mark truncation
        // without reading an unbounded body.
        let rows: Vec<HistoryRow> = sqlx::query_as(
            "WITH eligible AS ( \
               SELECT member.message_id, member.ordinal, member.channel_id, member.thread_id \
                 FROM workflow_history_members AS member \
                WHERE member.company_id = $1 AND member.run_id = $2 \
                  AND NOT EXISTS (SELECT 1 FROM internal_notes AS private_note \
                       WHERE private_note.company_id = member.company_id AND private_note.message_id = member.message_id) \
             ), newest AS ( \
               SELECT * FROM eligible ORDER BY ordinal DESC LIMIT $3 \
             ) \
             SELECT newest.message_id, association.created_at AS at, message.role, message.audience, \
                    association.entry_kind, author.display_label AS author_label, \
                    author_identity.subject AS author_subject, \
                    left(message.subject, $4) AS subject, left(message.clean_text_body, $4) AS body, \
                    (SELECT count(*) FROM eligible) AS eligible \
               FROM newest \
               JOIN thread_messages AS association \
                 ON (association.company_id, association.channel_id, association.thread_id, association.message_id) = \
                    ($1, newest.channel_id, newest.thread_id, newest.message_id) \
               JOIN messages AS message \
                 ON (message.company_id, message.id) = ($1, newest.message_id) \
               JOIN principals AS author \
                 ON (author.company_id, author.id) = (message.company_id, message.author_principal_id) \
               LEFT JOIN participant_identities AS author_identity \
                 ON (author_identity.company_id, author_identity.id) = (message.company_id, message.authored_identity_id) \
              ORDER BY newest.ordinal DESC",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(MAX_HISTORY_ITEMS as i64)
        .bind(MAX_HISTORY_TEXT as i32 + 1)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let eligible = rows.first().map_or(0, |row| row.eligible);
        Ok(FrozenHistory {
            max_tokens,
            eligible: u64::try_from(eligible).map_err(|_| invalid())?,
            newest: rows
                .into_iter()
                .map(HistoryRow::message)
                .collect::<AppResult<_>>()?,
        })
    }
}
