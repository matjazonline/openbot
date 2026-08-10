use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json::Value;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        message::{AttachmentMetadata, Message, MessageDirection, MessageRole},
        thread::Thread,
    },
    use_cases::thread::ThreadPersistence,
};

#[derive(sqlx::FromRow, Debug)]
pub struct ThreadDb {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub subject: String,
    pub participant_emails: Vec<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<ThreadDb> for Thread {
    fn from(db: ThreadDb) -> Self {
        Thread {
            id: db.id,
            workflow_id: db.workflow_id,
            subject: db.subject,
            participant_emails: db.participant_emails,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(sqlx::FromRow, Debug)]
pub struct MessageDb {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references_list: Vec<String>,
    pub sender: String,
    pub recipients_to: Vec<String>,
    pub recipients_cc: Vec<String>,
    pub subject: String,
    pub clean_text_body: String,
    pub raw_text_body: Option<String>,
    pub raw_html_body: Option<String>,
    pub attachments: Option<Value>,
    pub direction: String,
    pub role: String,
    pub created_at: NaiveDateTime,
}

impl TryFrom<MessageDb> for Message {
    type Error = AppError;

    fn try_from(db: MessageDb) -> AppResult<Self> {
        let direction = MessageDirection::from_str(&db.direction)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let role = MessageRole::from_str(&db.role)
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let attachments = match db.attachments {
            Some(v) if !v.is_null() => {
                let parsed: Vec<AttachmentMetadata> = serde_json::from_value(v)
                    .map_err(|e| AppError::Internal(format!("Failed to parse attachments JSON: {}", e)))?;
                Some(parsed)
            }
            _ => None,
        };

        Ok(Message {
            id: db.id,
            thread_id: db.thread_id,
            message_id: db.message_id,
            in_reply_to: db.in_reply_to,
            references_list: db.references_list,
            sender: db.sender,
            recipients_to: db.recipients_to,
            recipients_cc: db.recipients_cc,
            subject: db.subject,
            clean_text_body: db.clean_text_body,
            raw_text_body: db.raw_text_body,
            raw_html_body: db.raw_html_body,
            attachments,
            direction,
            role,
            created_at: db.created_at,
        })
    }
}

#[async_trait]
impl ThreadPersistence for PostgresPersistence {
    async fn create_thread(
        &self,
        workflow_id: Uuid,
        subject: &str,
        participant_emails: &[String],
    ) -> AppResult<Thread> {
        let id = Uuid::new_v4();
        let db = sqlx::query_as!(
            ThreadDb,
            r#"INSERT INTO threads (id, workflow_id, subject, participant_emails)
               VALUES ($1, $2, $3, $4)
               RETURNING id, workflow_id, subject, participant_emails, created_at as "created_at!", updated_at as "updated_at!""#,
            id,
            workflow_id,
            subject,
            participant_emails
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
        let db = sqlx::query_as!(
            ThreadDb,
            r#"SELECT id, workflow_id, subject, participant_emails, created_at as "created_at!", updated_at as "updated_at!"
               FROM threads WHERE id = $1"#,
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[String],
    ) -> AppResult<Thread> {
        let db = sqlx::query_as!(
            ThreadDb,
            r#"UPDATE threads
               SET participant_emails = $1, updated_at = CURRENT_TIMESTAMP
               WHERE id = $2
               RETURNING id, workflow_id, subject, participant_emails, created_at as "created_at!", updated_at as "updated_at!""#,
            participant_emails,
            id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.into())
    }

    async fn find_thread_by_message_ids(&self, message_ids: &[String]) -> AppResult<Option<Thread>> {
        if message_ids.is_empty() {
            return Ok(None);
        }

        let db = sqlx::query_as!(
            ThreadDb,
            r#"SELECT t.id, t.workflow_id, t.subject, t.participant_emails, t.created_at as "created_at!", t.updated_at as "updated_at!"
               FROM threads t
               JOIN messages m ON m.thread_id = t.id
               WHERE m.message_id = ANY($1)
               LIMIT 1"#,
            message_ids
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        Ok(db.map(Into::into))
    }

    async fn create_message(&self, message: &Message) -> AppResult<Message> {
        let attachments_json = message
            .attachments
            .as_ref()
            .map(|a| serde_json::to_value(a).unwrap_or(Value::Null));

        let db = sqlx::query_as!(
            MessageDb,
            r#"INSERT INTO messages (
                    id, thread_id, message_id, in_reply_to, references_list,
                    sender, recipients_to, recipients_cc, subject, clean_text_body,
                    raw_text_body, raw_html_body, attachments, direction, role
               )
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
               RETURNING id, thread_id, message_id, in_reply_to, references_list,
                         sender, recipients_to, recipients_cc, subject, clean_text_body,
                         raw_text_body, raw_html_body, attachments, direction, role,
                         created_at as "created_at!""#,
            message.id,
            message.thread_id,
            message.message_id,
            message.in_reply_to.as_deref(),
            &message.references_list,
            message.sender,
            &message.recipients_to,
            &message.recipients_cc,
            message.subject,
            message.clean_text_body,
            message.raw_text_body.as_deref(),
            message.raw_html_body.as_deref(),
            attachments_json,
            message.direction.as_str(),
            message.role.as_str()
        )
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;

        // Update thread's updated_at timestamp
        let _ = sqlx::query!(
            "UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1",
            message.thread_id
        )
        .execute(&self.pool)
        .await;

        db.try_into()
    }

    async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>> {
        let db = sqlx::query_as!(
            MessageDb,
            r#"SELECT id, thread_id, message_id, in_reply_to, references_list,
                      sender, recipients_to, recipients_cc, subject, clean_text_body,
                      raw_text_body, raw_html_body, attachments, direction, role,
                      created_at as "created_at!"
               FROM messages WHERE message_id = $1"#,
            message_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        match db {
            Some(d) => Ok(Some(d.try_into()?)),
            None => Ok(None),
        }
    }

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        let db_list = sqlx::query_as!(
            MessageDb,
            r#"SELECT id, thread_id, message_id, in_reply_to, references_list,
                      sender, recipients_to, recipients_cc, subject, clean_text_body,
                      raw_text_body, raw_html_body, attachments, direction, role,
                      created_at as "created_at!"
               FROM messages WHERE thread_id = $1 ORDER BY created_at ASC"#,
            thread_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        let mut messages = Vec::new();
        for db in db_list {
            messages.push(db.try_into()?);
        }
        Ok(messages)
    }
}
