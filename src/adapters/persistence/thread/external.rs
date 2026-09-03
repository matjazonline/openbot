//! Provider keys, and the canonical rows they name.
//!
//! `external_threads` and `external_messages` are the only place a provider's own identifiers are
//! stored. Both are qualified by the binding that carried them, so the same Slack timestamp in two
//! workspaces, or the same Message-ID delivered to two channels, are distinct facts that never
//! collide -- and neither is a column on `threads` or `messages`, which is what lets one canonical
//! thread be bound to email and several Slack conversations at once.

use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        message::{CanonicalMessageId, ExternalMessageCollision},
        transport::{ChannelBindingId, ExternalMessageKey, ExternalThreadKey},
    },
    transport::ExternalCorrelationStore,
};

/// What a provider key already names, and the content it named it with.
#[derive(Clone)]
pub(super) struct ExistingExternalMessage {
    pub message_id: CanonicalMessageId,
    pub content_hash: Vec<u8>,
}

/// The canonical thread a provider conversation already names on this binding, if any.
pub(super) async fn find_external_thread(
    connection: &mut sqlx::PgConnection,
    binding_id: ChannelBindingId,
    key: &ExternalThreadKey,
) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        r#"SELECT mapping.thread_id
           FROM external_threads AS mapping
           WHERE mapping.binding_id = $1 AND mapping.external_thread_key = $2"#,
    )
    .bind(binding_id.as_uuid())
    .bind(key.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)
}

