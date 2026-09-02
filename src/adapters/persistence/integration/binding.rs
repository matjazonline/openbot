//! Channel bindings and their append-only history.
//!
//! Creating a binding and recording that it was created are one transaction, and so are changing
//! its status and recording that change. Anything less would let a link exist with no record of
//! who made it — which for a private provider conversation is the record of a read grant to
//! everyone in that conversation.

use async_trait::async_trait;
use uuid::Uuid;

use super::{
    AuditEventDb, BindingDb, active_binding_select, binding_select, encoded_provenance,
    stored_reason,
};
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        transport::{
            BindingAuditAction, BindingAuditEvent, BindingChangeReason, ChannelBinding,
            ChannelBindingId,
        },
    },
    use_cases::integration::{
        BindingStatusChange, BindingWrite, ChannelBindingPersistence, InboundEndpoint,
        MAX_BINDING_AUDIT_EVENTS,
    },
};

/// The Postgres unique-violation class. A binding write hits it when two channels try to claim the
/// same external endpoint, which is ordinary contention rather than a fault.
const UNIQUE_VIOLATION: &str = "23505";

#[async_trait]
impl ChannelBindingPersistence for PostgresPersistence {
    async fn create_binding(&self, write: BindingWrite) -> AppResult<ChannelBinding> {
        write.validate()?;
        let created_by = encoded_provenance(&write.created_by)?;
        let access_snapshot = serde_json::to_value(&write.access_snapshot)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;

        let query = format!(
            r#"INSERT INTO channel_bindings
                   (id, company_id, channel_id, installation_id, transport, namespace,
                    external_endpoint_key, display_label, access_policy, delivery_policy,
                    status, created_by, access_snapshot)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'active', $11, $12)
               RETURNING {}"#,
            binding_columns_for_returning()
        );
        let binding: ChannelBinding = sqlx::query_as::<_, BindingDb>(&query)
            .bind(Uuid::new_v4())
            .bind(write.company_id)
            .bind(write.channel_id)
            .bind(write.installation_id.map(Into::<Uuid>::into))
            .bind(write.transport.as_str())
            .bind(write.namespace.as_str())
            .bind(write.external_endpoint_key.as_str())
            .bind(&write.display_label)
            .bind(write.access_policy.as_str())
            .bind(write.delivery_policy.as_str())
            .bind(&created_by)
            .bind(&access_snapshot)
            .fetch_one(&mut *transaction)
            .await
            .map_err(endpoint_conflict_error)?
            .try_into()?;

