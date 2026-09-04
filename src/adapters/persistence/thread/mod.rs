//! Threads, and the canonical messages associated with them.
//!
//! The module is split along the boundaries the storage model has: [`message`] owns the canonical
//! payload and its thread associations, [`external`] owns the provider keys those rows are reached
//! by, and [`email_metadata`] owns the headers only mail has. This file owns threads themselves and
//! the [`ThreadPersistence`] port that ties the three together.

mod email_metadata;
mod external;
mod inbound;
mod message;
mod views;

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "read_tests.rs"]
mod read_tests;

#[cfg(test)]
#[path = "inbound_tests.rs"]
mod inbound_tests;

#[cfg(test)]
#[path = "view_tests.rs"]
mod view_tests;

pub(crate) use message::{associate_message_on, insert_message_on};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};
use uuid::Uuid;

use crate::adapters::persistence::delivery::enqueue::insert_delivery_on;
use crate::adapters::persistence::participant::resolve_or_create_external_identity_on;
use crate::adapters::protocols::email::EmailIdentity;
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        cursor::{MessageCursor, ThreadCursor},
        message::{CanonicalMessageId, Message, MessageRole},
        message_view::{
            AgentHistoryMessage, EmailReplyContext, MessageAuditView, ThreadMessageView,
        },
        participant::{IdentityClaimMetadata, IdentityProvenance, ThreadPrincipalRole},
        thread::{Thread, ThreadParticipantProjection},
        transport::{PrincipalId, TransportKind},
        value_objects::{EmailAddress, MessageId, ThreadIndex},
    },
    transport::{DeliveryCreation, NewDelivery, ThreadPrincipalIntent},
    use_cases::{
        participant::IdentityObservation,
        thread::{MessageWrite, ThreadPersistence},
    },
};

use message::{MESSAGE_SELECT, MessageDb};

#[derive(sqlx::FromRow, Debug)]
pub struct ThreadDb {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub participant_principal_ids: Vec<Uuid>,
    pub participant_identities: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct ThreadIdentityRow {
    transport: String,
    namespace: String,
    subject: String,
}

impl TryFrom<ThreadDb> for Thread {
    type Error = AppError;