/// The email interface of one channel.
///
/// Email is a deployment transport, so every channel has exactly one of these from the moment it
/// is created -- see `write_canonical_email_binding`. The inbound commit does *not* use this: it is
/// handed the binding each association arrived on, because a channel may carry several. This
/// remains for the producers that have only a channel in hand -- an agent reply, a schedule -- and
/// for those the channel's own email interface is the honest answer.
pub(super) async fn canonical_email_binding(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<ChannelBindingId> {
    let binding: Option<Uuid> = sqlx::query_scalar(
        r#"SELECT binding.id
           FROM channel_bindings AS binding
           WHERE binding.company_id = $1
             AND binding.channel_id = $2
             AND binding.transport = 'email'
             AND binding.installation_id IS NULL
           ORDER BY binding.created_at, binding.id
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;

    binding.map(ChannelBindingId::new).ok_or_else(|| {
        AppError::Internal(format!(
            "Channel {channel_id} has no canonical email binding to correlate mail against"
        ))
    })
}

/// The canonical message a provider key already names on this binding, if any.
pub(super) async fn find_external_message(
    connection: &mut sqlx::PgConnection,
    binding_id: ChannelBindingId,
    key: &ExternalMessageKey,
) -> AppResult<Option<ExistingExternalMessage>> {
    let row: Option<(Uuid, Vec<u8>)> = sqlx::query_as(
        r#"SELECT message.id, message.content_hash
           FROM external_messages AS mapping
           JOIN messages AS message
             ON (message.company_id, message.id) = (mapping.company_id, mapping.message_id)
           WHERE mapping.binding_id = $1 AND mapping.external_message_key = $2"#,
    )
    .bind(binding_id.as_uuid())
    .bind(key.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;

    Ok(
        row.map(|(message_id, content_hash)| ExistingExternalMessage {
            message_id: CanonicalMessageId::new(message_id),
            content_hash,
        }),
    )
}

/// Reuse the canonical message a redelivered provider key names, or refuse the redelivery.
///
/// Identical content is the ordinary case -- a provider retrying an event it never saw acknowledged
/// -- and returns the message already stored. Different content under a key the provider has
/// already used is not a redelivery at all, so it becomes a typed error instead of silently
/// rewriting a message agents have read.
pub(super) fn reuse_or_reject(
    existing: ExistingExternalMessage,
    binding_id: ChannelBindingId,
    key: &ExternalMessageKey,
    content_hash: &[u8],
) -> Result<CanonicalMessageId, ExternalMessageCollision> {
    if existing.content_hash == content_hash {
        return Ok(existing.message_id);
    }
    Err(ExternalMessageCollision {
        binding_id,
        external_message_key: key.clone(),
        existing_message_id: existing.message_id,
    })
}

/// Record that this binding carried this canonical message under this key.
pub(super) async fn insert_external_message(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    binding_id: ChannelBindingId,
    key: &ExternalMessageKey,
    message_id: CanonicalMessageId,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO external_messages (
                id, company_id, binding_id, external_message_key, message_id
           ) VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(binding_id.as_uuid())
    .bind(key.as_str())
    .bind(message_id.as_uuid())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Insert a mapping, or verify that a competing writer stored this exact canonical message.
pub(super) async fn insert_or_verify_external_message(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    binding_id: ChannelBindingId,
    key: &ExternalMessageKey,
    message_id: CanonicalMessageId,
    content_hash: &[u8],
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO external_messages (
                id, company_id, binding_id, external_message_key, message_id
           ) VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (binding_id, external_message_key) DO NOTHING"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(binding_id.as_uuid())
    .bind(key.as_str())
    .bind(message_id.as_uuid())
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;

    let stored = find_external_message(connection, binding_id, key)
        .await?
        .ok_or_else(|| {
            AppError::Internal("External message mapping vanished after insert".into())
        })?;
    if stored.message_id != message_id {
        return Err(ExternalMessageCollision {
            binding_id,
            external_message_key: key.clone(),
            existing_message_id: stored.message_id,
        }
        .into());
    }
    reuse_or_reject(stored, binding_id, key, content_hash)?;
    Ok(())
}

/// Bind a provider conversation to an internal thread, if it is not bound already.
///
/// `DO NOTHING` rather than `DO UPDATE` is the invariant: one provider conversation resolves to
/// exactly one internal thread, forever. A reply that arrives before its root creates the binding,
/// and the root then joins the thread the reply started rather than moving it.
pub(super) async fn upsert_external_thread(
    connection: &mut sqlx::PgConnection,
    company_id: Uuid,
    binding_id: ChannelBindingId,
    key: &ExternalThreadKey,
    thread_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO external_threads (
                id, company_id, binding_id, external_thread_key, thread_id
           ) VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (binding_id, external_thread_key) DO NOTHING"#,
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(binding_id.as_uuid())
    .bind(key.as_str())
    .bind(thread_id)
    .execute(&mut *connection)
    .await
    .map_err(AppError::from)?;
    let stored = find_external_thread(connection, binding_id, key)
        .await?
        .ok_or_else(|| {
            AppError::Internal("External thread mapping vanished after insert".into())
        })?;
    if stored != thread_id {
        return Err(AppError::Conflict(format!(
            "provider thread key '{}' on binding {binding_id} already names thread {stored}",
            key.as_str()
        )));
    }
    Ok(())
}

/// Read-only provider-key resolution, for the decisions that happen before the commit.
///
/// Everything correctness-critical lives inside the inbound commit; this exists so a policy phase
/// can *decide* which conversation an arriving message continues, where a stale answer costs an
/// extra thread rather than a lost or duplicated message.
#[async_trait::async_trait]
impl ExternalCorrelationStore for PostgresPersistence {
    async fn thread_for_thread_keys(
        &self,
        binding_id: ChannelBindingId,
        thread_keys: &[ExternalThreadKey],
    ) -> AppResult<Option<Uuid>> {
        let keys = key_strings(thread_keys, ExternalThreadKey::as_str);
        if keys.is_empty() {
            return Ok(None);
        }
        sqlx::query_scalar(
            r#"SELECT mapping.thread_id
               FROM external_threads AS mapping
               WHERE mapping.binding_id = $1 AND mapping.external_thread_key = ANY($2)
               ORDER BY array_position($2, mapping.external_thread_key)
               LIMIT 1"#,
        )
        .bind(binding_id.as_uuid())
        .bind(&keys)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
    }

    /// The newest association wins among equal-ranked candidates, which is what a reader means by
    /// "the conversation this belongs to".
    async fn thread_for_message_keys(
        &self,
        binding_id: ChannelBindingId,
        message_keys: &[ExternalMessageKey],
    ) -> AppResult<Option<Uuid>> {
        let keys = key_strings(message_keys, ExternalMessageKey::as_str);
        if keys.is_empty() {
            return Ok(None);
        }
        sqlx::query_scalar(
            r#"SELECT association.thread_id
               FROM external_messages AS mapping
               JOIN channel_bindings AS binding
                 ON (binding.company_id, binding.id) = (mapping.company_id, mapping.binding_id)
               JOIN thread_messages AS association
                 ON (association.company_id, association.message_id) =
                    (mapping.company_id, mapping.message_id)
                AND association.channel_id = binding.channel_id
               WHERE mapping.binding_id = $1 AND mapping.external_message_key = ANY($2)
               ORDER BY array_position($2, mapping.external_message_key),
                        association.created_at DESC
               LIMIT 1"#,
        )
        .bind(binding_id.as_uuid())
        .bind(&keys)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)
    }

    async fn message_for_external_key(
        &self,
        binding_id: ChannelBindingId,
        message_key: &ExternalMessageKey,
    ) -> AppResult<Option<CanonicalMessageId>> {
        let stored: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT mapping.message_id
               FROM external_messages AS mapping
               WHERE mapping.binding_id = $1 AND mapping.external_message_key = $2"#,
        )
        .bind(binding_id.as_uuid())
        .bind(message_key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(stored.map(CanonicalMessageId::new))
    }
}

/// Candidate keys as the text `= ANY($2)` compares against, in the order they were offered.
fn key_strings<T>(keys: &[T], as_str: fn(&T) -> &str) -> Vec<String> {
    keys.iter().map(|key| as_str(key).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(hash: &[u8]) -> ExistingExternalMessage {
        ExistingExternalMessage {
            message_id: CanonicalMessageId::random(),
            content_hash: hash.to_vec(),
        }
    }

    #[test]
    fn an_identical_redelivery_returns_the_message_already_stored() {
        let stored = existing(&[7; 32]);
        let expected = stored.message_id;
        let key = ExternalMessageKey::parse("<m@example.com>").unwrap();

        assert_eq!(
            reuse_or_reject(stored, ChannelBindingId::random(), &key, &[7; 32]),
            Ok(expected)
        );
    }

    #[test]
    fn changed_content_under_a_used_key_is_a_collision_naming_what_it_hit() {
        let stored = existing(&[7; 32]);
        let expected = stored.message_id;
        let binding_id = ChannelBindingId::random();
        let key = ExternalMessageKey::parse("<m@example.com>").unwrap();

        assert_eq!(
            reuse_or_reject(stored, binding_id, &key, &[9; 32]),
            Err(ExternalMessageCollision {
                binding_id,
                external_message_key: key,
                existing_message_id: expected,
            })
        );
    }
}