        append_audit_event(
            &mut transaction,
            &binding,
            BindingAuditAction::Linked,
            None,
            &write.created_by,
        )
        .await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(binding)
    }

    async fn active_bindings_for_channel(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Vec<ChannelBinding>> {
        let query = format!(
            "{} AND binding.company_id = $1 AND binding.channel_id = $2 \
             ORDER BY binding.transport, binding.created_at, binding.id",
            active_binding_select()
        );
        sqlx::query_as::<_, BindingDb>(&query)
            .bind(company_id)
            .bind(channel_id)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    /// The exact endpoint lookup an inbound event routes through.
    ///
    /// `installation_id` participates as a value *and* as a NULL test, because SQL equality on a
    /// NULL matches nothing: an email endpoint would otherwise be unroutable, and — worse — a
    /// caller could omit the installation and have a Slack endpoint resolve against the deployment
    /// namespace.
    async fn find_active_binding_by_endpoint(
        &self,
        endpoint: &InboundEndpoint,
    ) -> AppResult<Option<ChannelBinding>> {
        let query = format!(
            "{} AND binding.transport = $1 \
               AND binding.namespace = $2 \
               AND binding.external_endpoint_key = $3 \
               AND binding.installation_id IS NOT DISTINCT FROM $4",
            active_binding_select()
        );
        sqlx::query_as::<_, BindingDb>(&query)
            .bind(endpoint.transport.as_str())
            .bind(endpoint.namespace.as_str())
            .bind(endpoint.external_endpoint_key.as_str())
            .bind(endpoint.installation_id.map(Into::<Uuid>::into))
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn list_bindings_for_company(&self, company_id: Uuid) -> AppResult<Vec<ChannelBinding>> {
        let query = format!(
            "{} WHERE binding.company_id = $1 \
             ORDER BY binding.created_at DESC, binding.id DESC",
            binding_select()
        );
        sqlx::query_as::<_, BindingDb>(&query)
            .bind(company_id)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    async fn get_binding(
        &self,
        company_id: Uuid,
        binding_id: ChannelBindingId,
    ) -> AppResult<Option<ChannelBinding>> {
        let query = format!(
            "{} WHERE binding.company_id = $1 AND binding.id = $2",
            binding_select()
        );
        sqlx::query_as::<_, BindingDb>(&query)
            .bind(company_id)
            .bind(binding_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn set_binding_status(&self, change: BindingStatusChange) -> AppResult<ChannelBinding> {
        change.validate()?;
        let action = change.audit_action();
        let mut transaction = self.pool.begin().await.map_err(AppError::from)?;

        let query = format!(
            r#"UPDATE channel_bindings AS binding
               SET status = $3,
                   disabled_reason = $4,
                   updated_at = CURRENT_TIMESTAMP
               WHERE binding.company_id = $1 AND binding.id = $2
               RETURNING {}"#,
            binding_columns_for_returning()
        );
        let binding: ChannelBinding = sqlx::query_as::<_, BindingDb>(&query)
            .bind(change.company_id)
            .bind(change.binding_id.as_uuid())
            .bind(change.status.as_str())
            .bind(stored_reason(change.reason))
            .fetch_optional(&mut *transaction)
            .await
            .map_err(endpoint_conflict_error)?
            .ok_or_else(|| AppError::NotFound("That channel binding was not found.".into()))?
            .try_into()?;

        append_audit_event(
            &mut transaction,
            &binding,
            action,
            change.reason,
            &change.actor,
        )
        .await?;
        transaction.commit().await.map_err(AppError::from)?;
        Ok(binding)
    }

    async fn list_binding_audit_events(
        &self,
        company_id: Uuid,
        binding_id: ChannelBindingId,
        limit: i64,
    ) -> AppResult<Vec<BindingAuditEvent>> {
        let limit = limit.clamp(1, MAX_BINDING_AUDIT_EVENTS);
        let query = format!(
            r#"SELECT {}
               FROM binding_audit_events AS event
               WHERE event.company_id = $1 AND event.binding_id = $2
               ORDER BY event.created_at DESC, event.id DESC
               LIMIT $3"#,
            super::AUDIT_EVENT_COLUMNS
        );
        sqlx::query_as::<_, AuditEventDb>(&query)
            .bind(company_id)
            .bind(binding_id.as_uuid())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }
}

/// Append one lifecycle record, built from the binding as it now stands.
///
/// The metadata comes from [`ChannelBinding::audit_metadata`] rather than from the caller, so no
/// call site can decide to put something else in it.
pub(super) async fn append_audit_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: &ChannelBinding,
    action: BindingAuditAction,
    reason: Option<BindingChangeReason>,
    actor: &CreationProvenance,
) -> AppResult<()> {
    let metadata = serde_json::to_value(binding.audit_metadata())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query(
        r#"INSERT INTO binding_audit_events
               (id, company_id, binding_id, action, reason, actor, metadata)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(Uuid::new_v4())
    .bind(binding.company_id)
    .bind(binding.id.as_uuid())
    .bind(action.as_str())
    .bind(stored_reason(reason))
    .bind(encoded_provenance(actor)?)
    .bind(metadata)
    .execute(&mut **transaction)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

/// Two channels claiming one conversation is contention, not a fault, so it reads back as a
/// conflict a manager can act on.
fn endpoint_conflict_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .as_deref()
        == Some(UNIQUE_VIOLATION)
    {
        return AppError::Conflict(
            "That endpoint is already linked to another channel. Disable the existing binding \
             first."
                .into(),
        );
    }
    AppError::from(error)
}

/// `RETURNING` has no `FROM` to alias, so the shared column list loses its qualifier here.
fn binding_columns_for_returning() -> String {
    super::BINDING_COLUMNS.replace("binding.", "")
}
