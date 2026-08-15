use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{collections::HashSet, str::FromStr};
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
    pub channel_id: Uuid,
    pub subject: String,
    pub participant_emails: Vec<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<ThreadDb> for Thread {
    fn from(db: ThreadDb) -> Self {
        Thread {
            id: db.id,
            channel_id: db.channel_id,
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
    pub thread_index: Option<String>,
    pub created_at: NaiveDateTime,
}

impl TryFrom<MessageDb> for Message {
    type Error = AppError;

    fn try_from(db: MessageDb) -> AppResult<Self> {
        let direction = MessageDirection::from_str(&db.direction)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let role =
            MessageRole::from_str(&db.role).map_err(|e| AppError::Internal(e.to_string()))?;

        let attachments = match db.attachments {
            Some(v) if !v.is_null() => {
                let parsed: Vec<AttachmentMetadata> = serde_json::from_value(v).map_err(|e| {
                    AppError::Internal(format!("Failed to parse attachments JSON: {e}"))
                })?;
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
            thread_index: db.thread_index,
            created_at: db.created_at,
        })
    }
}

const THREAD_SELECT: &str = r#"
    SELECT t.id, t.channel_id, t.subject,
           COALESCE(
               (SELECT array_agg(tp.email::text ORDER BY tp.email::text)
                FROM thread_participants tp WHERE tp.thread_id = t.id),
               ARRAY[]::text[]
           ) AS participant_emails,
           t.created_at, t.updated_at
    FROM threads t
"#;

const MESSAGE_SELECT: &str = r#"
    SELECT tm.id, tm.thread_id, em.message_id, em.in_reply_to,
           em.references_list, em.sender::text AS sender, em.recipients_to,
           em.recipients_cc, em.subject, tm.clean_text_body, em.raw_text_body,
           em.raw_html_body, em.attachments, tm.direction, tm.role,
           em.thread_index, tm.created_at
    FROM thread_messages tm
    JOIN email_messages em ON em.id = tm.email_message_id
"#;

async fn load_thread(pool: &PgPool, id: Uuid) -> AppResult<Option<Thread>> {
    let query = format!("{THREAD_SELECT} WHERE t.id = $1");
    let db = sqlx::query_as::<_, ThreadDb>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?;
    Ok(db.map(Into::into))
}

fn normalized_participants(participants: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    participants
        .iter()
        .filter_map(|email| {
            let normalized = email.trim().to_lowercase();
            (!normalized.is_empty() && seen.insert(normalized.clone())).then_some(normalized)
        })
        .collect()
}

#[async_trait]
impl ThreadPersistence for PostgresPersistence {
    async fn create_thread(
        &self,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[String],
    ) -> AppResult<Thread> {
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let inserted = sqlx::query(
            r#"INSERT INTO threads (id, company_id, channel_id, subject)
               SELECT $1, company_id, id, $3 FROM channels WHERE id = $2"#,
        )
        .bind(id)
        .bind(channel_id)
        .bind(subject)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        if inserted.rows_affected() == 0 {
            return Err(AppError::Internal("Channel not found".into()));
        }

        for email in normalized_participants(participant_emails) {
            sqlx::query("INSERT INTO thread_participants (thread_id, email) VALUES ($1, $2)")
                .bind(id)
                .bind(email)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        }

        tx.commit().await.map_err(AppError::from)?;
        load_thread(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Created thread was not found".into()))
    }

    async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
        load_thread(&self.pool, id).await
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[String],
    ) -> AppResult<Thread> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        for email in normalized_participants(participant_emails) {
            sqlx::query(
                r#"INSERT INTO thread_participants (thread_id, email)
                   VALUES ($1, $2) ON CONFLICT (thread_id, email) DO NOTHING"#,
            )
            .bind(id)
            .bind(email)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        }

        let updated =
            sqlx::query("UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
        if updated.rows_affected() == 0 {
            return Err(AppError::Internal("Thread not found".into()));
        }

        tx.commit().await.map_err(AppError::from)?;
        load_thread(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Updated thread was not found".into()))
    }

    async fn find_thread_by_message_ids(
        &self,
        channel_id: Uuid,
        message_ids: &[String],
    ) -> AppResult<Option<Thread>> {
        if message_ids.is_empty() {
            return Ok(None);
        }

        let query = format!(
            r#"{THREAD_SELECT}
               JOIN thread_messages tm ON tm.thread_id = t.id
               JOIN email_messages em ON em.id = tm.email_message_id
               WHERE t.channel_id = $1 AND em.message_id = ANY($2)
               ORDER BY array_position($2, em.message_id), tm.created_at DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(channel_id)
            .bind(message_ids)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        Ok(db.map(Into::into))
    }

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &str,
    ) -> AppResult<Option<Thread>> {
        let thread_index = thread_index.trim();
        if thread_index.is_empty() {
            return Ok(None);
        }

        let query = format!(
            r#"{THREAD_SELECT}
               JOIN thread_messages tm ON tm.thread_id = t.id
               JOIN email_messages em ON em.id = tm.email_message_id
               WHERE t.channel_id = $1
                 AND em.thread_index IS NOT NULL
                 AND $2 LIKE em.thread_index || '%'
               ORDER BY length(em.thread_index) DESC, tm.created_at DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(channel_id)
            .bind(thread_index)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        Ok(db.map(Into::into))
    }

    async fn count_recent_messages(&self, thread_id: Uuid, duration_secs: i64) -> AppResult<usize> {
        let count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM thread_messages
               WHERE thread_id = $1
                 AND created_at >= CURRENT_TIMESTAMP - make_interval(secs => $2)"#,
        )
        .bind(thread_id)
        .bind(duration_secs as f64)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(count as usize)
    }

    async fn create_message(&self, message: &Message) -> AppResult<Message> {
        let attachments = message
            .attachments
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|e| AppError::Internal(format!("Failed to serialize attachments: {e}")))?;
        let email_message_id = Uuid::new_v4();
        let content_hash = canonical_message_hash(message, attachments.as_ref());
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;

        let canonical_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO email_messages (
                    id, company_id, message_id, content_hash, in_reply_to, references_list, sender,
                    recipients_to, recipients_cc, subject, raw_text_body,
                    raw_html_body, attachments, thread_index
               )
               SELECT $1, company_id, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
               FROM threads WHERE id = $2
               ON CONFLICT (company_id, message_id)
               DO UPDATE SET message_id = EXCLUDED.message_id
               WHERE email_messages.content_hash = EXCLUDED.content_hash
               RETURNING id"#,
        )
        .bind(email_message_id)
        .bind(message.thread_id)
        .bind(&message.message_id)
        .bind(content_hash)
        .bind(message.in_reply_to.as_deref())
        .bind(&message.references_list)
        .bind(&message.sender)
        .bind(&message.recipients_to)
        .bind(&message.recipients_cc)
        .bind(&message.subject)
        .bind(message.raw_text_body.as_deref())
        .bind(message.raw_html_body.as_deref())
        .bind(attachments)
        .bind(message.thread_index.as_deref())
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let association_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO thread_messages (
                    id, company_id, channel_id, thread_id, email_message_id,
                    clean_text_body, direction, role
               )
               SELECT $1, company_id, channel_id, id, $3, $4, $5, $6
               FROM threads WHERE id = $2
               ON CONFLICT (channel_id, email_message_id) DO UPDATE SET
                   clean_text_body = EXCLUDED.clean_text_body,
                   direction = EXCLUDED.direction,
                   role = EXCLUDED.role
               WHERE thread_messages.thread_id = EXCLUDED.thread_id
               RETURNING id"#,
        )
        .bind(message.id)
        .bind(message.thread_id)
        .bind(canonical_id)
        .bind(&message.clean_text_body)
        .bind(message.direction.as_str())
        .bind(message.role.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(AppError::from)?;

        sqlx::query("UPDATE threads SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(message.thread_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;
        tx.commit().await.map_err(AppError::from)?;

        let query = format!("{MESSAGE_SELECT} WHERE tm.id = $1");
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(association_id)
            .fetch_one(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.try_into()
    }

    async fn get_message_by_message_id(
        &self,
        company_id: Uuid,
        message_id: &str,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            "{MESSAGE_SELECT} WHERE em.company_id = $1 AND em.message_id = $2 \
             ORDER BY tm.created_at, tm.id LIMIT 1"
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(company_id)
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn find_outbound_reply(
        &self,
        thread_id: Uuid,
        in_reply_to: &str,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            r#"{MESSAGE_SELECT}
               WHERE tm.thread_id = $1 AND tm.direction = 'outbound'
                 AND em.in_reply_to = $2
               ORDER BY tm.created_at DESC, tm.id DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(in_reply_to)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
        let query = format!(
            r#"SELECT * FROM (
                   {MESSAGE_SELECT}
                   WHERE tm.thread_id = $1
                   ORDER BY tm.created_at DESC, tm.id DESC
                   LIMIT 200
               ) recent
               ORDER BY recent.created_at ASC, recent.id ASC"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.into_iter().map(TryInto::try_into).collect()
    }
}

fn canonical_message_hash(message: &Message, attachments: Option<&Value>) -> Vec<u8> {
    let canonical = serde_json::json!({
        "message_id": message.message_id,
        "in_reply_to": message.in_reply_to,
        "references": message.references_list,
        "sender": message.sender.to_lowercase(),
        "to": message.recipients_to,
        "cc": message.recipients_cc,
        "subject": message.subject,
        "raw_text": message.raw_text_body,
        "raw_html": message.raw_html_body,
        "attachments": attachments,
        "thread_index": message.thread_index,
    });
    Sha256::digest(canonical.to_string().as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::message::{MessageDirection, MessageRole};
    use crate::use_cases::{
        channel::ChannelPersistence, company::CompanyPersistence, user::UserPersistence,
    };

    #[tokio::test]
    async fn one_email_can_be_associated_with_isolated_channel_threads() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Ok(pool) = sqlx::PgPool::connect(&database_url).await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());

        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("thread_owner_{suffix}");
        let email = format!("{username}@example.com");
        persistence
            .create_user(&username, &email, "hash")
            .await
            .unwrap();
        let owner = UserPersistence::get_by_email(&persistence, &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            owner.id,
            "Thread Test",
            &format!("thread-test-{suffix}"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let first_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            "First",
            "first",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let second_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            "Second",
            "second",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let first_thread = persistence
            .create_thread(first_channel.id, "Subject", std::slice::from_ref(&email))
            .await
            .unwrap();
        let second_thread = persistence
            .create_thread(second_channel.id, "Subject", std::slice::from_ref(&email))
            .await
            .unwrap();

        let internet_message_id = format!("<{suffix}@example.com>");
        let message = Message {
            id: Uuid::new_v4(),
            thread_id: first_thread.id,
            message_id: internet_message_id.clone(),
            in_reply_to: None,
            references_list: vec![],
            sender: email.clone(),
            recipients_to: vec!["first@example.com".into()],
            recipients_cc: vec![],
            subject: "Subject".into(),
            clean_text_body: "First context".into(),
            raw_text_body: Some("Body".into()),
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: Some("root-index".into()),
            created_at: chrono::Utc::now().naive_utc(),
        };
        persistence.create_message(&message).await.unwrap();
        persistence
            .create_message(&Message {
                id: Uuid::new_v4(),
                thread_id: second_thread.id,
                clean_text_body: "Second context".into(),
                ..message.clone()
            })
            .await
            .unwrap();

        let canonical_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM email_messages WHERE message_id = $1")
                .bind(&internet_message_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(canonical_count, 1);
        assert_eq!(
            persistence
                .find_thread_by_message_ids(first_channel.id, &[internet_message_id.clone()])
                .await
                .unwrap()
                .unwrap()
                .id,
            first_thread.id
        );
        assert_eq!(
            persistence
                .find_thread_by_message_ids(second_channel.id, &[internet_message_id])
                .await
                .unwrap()
                .unwrap()
                .id,
            second_thread.id
        );

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }
}
