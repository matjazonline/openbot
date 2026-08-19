use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        cursor::{MessageCursor, ThreadCursor},
        message::{AttachmentMetadata, Message, MessageDirection, MessageRole},
        thread::Thread,
        value_objects::{EmailAddress, MessageId, ThreadIndex},
    },
    use_cases::thread::ThreadPersistence,
};

#[derive(sqlx::FromRow, Debug)]
pub struct ThreadDb {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub participant_emails: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ThreadDb> for Thread {
    fn from(db: ThreadDb) -> Self {
        Thread {
            id: db.id,
            channel_id: db.channel_id,
            subject: db.subject,
            participant_emails: db
                .participant_emails
                .into_iter()
                .map(EmailAddress::from)
                .collect(),
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
    pub created_at: DateTime<Utc>,
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
            message_id: MessageId::from(db.message_id),
            in_reply_to: db.in_reply_to.map(MessageId::from),
            references_list: db
                .references_list
                .into_iter()
                .map(MessageId::from)
                .collect(),
            sender: EmailAddress::from(db.sender),
            recipients_to: db
                .recipients_to
                .into_iter()
                .map(EmailAddress::from)
                .collect(),
            recipients_cc: db
                .recipients_cc
                .into_iter()
                .map(EmailAddress::from)
                .collect(),
            subject: db.subject,
            clean_text_body: db.clean_text_body,
            raw_text_body: db.raw_text_body,
            raw_html_body: db.raw_html_body,
            attachments,
            direction,
            role,
            thread_index: db.thread_index.map(ThreadIndex::from),
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

fn normalized_participants(participants: &[EmailAddress]) -> Vec<String> {
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
        participant_emails: &[EmailAddress],
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

    async fn list_threads_by_channel_id(
        &self,
        channel_id: Uuid,
        before: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        let db = if let Some(ThreadCursor { updated_at, id }) = before {
            let query = format!(
                r#"{THREAD_SELECT}
                   WHERE t.channel_id = $1 AND (t.updated_at, t.id) < ($2, $3)
                   ORDER BY t.updated_at DESC, t.id DESC
                   LIMIT $4"#
            );
            sqlx::query_as::<_, ThreadDb>(&query)
                .bind(channel_id)
                .bind(updated_at)
                .bind(id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
        } else {
            let query = format!(
                r#"{THREAD_SELECT}
                   WHERE t.channel_id = $1
                   ORDER BY t.updated_at DESC, t.id DESC
                   LIMIT $2"#
            );
            sqlx::query_as::<_, ThreadDb>(&query)
                .bind(channel_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
        }
        .map_err(AppError::from)?;

        Ok(db.into_iter().map(Into::into).collect())
    }

    async fn list_thread_last_roles(
        &self,
        thread_ids: &[Uuid],
    ) -> AppResult<HashMap<Uuid, MessageRole>> {
        if thread_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // `DISTINCT ON` with the column's own sort key, so this rides
        // `thread_messages_thread_created_idx` instead of reading each thread's history.
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            r#"SELECT DISTINCT ON (thread_id) thread_id, role
               FROM thread_messages
               WHERE thread_id = ANY($1)
               ORDER BY thread_id, created_at DESC, id DESC"#,
        )
        .bind(thread_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;

        rows.into_iter()
            .map(|(thread_id, role)| {
                let role = MessageRole::from_str(&role)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                Ok((thread_id, role))
            })
            .collect()
    }

    async fn list_threads_updated_after(
        &self,
        channel_id: Uuid,
        after: Option<ThreadCursor>,
        limit: usize,
    ) -> AppResult<Vec<Thread>> {
        // The mirror of the paging query above: that one reads *backwards* into history, this one
        // reads forwards from what a live column has already shown. Ascending, so a batch can be
        // applied in order and the newest ends up on top.
        let query = format!(
            r#"{THREAD_SELECT}
               WHERE t.channel_id = $1
                 AND ($2::timestamptz IS NULL OR (t.updated_at, t.id) > ($2, $3))
               ORDER BY t.updated_at ASC, t.id ASC
               LIMIT $4"#
        );
        let db = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(channel_id)
            .bind(after.map(|cursor| cursor.updated_at))
            .bind(after.map(|cursor| cursor.id))
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        Ok(db.into_iter().map(Into::into).collect())
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
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
        message_ids: &[MessageId],
    ) -> AppResult<Option<Thread>> {
        if message_ids.is_empty() {
            return Ok(None);
        }
        let message_id_strs: Vec<&str> = message_ids.iter().map(MessageId::as_str).collect();

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
            .bind(&message_id_strs)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        Ok(db.map(Into::into))
    }

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &ThreadIndex,
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

        let references_list: Vec<&str> = message
            .references_list
            .iter()
            .map(MessageId::as_str)
            .collect();
        let recipients_to: Vec<&str> = message
            .recipients_to
            .iter()
            .map(EmailAddress::as_str)
            .collect();
        let recipients_cc: Vec<&str> = message
            .recipients_cc
            .iter()
            .map(EmailAddress::as_str)
            .collect();

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
        .bind(message.message_id.as_str())
        .bind(content_hash)
        .bind(message.in_reply_to.as_deref())
        .bind(&references_list)
        .bind(message.sender.as_str())
        .bind(&recipients_to)
        .bind(&recipients_cc)
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
        message_id: &MessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            "{MESSAGE_SELECT} WHERE em.company_id = $1 AND em.message_id = $2 \
             ORDER BY tm.created_at, tm.id LIMIT 1"
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(company_id)
            .bind(message_id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn find_outbound_reply(
        &self,
        thread_id: Uuid,
        in_reply_to: &MessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            r#"{MESSAGE_SELECT}
               WHERE tm.thread_id = $1 AND tm.direction = 'outbound'
                  AND em.in_reply_to = $2
                  AND NOT EXISTS (
                      SELECT 1 FROM email_outbox outbox
                      JOIN task_outreach_targets target ON target.outbox_id = outbox.id
                      WHERE outbox.provider_message_id = em.message_id
                  )
               ORDER BY tm.created_at DESC, tm.id DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(in_reply_to.as_str())
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

    async fn list_messages_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<Message>> {
        // Ascending and unwrapped, unlike `list_messages_by_thread_id`: this reads *forwards* from
        // a known point, so there is no newest-N window to reverse. `(created_at, id)` as a row
        // comparison matches `thread_messages_thread_created_idx` exactly.
        let query = format!(
            r#"{MESSAGE_SELECT}
               WHERE tm.thread_id = $1
                 AND ($2::timestamptz IS NULL OR (tm.created_at, tm.id) > ($2, $3))
               ORDER BY tm.created_at ASC, tm.id ASC
               LIMIT $4"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            // Both sides stay `timestamptz`. Binding a naive value instead would make Postgres
            // promote it through the *session* `TimeZone` to compare it, so the same cursor would
            // mean different instants on a UTC server and a local one.
            .bind(after.map(|cursor| cursor.created_at))
            .bind(after.map(|cursor| cursor.id))
            .bind(limit as i64)
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
        channel::{ChannelPersistence, ChannelWrite},
        company::CompanyPersistence,
        user::UserPersistence,
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
            ChannelWrite {
                name: "First".into(),
                slug: "first".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let second_channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Second".into(),
                slug: "second".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = EmailAddress::from(email.clone());
        let first_thread = persistence
            .create_thread(
                first_channel.id,
                "Subject",
                std::slice::from_ref(&email_addr),
            )
            .await
            .unwrap();
        let second_thread = persistence
            .create_thread(
                second_channel.id,
                "Subject",
                std::slice::from_ref(&email_addr),
            )
            .await
            .unwrap();
        let another_first_thread = persistence
            .create_thread(
                first_channel.id,
                "Another subject",
                std::slice::from_ref(&email_addr),
            )
            .await
            .unwrap();

        let first_page = persistence
            .list_threads_by_channel_id(first_channel.id, None, 1)
            .await
            .unwrap();
        assert_eq!(first_page.len(), 1);
        let cursor = first_page[0].cursor();
        let second_page = persistence
            .list_threads_by_channel_id(first_channel.id, Some(cursor), 1)
            .await
            .unwrap();
        assert_eq!(second_page.len(), 1);
        assert_ne!(first_page[0].id, second_page[0].id);
        assert!(
            [first_thread.id, another_first_thread.id]
                .into_iter()
                .all(|id| id == first_page[0].id || id == second_page[0].id)
        );

        let internet_message_id = format!("<{suffix}@example.com>");
        let message = Message {
            id: Uuid::new_v4(),
            thread_id: first_thread.id,
            message_id: MessageId::from(internet_message_id.clone()),
            in_reply_to: None,
            references_list: vec![],
            sender: email_addr.clone(),
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
            created_at: chrono::Utc::now(),
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
                .find_thread_by_message_ids(
                    first_channel.id,
                    &[MessageId::from(internet_message_id.clone())]
                )
                .await
                .unwrap()
                .unwrap()
                .id,
            first_thread.id
        );
        assert_eq!(
            persistence
                .find_thread_by_message_ids(
                    second_channel.id,
                    &[MessageId::from(internet_message_id)]
                )
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

    #[tokio::test]
    async fn find_outbound_reply_excludes_outreach_outbox_messages() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return;
        };
        let Ok(pool) = sqlx::PgPool::connect(&database_url).await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool.clone());

        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("outbox_reply_owner_{suffix}");
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
            "Outbox Reply Test",
            &format!("outbox-reply-{suffix}"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Outbox Channel".into(),
                slug: format!("outbox-channel-{suffix}"),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let email_addr = EmailAddress::from(email.clone());
        let thread = persistence
            .create_thread(
                channel.id,
                "Outbox Subject",
                std::slice::from_ref(&email_addr),
            )
            .await
            .unwrap();

        let trigger_msg_id = format!("<trigger-{suffix}@example.com>");
        let outreach_msg_id = format!("<outreach-{suffix}@example.com>");

        let outreach_outbound = Message {
            id: Uuid::new_v4(),
            thread_id: thread.id,
            message_id: MessageId::from(outreach_msg_id.clone()),
            in_reply_to: Some(MessageId::from(trigger_msg_id.clone())),
            references_list: vec![],
            sender: EmailAddress::from(format!("outbox-channel-{suffix}@example.com")),
            recipients_to: vec!["target@example.com".into()],
            recipients_cc: vec![],
            subject: "Outreach".into(),
            clean_text_body: "Outreach body".into(),
            raw_text_body: None,
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Outbound,
            role: MessageRole::Agent,
            thread_index: None,
            created_at: chrono::Utc::now(),
        };
        persistence
            .create_message(&outreach_outbound)
            .await
            .unwrap();

        // Task & outreach setup in DB
        let task_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO background_tasks (id, company_id, channel_id, thread_id, task_type, status, payload) VALUES ($1, $2, $3, $4, 'email_agent_dispatch', 'waiting_for_third_party_reply', '{}')",
        )
        .bind(task_id)
        .bind(company.id)
        .bind(channel.id)
        .bind(thread.id)
        .execute(&pool)
        .await
        .unwrap();

        // `target_count` / `response_count` are not columns: both are derived by counting
        // `task_outreach_targets` (see `task.rs`), and the single target inserted below is what
        // makes this outreach one-of-one awaiting a reply.
        let outreach_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO task_outreaches (
                    id, task_id, outreach_key, status, required_threshold_percent,
                    expires_at, subject, body
               ) VALUES ($1, $2, $3, 'waiting', 100.0, $4, 'Outreach', 'Outreach body')"#,
        )
        .bind(outreach_id)
        .bind(task_id)
        .bind(&suffix)
        .bind(chrono::Utc::now() + chrono::Duration::hours(1))
        .execute(&pool)
        .await
        .unwrap();

        // `email_outbox` reaches a thread through its task, not directly: it has no channel_id or
        // thread_id. `provider_message_id` is the only field the exclusion in `find_outbound_reply`
        // matches on, and `idempotency_key` is unique across the table.
        let outbox_id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO email_outbox (
                    id, company_id, channel_id, task_id, idempotency_key, payload, status,
                    provider_message_id
               ) VALUES ($1, $2, $3, $4, $5, '{}', 'sent', $6)"#,
        )
        .bind(outbox_id)
        .bind(company.id)
        .bind(channel.id)
        .bind(task_id)
        .bind(format!("outreach:{suffix}:target:0"))
        .bind(&outreach_msg_id)
        .execute(&pool)
        .await
        .unwrap();

        // Keyed by (outreach_id, email); there is no surrogate id column.
        sqlx::query(
            "INSERT INTO task_outreach_targets (outreach_id, email, outbox_id) VALUES ($1, 'target@example.com', $2)",
        )
        .bind(outreach_id)
        .bind(outbox_id)
        .execute(&pool)
        .await
        .unwrap();

        // Verify that find_outbound_reply ignores the outreach outbound message
        let found = persistence
            .find_outbound_reply(thread.id, &MessageId::from(trigger_msg_id))
            .await
            .unwrap();
        assert!(found.is_none());

        CompanyPersistence::delete(&persistence, company.id)
            .await
            .unwrap();
    }

    /// A user, company, channel and thread to hang messages off, so the streaming tests below say
    /// only what they are actually about.
    struct ThreadFixture {
        persistence: PostgresPersistence,
        pool: PgPool,
        company_id: Uuid,
        thread: Thread,
    }

    /// `None` when there is no database to talk to — these tests skip rather than fail, matching
    /// the others in this module.
    async fn thread_fixture(label: &str) -> Option<ThreadFixture> {
        let database_url = std::env::var("DATABASE_URL").ok()?;
        let pool = sqlx::PgPool::connect(&database_url).await.ok()?;
        let persistence = PostgresPersistence::new(pool.clone());

        let suffix = Uuid::new_v4().simple().to_string();
        let username = format!("{label}_{suffix}");
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
            "Stream Test",
            // Slugs are hyphen-only; the label reads as a Rust identifier.
            &format!("{}-{suffix}", label.replace('_', "-")),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Stream".into(),
                slug: "stream".into(),
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let thread = persistence
            .create_thread(
                channel.id,
                "Subject",
                std::slice::from_ref(&EmailAddress::from(email.clone())),
            )
            .await
            .unwrap();

        Some(ThreadFixture {
            persistence,
            pool,
            company_id: company.id,
            thread,
        })
    }

    /// One message in `thread`, distinguishable by its body and pinned to `created_at` so the
    /// cursor's tie-break can be exercised deliberately.
    fn streamed_message(thread_id: Uuid, body: &str, created_at: DateTime<Utc>) -> Message {
        Message {
            id: Uuid::new_v4(),
            thread_id,
            message_id: MessageId::from(format!("<{}@example.com>", Uuid::new_v4())),
            in_reply_to: None,
            references_list: vec![],
            sender: EmailAddress::from("sender@example.com"),
            recipients_to: vec!["recipient@example.com".into()],
            recipients_cc: vec![],
            subject: "Subject".into(),
            clean_text_body: body.into(),
            raw_text_body: Some(body.into()),
            raw_html_body: None,
            attachments: None,
            direction: MessageDirection::Inbound,
            role: MessageRole::Human,
            thread_index: None,
            created_at,
        }
    }

    fn bodies(messages: &[Message]) -> Vec<&str> {
        messages
            .iter()
            .map(|message| message.clean_text_body.as_str())
            .collect()
    }

    #[tokio::test]
    async fn list_messages_after_reads_forward_from_a_cursor() {
        let Some(fixture) = thread_fixture("stream_forward").await else {
            return;
        };
        let base = chrono::Utc::now();

        let mut saved = Vec::new();
        for (offset, body) in [(0, "first"), (1, "second"), (2, "third")] {
            saved.push(
                fixture
                    .persistence
                    .create_message(&streamed_message(
                        fixture.thread.id,
                        body,
                        base + chrono::Duration::seconds(offset),
                    ))
                    .await
                    .unwrap(),
            );
        }

        // No cursor: a reader joining an empty pane gets the whole thread, oldest first.
        let all = fixture
            .persistence
            .list_messages_after(fixture.thread.id, None, 50)
            .await
            .unwrap();
        assert_eq!(bodies(&all), ["first", "second", "third"]);

        // Resuming excludes the message the cursor names -- it has already been rendered.
        let after_first = fixture
            .persistence
            .list_messages_after(fixture.thread.id, Some(saved[0].cursor()), 50)
            .await
            .unwrap();
        assert_eq!(bodies(&after_first), ["second", "third"]);

        // A reader who is up to date gets nothing, rather than the thread again.
        let after_last = fixture
            .persistence
            .list_messages_after(fixture.thread.id, Some(saved[2].cursor()), 50)
            .await
            .unwrap();
        assert!(after_last.is_empty());

        // The batch limit is what stops one wake-up loading an unbounded backlog.
        let limited = fixture
            .persistence
            .list_messages_after(fixture.thread.id, None, 2)
            .await
            .unwrap();
        assert_eq!(bodies(&limited), ["first", "second"]);

        CompanyPersistence::delete(&fixture.persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// Messages saved in one transaction share a timestamp, so a timestamp-only cursor would skip
    /// or repeat them. This is the case the `(created_at, id)` comparison exists for.
    #[tokio::test]
    async fn list_messages_after_breaks_timestamp_ties_by_id() {
        let Some(fixture) = thread_fixture("stream_ties").await else {
            return;
        };
        let shared = chrono::Utc::now();

        let mut saved = Vec::new();
        for body in ["one", "two", "three"] {
            saved.push(
                fixture
                    .persistence
                    .create_message(&streamed_message(fixture.thread.id, body, shared))
                    .await
                    .unwrap(),
            );
        }
        saved.sort_by_key(|message| message.cursor());

        let all = fixture
            .persistence
            .list_messages_after(fixture.thread.id, None, 50)
            .await
            .unwrap();
        assert_eq!(
            all.iter().map(|m| m.id).collect::<Vec<_>>(),
            saved.iter().map(|m| m.id).collect::<Vec<_>>(),
            "same instant, so ordering falls to the id"
        );

        // Resuming from the middle of a tie must return exactly the rest of it.
        let rest = fixture
            .persistence
            .list_messages_after(fixture.thread.id, Some(saved[0].cursor()), 50)
            .await
            .unwrap();
        assert_eq!(
            rest.iter().map(|m| m.id).collect::<Vec<_>>(),
            saved[1..].iter().map(|m| m.id).collect::<Vec<_>>()
        );

        CompanyPersistence::delete(&fixture.persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// The mirror of the message stream, one level up: a thread whose message just landed must
    /// surface in its channel's live column, and a column that reconnects must not replay threads
    /// it already shows.
    #[tokio::test]
    async fn list_threads_updated_after_reads_forward_from_a_cursor() {
        let Some(fixture) = thread_fixture("stream_column").await else {
            return;
        };

        // `updated_at` is set by the database, and `create_message` bumps it -- so the ordering
        // here is established the same way it is in production, not by writing timestamps.
        let mut threads = vec![fixture.thread.clone()];
        for subject in ["second", "third"] {
            threads.push(
                fixture
                    .persistence
                    .create_thread(
                        fixture.thread.channel_id,
                        subject,
                        &[EmailAddress::from("someone@example.com")],
                    )
                    .await
                    .unwrap(),
            );
        }

        let all = fixture
            .persistence
            .list_threads_updated_after(fixture.thread.channel_id, None, 50)
            .await
            .unwrap();
        assert_eq!(all.len(), 3, "no cursor means the whole channel");
        assert!(
            all.windows(2)
                .all(|pair| pair[0].cursor() < pair[1].cursor()),
            "oldest first, so the newest is applied last and lands on top"
        );

        let after_first = fixture
            .persistence
            .list_threads_updated_after(fixture.thread.channel_id, Some(all[0].cursor()), 50)
            .await
            .unwrap();
        assert_eq!(after_first.len(), 2);

        let caught_up = fixture
            .persistence
            .list_threads_updated_after(fixture.thread.channel_id, Some(all[2].cursor()), 50)
            .await
            .unwrap();
        assert!(caught_up.is_empty());

        // A message bumps its thread past every other one, which is exactly what makes the live
        // column reorder rather than just grow.
        let oldest = &threads[0];
        fixture
            .persistence
            .create_message(&streamed_message(oldest.id, "bump", chrono::Utc::now()))
            .await
            .unwrap();

        let bumped = fixture
            .persistence
            .list_threads_updated_after(fixture.thread.channel_id, Some(all[2].cursor()), 50)
            .await
            .unwrap();
        assert_eq!(
            bumped.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![oldest.id],
            "only the bumped thread is newer than what the column already showed"
        );

        CompanyPersistence::delete(&fixture.persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// The first notification for `thread_id`, or `None` if none arrives within `timeout`.
    ///
    /// `LISTEN` is database-wide, so a listener sees every thread's messages -- including those of
    /// tests running beside this one. Filtering here is not test scaffolding: it is what the SSE
    /// handler does with each broadcast event, for the same reason.
    async fn notification_for(
        listener: &mut sqlx::postgres::PgListener,
        thread_id: Uuid,
        timeout: std::time::Duration,
    ) -> Option<serde_json::Value> {
        tokio::time::timeout(timeout, async {
            loop {
                let notification = listener.recv().await.unwrap();
                let payload: serde_json::Value =
                    serde_json::from_str(notification.payload()).expect("payload should be JSON");
                if payload["thread_id"].as_str() == Some(&thread_id.to_string()) {
                    return payload;
                }
            }
        })
        .await
        .ok()
    }

    /// The link between a committed message and an open mailbox. Without the trigger firing,
    /// nothing else in the live path runs.
    #[tokio::test]
    async fn committing_a_message_notifies_listeners() {
        let Some(fixture) = thread_fixture("stream_notify").await else {
            return;
        };

        let mut listener = sqlx::postgres::PgListener::connect_with(&fixture.pool)
            .await
            .unwrap();
        listener.listen("thread_message").await.unwrap();

        fixture
            .persistence
            .create_message(&streamed_message(
                fixture.thread.id,
                "live",
                chrono::Utc::now(),
            ))
            .await
            .unwrap();

        let payload = notification_for(
            &mut listener,
            fixture.thread.id,
            std::time::Duration::from_secs(5),
        )
        .await
        .expect("expected a notification for this thread within 5s");

        assert_eq!(
            payload["channel_id"].as_str().unwrap(),
            fixture.thread.channel_id.to_string()
        );
        assert_eq!(
            payload["company_id"].as_str().unwrap(),
            fixture.company_id.to_string()
        );

        CompanyPersistence::delete(&fixture.persistence, fixture.company_id)
            .await
            .unwrap();
    }

    /// The notification is bound to the writing transaction, so a rolled-back message must never
    /// be announced -- a reader would query for it and find nothing.
    #[tokio::test]
    async fn a_rolled_back_message_is_not_announced() {
        let Some(fixture) = thread_fixture("stream_rollback").await else {
            return;
        };

        let mut listener = sqlx::postgres::PgListener::connect_with(&fixture.pool)
            .await
            .unwrap();
        listener.listen("thread_message").await.unwrap();

        let email_message_id = Uuid::new_v4();
        let mut tx = fixture.pool.begin().await.unwrap();
        sqlx::query(
            r#"INSERT INTO email_messages (id, company_id, message_id, content_hash, sender, subject)
               VALUES ($1, $2, $3, '\x00'::bytea, 'sender@example.com', 'Subject')"#,
        )
        .bind(email_message_id)
        .bind(fixture.company_id)
        .bind(format!("<{email_message_id}@example.com>"))
        .execute(&mut *tx)
        .await
        .unwrap();
        let inserted = sqlx::query(
            r#"INSERT INTO thread_messages (
                   id, company_id, channel_id, thread_id, email_message_id,
                   clean_text_body, direction, role
               )
               SELECT $1, company_id, channel_id, id, $2, 'rolled back', 'inbound', 'human'
               FROM threads WHERE id = $3"#,
        )
        .bind(Uuid::new_v4())
        .bind(email_message_id)
        .bind(fixture.thread.id)
        .execute(&mut *tx)
        .await
        .unwrap();
        // Without this the rollback below would prove nothing: the row has to have really been
        // written for its absence afterwards to mean anything.
        assert_eq!(inserted.rows_affected(), 1);

        tx.rollback().await.unwrap();

        // Nothing committed, so nothing may arrive for this thread.
        assert!(
            notification_for(
                &mut listener,
                fixture.thread.id,
                std::time::Duration::from_secs(1)
            )
            .await
            .is_none(),
            "a rolled-back message must not notify"
        );

        CompanyPersistence::delete(&fixture.persistence, fixture.company_id)
            .await
            .unwrap();
    }
}
