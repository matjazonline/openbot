//! The canonical email interface every business channel gets when it is created.
//!
//! Email is a deployment transport, so this binding needs no installation and no linking gesture:
//! a channel is reachable the moment it has an address. Keeping it as a real binding row -- rather
//! than as "the absence of a binding means email" -- is what lets the ingress and delivery paths
//! ask one question, "which interfaces is this channel carrying?", for every transport.
//!
//! It lives here rather than in `channel.rs` so that the SQL for `channel_bindings` stays in one
//! module; `channel.rs` calls [`write_canonical_email_binding`] inside the transaction that
//! creates or updates the channel, which is what makes the two facts atomic.

use uuid::Uuid;

use super::{BindingDb, encoded_provenance};
use crate::{
    adapters::protocols::email::EmailEndpointKey,
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingDeliveryPolicy,
            ChannelBinding,
        },
        value_objects::ChannelSlug,
    },
};

/// The longest `display_label` the column accepts. A channel name has no length limit of its own,
/// so it is truncated here rather than turned into a constraint violation half-way through
/// creating a channel.
const MAX_DISPLAY_LABEL_BYTES: usize = 255;

/// Everything the canonical email binding of one channel is derived from.
///
/// Deliberately no company slug: the endpoint key is namespaced by the company's immutable id, so
/// renaming a company moves no binding and this writer never has to be told about it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CanonicalEmailBinding<'a> {
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub channel_slug: &'a ChannelSlug,
    pub channel_name: &'a str,
    pub created_by: &'a CreationProvenance,
}

/// Create, or move, the channel's one canonical email interface.
///
/// Called from inside the caller's transaction. A channel being created has no binding yet and
/// gets one plus its `linked` audit record; a channel whose primary address changed keeps the same
/// binding and records that the endpoint moved, so the history of an interface survives a rename.
///
/// A company rename is not a case here at all -- see [`EmailEndpointKey`].
///
/// Concurrency is settled by `channel_bindings_canonical_deployment_idx`, not by the `FOR UPDATE`
/// below: two creations racing on the *same* channel have no row to lock, and the second one's
/// insert is rejected by the partial unique index.
pub(crate) async fn write_canonical_email_binding(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    binding: CanonicalEmailBinding<'_>,
) -> AppResult<()> {
    let endpoint_key = EmailEndpointKey::canonical(binding.channel_slug).map_err(|error| {
        AppError::Internal(format!("Channel address is not a usable endpoint: {error}"))
    })?;
    let namespace = EmailEndpointKey::namespace(binding.company_id);
    let display_label = display_label(binding.channel_name, endpoint_key.as_str());

    let existing: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT binding.id, binding.external_endpoint_key
           FROM channel_bindings AS binding
           WHERE binding.company_id = $1
             AND binding.channel_id = $2
             AND binding.transport = 'email'
             AND binding.installation_id IS NULL
             AND binding.status IN ('active', 'paused')
           FOR UPDATE"#,
    )
    .bind(binding.company_id)
    .bind(binding.channel_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(AppError::from)?;

    let created_by = encoded_provenance(binding.created_by)?;
    let snapshot = serde_json::to_value(BindingAccessSnapshot::deployment_endpoint())
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let (row, action) = match existing {
        None => {
            let inserted = sqlx::query_as::<_, BindingDb>(&format!(
                r#"INSERT INTO channel_bindings
                       (id, company_id, channel_id, transport, namespace, external_endpoint_key,
                        display_label, access_policy, delivery_policy, status, created_by,
                        access_snapshot)
                   VALUES ($1, $2, $3, 'email', $4, $5, $6, $7, $8, 'active', $9, $10)
                   RETURNING {}"#,
                super::BINDING_COLUMNS.replace("binding.", "")
            ))
            .bind(Uuid::new_v4())
            .bind(binding.company_id)
            .bind(binding.channel_id)
            .bind(namespace.as_str())
            .bind(endpoint_key.as_str())
            .bind(&display_label)
            // The channel's own principal grants decide who may write to it; the address is not a
            // grant of its own.
            .bind(BindingAccessPolicy::ChannelAcl.as_str())
            // Email carries both agent replies and agent-initiated outreach today, which is what
            // `reply_and_initiate` names.
            .bind(BindingDeliveryPolicy::ReplyAndInitiate.as_str())
            .bind(&created_by)
            .bind(&snapshot)
            .fetch_one(&mut **transaction)
            .await
            .map_err(address_conflict_error)?;
            (inserted, Some(BindingAuditAction::Linked))
        }
        Some((id, previous_key)) => {
            let moved = previous_key != endpoint_key.as_str();
            let updated = sqlx::query_as::<_, BindingDb>(&format!(
                r#"UPDATE channel_bindings AS binding
                   SET external_endpoint_key = $2,
                       display_label = $3,
                       updated_at = CURRENT_TIMESTAMP
                   WHERE binding.id = $1
                   RETURNING {}"#,
                super::BINDING_COLUMNS.replace("binding.", "")
            ))
            .bind(id)
            .bind(endpoint_key.as_str())
            .bind(&display_label)
            .fetch_one(&mut **transaction)
            .await
            .map_err(address_conflict_error)?;
            (
                updated,
                moved.then_some(BindingAuditAction::EndpointChanged),
            )
        }
    };

    // A rename of the channel's display name is cosmetic and leaves no audit record; a change of
    // the address it answers on is not.
    let Some(action) = action else {
        return Ok(());
    };
    let binding_row: ChannelBinding = row.try_into()?;
    super::binding::append_audit_event(transaction, &binding_row, action, None, binding.created_by)
        .await
}

/// A channel's name, bounded to what the column accepts, or its address when the name is unusable.
fn display_label(channel_name: &str, endpoint_key: &str) -> String {
    let trimmed = channel_name.trim();
    if trimmed.is_empty() {
        return endpoint_key.to_string();
    }
    match trimmed
        .char_indices()
        .find(|(index, character)| index + character.len_utf8() > MAX_DISPLAY_LABEL_BYTES)
    {
        Some((index, _)) => trimmed[..index].to_string(),
        None => trimmed.to_string(),
    }
}

/// The address namespace is shared per company, so a collision is user input rather than a fault.
fn address_conflict_error(error: sqlx::Error) -> AppError {
    if error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .as_deref()
        == Some("23505")
    {
        return AppError::BadRequest(
            "That channel address is already answering for another channel in this company.".into(),
        );
    }
    AppError::from(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_display_label_is_bounded_and_never_blank() {
        assert_eq!(display_label("  Support  ", "support@acme"), "Support");
        assert_eq!(display_label("   ", "support@acme"), "support@acme");

        // A multi-byte character straddling the limit is dropped whole rather than split into
        // invalid UTF-8.
        let long = "é".repeat(MAX_DISPLAY_LABEL_BYTES);
        let truncated = display_label(&long, "support@acme");
        assert!(truncated.len() <= MAX_DISPLAY_LABEL_BYTES);
        assert_eq!(truncated.chars().count(), MAX_DISPLAY_LABEL_BYTES / 2);
    }
}