    fn try_from(db: ThreadDb) -> AppResult<Self> {
        let rows: Vec<ThreadIdentityRow> = serde_json::from_value(db.participant_identities)
            .map_err(|error| {
                AppError::Internal(format!("Stored thread identities are invalid: {error}"))
            })?;
        let identities = rows
            .into_iter()
            .map(|row| {
                Ok(crate::entities::transport::QualifiedIdentity::new(
                    TransportKind::from_str(&row.transport).map_err(|error| {
                        AppError::Internal(format!("Stored thread identity is invalid: {error}"))
                    })?,
                    crate::entities::transport::IdentityNamespace::parse(row.namespace).map_err(
                        |error| {
                            AppError::Internal(format!(
                                "Stored thread identity is invalid: {error}"
                            ))
                        },
                    )?,
                    crate::entities::transport::IdentitySubject::parse(row.subject).map_err(
                        |error| {
                            AppError::Internal(format!(
                                "Stored thread identity is invalid: {error}"
                            ))
                        },
                    )?,
                ))
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(Thread {
            id: db.id,
            channel_id: db.channel_id,
            subject: db.subject,
            participant_principal_ids: db
                .participant_principal_ids
                .into_iter()
                .map(PrincipalId::new)
                .collect(),
            participant_projection: ThreadParticipantProjection { identities },
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

const THREAD_SELECT: &str = r#"
    SELECT thread.id, thread.channel_id, thread.subject,
           COALESCE(
               (SELECT array_agg(DISTINCT thread_principal.principal_id)
                  FROM thread_principals AS thread_principal
                 WHERE thread_principal.company_id = thread.company_id
                   AND thread_principal.thread_id = thread.id),
               ARRAY[]::uuid[]
           ) AS participant_principal_ids,
           COALESCE((
               SELECT jsonb_agg(jsonb_build_object(
                          'transport', identity.transport,
                          'namespace', identity.namespace,
                          'subject', identity.subject)
                      ORDER BY identity.transport, identity.namespace, identity.subject)
                 FROM (
                     SELECT DISTINCT handle.transport, handle.namespace, handle.subject
                       FROM thread_principals AS thread_principal
                       JOIN participant_identities AS handle
                         ON (handle.company_id, handle.principal_id) =
                            (thread_principal.company_id, thread_principal.principal_id)
                      WHERE thread_principal.company_id = thread.company_id
                        AND thread_principal.thread_id = thread.id
                        AND handle.status <> 'disabled'
                 ) AS identity
           ), '[]'::jsonb) AS participant_identities,
           thread.created_at, thread.updated_at
      FROM threads AS thread
"#;

async fn load_thread(pool: &PgPool, id: Uuid) -> AppResult<Option<Thread>> {
    let query = format!("{THREAD_SELECT} WHERE thread.id = $1");
    let db = sqlx::query_as::<_, ThreadDb>(&query)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?;
    db.map(Thread::try_from).transpose()
}

async fn load_message(pool: &PgPool, association_id: Uuid) -> AppResult<Message> {
    let query = format!("{MESSAGE_SELECT} WHERE association.id = $1");
    sqlx::query_as::<_, MessageDb>(&query)
        .bind(association_id)
        .fetch_one(pool)
        .await
        .map_err(AppError::from)?
        .try_into()
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

async fn insert_thread_email_participants(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    participant_emails: &[EmailAddress],
) -> AppResult<()> {
    let participants = normalized_participants(participant_emails);
    let mut intents = Vec::with_capacity(participants.len());
    for email in participants {
        let identity = EmailIdentity::parse(EmailAddress::from(email))
            .map(EmailIdentity::qualify_default)
            .map_err(|error| {
                AppError::BadRequest(format!("Invalid thread participant: {error}"))
            })?;
        intents.push(ThreadPrincipalIntent::new(
            identity,
            ThreadPrincipalRole::Participant,
        ));
    }
    insert_thread_principals(transaction, company_id, channel_id, thread_id, &intents).await
}

/// Resolve transport-qualified handles and record their explicitly stated thread roles.
pub(super) async fn insert_thread_principals(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    intents: &[ThreadPrincipalIntent],
) -> AppResult<()> {
    let mut seen = HashSet::new();
    for intent in intents {
        if !seen.insert((intent.identity.clone(), intent.role)) {
            continue;
        }
        let provenance = IdentityProvenance::TransportIngress;
        let resolved = resolve_or_create_external_identity_on(
            transaction,
            company_id,
            IdentityObservation {
                identity: intent.identity.clone(),
                display_label: None,
                claim_metadata: IdentityClaimMetadata::observation(),
                provenance,
            },
        )
        .await?;
        sqlx::query(
            r#"INSERT INTO thread_principals
                   (company_id, channel_id, thread_id, principal_id, role)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(thread_id)
        .bind(resolved.principal.id.as_uuid())
        .bind(intent.role.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(AppError::from)?;
    }
    Ok(())
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
        let scope: Option<(Uuid, Uuid)> = sqlx::query_as(
            r#"INSERT INTO threads (id, company_id, channel_id, subject)
               SELECT $1, company_id, id, $3 FROM channels WHERE id = $2
               RETURNING company_id, channel_id"#,
        )
        .bind(id)
        .bind(channel_id)
        .bind(subject)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;

        let Some((company_id, channel_id)) = scope else {
            return Err(AppError::Internal("Channel not found".into()));
        };

        insert_thread_email_participants(&mut tx, company_id, channel_id, id, participant_emails)
            .await?;

        tx.commit().await.map_err(AppError::from)?;
        load_thread(&self.pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Created thread was not found".into()))
    }

    async fn ensure_schedule_run_thread(
        &self,
        run_id: Uuid,
        channel_id: Uuid,
        subject: &str,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let run: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT thread_id FROM schedule_runs WHERE id = $1 FOR UPDATE")
                .bind(run_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::from)?;
        let (existing,) =
            run.ok_or_else(|| AppError::Internal("Durable schedule run was not found".into()))?;

        if let Some(thread_id) = existing {
            tx.commit().await.map_err(AppError::from)?;
            return load_thread(&self.pool, thread_id).await?.ok_or_else(|| {
                AppError::Internal("Durable schedule run references a missing thread".into())
            });
        }

        let id = Uuid::new_v4();
        let scope: Option<(Uuid, Uuid)> = sqlx::query_as(
            r#"INSERT INTO threads (id, company_id, channel_id, subject)
               SELECT $1, company_id, id, $3 FROM channels WHERE id = $2
               RETURNING company_id, channel_id"#,
        )
        .bind(id)
        .bind(channel_id)
        .bind(subject)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let Some((company_id, channel_id)) = scope else {
            return Err(AppError::Internal("Channel not found".into()));
        };
        insert_thread_email_participants(&mut tx, company_id, channel_id, id, participant_emails)
            .await?;
        sqlx::query(
            "UPDATE schedule_runs SET thread_id = $2, updated_at = CURRENT_TIMESTAMP WHERE id = $1",
        )
        .bind(run_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
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
                   WHERE thread.channel_id = $1
                     AND (thread.updated_at, thread.id) < ($2, $3)
                   ORDER BY thread.updated_at DESC, thread.id DESC
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
                   WHERE thread.channel_id = $1
                   ORDER BY thread.updated_at DESC, thread.id DESC
                   LIMIT $2"#
            );
            sqlx::query_as::<_, ThreadDb>(&query)
                .bind(channel_id)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await
        }
        .map_err(AppError::from)?;

        db.into_iter().map(Thread::try_from).collect()
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
            r#"SELECT DISTINCT ON (association.thread_id) association.thread_id, message.role
               FROM thread_messages AS association
               JOIN messages AS message
                 ON (message.company_id, message.id) =
                    (association.company_id, association.message_id)
               WHERE association.thread_id = ANY($1)
               ORDER BY association.thread_id, association.created_at DESC, association.id DESC"#,
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
               WHERE thread.channel_id = $1
                 AND ($2::timestamptz IS NULL
                      OR (thread.updated_at, thread.id) > ($2, $3))
               ORDER BY thread.updated_at ASC, thread.id ASC
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

        db.into_iter().map(Thread::try_from).collect()
    }

    async fn update_thread_participants(
        &self,
        id: Uuid,
        participant_emails: &[EmailAddress],
    ) -> AppResult<Thread> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let scope: Option<(Uuid, Uuid)> =
            sqlx::query_as("SELECT company_id, channel_id FROM threads WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(AppError::from)?;
        let Some((company_id, channel_id)) = scope else {
            return Err(AppError::Internal("Thread not found".into()));
        };
        insert_thread_email_participants(&mut tx, company_id, channel_id, id, participant_emails)
            .await?;

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

    async fn find_thread_by_thread_index(
        &self,
        channel_id: Uuid,
        thread_index: &ThreadIndex,
    ) -> AppResult<Option<Thread>> {
        let candidates = match thread_index.ancestor_chain() {
            Ok(candidates) => candidates,
            Err(_) => return Ok(None),
        };
        let candidate_values: Vec<&str> = candidates.iter().map(ThreadIndex::as_str).collect();

        let query = format!(
            r#"{THREAD_SELECT}
               JOIN thread_messages AS association ON association.thread_id = thread.id
               JOIN email_message_metadata AS email
                 ON (email.company_id, email.message_id) =
                    (association.company_id, association.message_id)
               WHERE thread.channel_id = $1
                 AND email.thread_index IS NOT NULL
                 AND email.thread_index = ANY($2)
               ORDER BY length(email.thread_index) DESC,
                        association.created_at DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(channel_id)
            .bind(&candidate_values)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(Thread::try_from).transpose()
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

    async fn get_thread_message(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            "{MESSAGE_SELECT} WHERE association.thread_id = $1 AND association.message_id = $2"
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(message_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(Message::try_from).transpose()
    }

    async fn find_thread_for_message(
        &self,
        channel_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<Thread>> {
        let query = format!(
            r#"{THREAD_SELECT}
               JOIN thread_messages AS association ON association.thread_id = thread.id
               WHERE association.channel_id = $1 AND association.message_id = $2"#
        );
        let db = sqlx::query_as::<_, ThreadDb>(&query)
            .bind(channel_id)
            .bind(message_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(Thread::try_from).transpose()
    }

    async fn get_message_protocol_extension(
        &self,
        company_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<crate::transport::ProtocolExtension> {
        Ok(
            email_metadata::load_email_metadata(&self.pool, company_id, message_id.as_uuid())
                .await?
                .map_or_else(
                    crate::transport::ProtocolExtension::none,
                    crate::transport::ProtocolExtension::email,
                ),
        )
    }

    async fn create_message(&self, write: &MessageWrite) -> AppResult<Message> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let inserted = insert_message_on(&mut tx, write).await?;
        tx.commit().await.map_err(AppError::from)?;
        load_message(&self.pool, inserted.association_id).await
    }

    async fn create_message_with_deliveries(
        &self,
        write: &MessageWrite,
        deliveries: &[NewDelivery],
    ) -> AppResult<(Message, Vec<DeliveryCreation>)> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let inserted = insert_message_on(&mut tx, write).await?;
        let mut created = Vec::with_capacity(deliveries.len());
        for delivery in deliveries {
            if delivery.message_id != inserted.canonical_id {
                // A delivery naming a different message than the one just written would either
                // fail its foreign key or, worse, attach to some other message entirely. Refused
                // here because the caller minted both ids and can only have crossed them.
                return Err(AppError::Internal(format!(
                    "Delivery '{}' names message {} but this transaction stored {}",
                    delivery.idempotency_key, delivery.message_id, inserted.canonical_id
                )));
            }
            created.push(insert_delivery_on(&mut tx, delivery).await?);
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok((
            load_message(&self.pool, inserted.association_id).await?,
            created,
        ))
    }

    async fn find_outbound_reply_after(
        &self,
        thread_id: Uuid,
        answering: CanonicalMessageId,
    ) -> AppResult<Option<Message>> {
        let query = format!(
            r#"{MESSAGE_SELECT}
               WHERE association.thread_id = $1
                 AND message.direction = 'outbound'
                 AND (association.created_at, association.id) >
                     (SELECT answered.created_at, answered.id
                        FROM thread_messages AS answered
                       WHERE answered.thread_id = $1 AND answered.message_id = $2)
                 -- The agent asking a third party something is not the agent answering this
                 -- turn. Recognised by the canonical relation the outreach recorded, so a
                 -- transport with no message header of its own is excluded on the same terms.
                 AND NOT EXISTS (
                     SELECT 1 FROM task_outreach_targets AS target
                     WHERE target.request_message_id = message.id
                 )
               ORDER BY association.created_at DESC, association.id DESC
               LIMIT 1"#
        );
        let db = sqlx::query_as::<_, MessageDb>(&query)
            .bind(thread_id)
            .bind(answering.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;
        db.map(TryInto::try_into).transpose()
    }

    async fn associate_message(
        &self,
        thread_id: Uuid,
        message: CanonicalMessageId,
    ) -> AppResult<Message> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        let association_id = associate_message_on(&mut tx, thread_id, message).await?;
        tx.commit().await.map_err(AppError::from)?;
        load_message(&self.pool, association_id).await
    }

    async fn list_thread_message_views(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Vec<ThreadMessageView>> {
        views::list_thread_messages(&self.pool, thread_id).await
    }

    async fn list_thread_message_views_after(
        &self,
        thread_id: Uuid,
        after: Option<MessageCursor>,
        limit: usize,
    ) -> AppResult<Vec<ThreadMessageView>> {
        views::list_thread_messages_after(&self.pool, thread_id, after, limit).await
    }

    async fn get_thread_message_view(
        &self,
        thread_id: Uuid,
        message_id: CanonicalMessageId,
    ) -> AppResult<Option<ThreadMessageView>> {
        views::get_thread_message(&self.pool, thread_id, message_id).await
    }

    async fn list_agent_history(&self, thread_id: Uuid) -> AppResult<Vec<AgentHistoryMessage>> {
        views::list_agent_history(&self.pool, thread_id).await
    }

    async fn latest_email_reply_context(
        &self,
        thread_id: Uuid,
    ) -> AppResult<Option<EmailReplyContext>> {
        views::latest_email_reply_context(&self.pool, thread_id).await
    }

    async fn latest_thread_rfc_message_id(&self, thread_id: Uuid) -> AppResult<Option<MessageId>> {
        views::latest_thread_rfc_message_id(&self.pool, thread_id).await
    }

    async fn get_message_audit(
        &self,
        company_id: Uuid,
        association_id: Uuid,
    ) -> AppResult<Option<MessageAuditView>> {
        views::get_message_audit(&self.pool, company_id, association_id).await
    }
}
