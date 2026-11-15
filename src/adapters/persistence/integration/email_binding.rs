//! Canonical email interfaces for desk and principal addresses.
//! The caller owns the transaction: address, endpoint and audit changes commit together.
use sqlx::PgConnection;
use uuid::Uuid;

use super::{BindingDb, encoded_provenance};
use crate::{
    adapters::protocols::email::EmailEndpointKey,
    app_error::{AppError, AppResult},
    entities::{
        address::AddressTarget,
        creation::CreationProvenance,
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingDeliveryPolicy,
            ChannelBinding, EndpointNamespace, PrincipalId,
        },
        value_objects::ChannelSlug,
    },
};

const MAX_DISPLAY_LABEL_BYTES: usize = 255;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CanonicalEmailBinding<'a> {
    pub company_id: Uuid,
    pub target: AddressTarget,
    pub slug: &'a ChannelSlug,
    pub name: &'a str,
    pub created_by: &'a CreationProvenance,
}

struct CanonicalEndpoint {
    key: EmailEndpointKey,
    namespace: EndpointNamespace,
    label: String,
}

fn endpoint_of(binding: &CanonicalEmailBinding<'_>) -> AppResult<CanonicalEndpoint> {
    let key = EmailEndpointKey::canonical(binding.slug).map_err(|error| {
        AppError::Internal(format!("Address is not a usable endpoint: {error}"))
    })?;
    Ok(CanonicalEndpoint {
        label: display_label(binding.name, key.as_str()),
        key,
        namespace: EmailEndpointKey::namespace(binding.company_id),
    })
}

/// Keep the same interface across principal retirement and restoration. Desk interfaces retain
/// their existing live-set uniqueness; a principal has one canonical email interface for life.
pub(crate) async fn write_canonical_email_binding(
    db: &mut PgConnection,
    binding: CanonicalEmailBinding<'_>,
) -> AppResult<()> {
    let endpoint = endpoint_of(&binding)?;
    let previous = existing_on(db, binding.company_id, binding.target).await?;
    let (row, actions) = match previous {
        None => (
            insert_on(db, &binding, &endpoint).await?,
            vec![BindingAuditAction::Linked],
        ),
        Some(previous) => update_on(db, &binding, &endpoint, previous).await?,
    };
    let row: ChannelBinding = row.try_into()?;
    for action in actions {
        super::binding::append_audit_event(db, &row, action, None, binding.created_by).await?;
    }
    Ok(())
}

async fn existing_on(
    db: &mut PgConnection,
    company: Uuid,
    target: AddressTarget,
) -> AppResult<Option<BindingDb>> {
    sqlx::query_as(&format!(
        "SELECT {} FROM channel_bindings AS binding WHERE binding.company_id = $1 \
         AND binding.channel_id IS NOT DISTINCT FROM $2 \
         AND binding.principal_id IS NOT DISTINCT FROM $3 \
         AND binding.transport = 'email' AND binding.installation_id IS NULL \
         AND ($3::uuid IS NOT NULL OR binding.status IN ('active', 'paused')) FOR UPDATE",
        super::BINDING_COLUMNS,
    ))
    .bind(company)
    .bind(target.desk_id())
    .bind(target.principal_id().map(PrincipalId::as_uuid))
    .fetch_optional(db)
    .await
    .map_err(AppError::from)
}

async fn insert_on(
    db: &mut PgConnection,
    binding: &CanonicalEmailBinding<'_>,
    endpoint: &CanonicalEndpoint,
) -> AppResult<BindingDb> {
    let snapshot = serde_json::to_value(BindingAccessSnapshot::deployment_endpoint())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    sqlx::query_as(&format!(
        "INSERT INTO channel_bindings \
         (id, company_id, channel_id, principal_id, transport, namespace, external_endpoint_key, \
          display_label, access_policy, delivery_policy, status, created_by, access_snapshot) \
         VALUES ($1,$2,$3,$4,'email',$5,$6,$7,$8,$9,'active',$10,$11) RETURNING {}",
        super::BINDING_COLUMNS.replace("binding.", ""),
    ))
    .bind(Uuid::new_v4())
    .bind(binding.company_id)
    .bind(binding.target.desk_id())
    .bind(binding.target.principal_id().map(PrincipalId::as_uuid))
    .bind(endpoint.namespace.as_str())
    .bind(endpoint.key.as_str())
    .bind(&endpoint.label)
    .bind(BindingAccessPolicy::TargetAcl.as_str())
    .bind(BindingDeliveryPolicy::ReplyAndInitiate.as_str())
    .bind(encoded_provenance(binding.created_by)?)
    .bind(snapshot)
    .fetch_one(db)
    .await
    .map_err(address_conflict_error)
}

async fn update_on(
    db: &mut PgConnection,
    binding: &CanonicalEmailBinding<'_>,
    endpoint: &CanonicalEndpoint,
    previous: BindingDb,
) -> AppResult<(BindingDb, Vec<BindingAuditAction>)> {
    let restored = matches!(binding.target, AddressTarget::Principal(_))
        && previous.status == "orphaned"
        && previous.disabled_reason.as_deref() == Some("principal_retired");
    let mut actions = Vec::with_capacity(2);
    if previous.external_endpoint_key != endpoint.key.as_str() {
        actions.push(BindingAuditAction::EndpointChanged);
    }
    if restored {
        actions.push(BindingAuditAction::Enabled);
    }
    let row = sqlx::query_as(&format!(
        "UPDATE channel_bindings AS binding SET external_endpoint_key=$3, display_label=$4, \
         status=CASE WHEN $5 THEN 'active' ELSE binding.status END, \
         disabled_reason=CASE WHEN $5 THEN NULL ELSE binding.disabled_reason END, \
         updated_at=CURRENT_TIMESTAMP WHERE binding.company_id=$1 AND binding.id=$2 RETURNING {}",
        super::BINDING_COLUMNS.replace("binding.", ""),
    ))
    .bind(binding.company_id)
    .bind(previous.id)
    .bind(endpoint.key.as_str())
    .bind(&endpoint.label)
    .bind(restored)
    .fetch_one(db)
    .await
    .map_err(address_conflict_error)?;
    Ok((row, actions))
}

/// Automatic endpoint synchronization, inside the transaction that registered or renamed the
/// principal's address. The transport records this automatic action with system provenance.
pub(crate) async fn sync_principal_email_binding_on(
    db: &mut PgConnection,
    company: Uuid,
    principal: PrincipalId,
) -> AppResult<()> {
    #[derive(sqlx::FromRow)]
    struct AddressRow {
        slug: String,
        display_label: String,
    }
    let address: AddressRow = sqlx::query_as(
        "SELECT address.slug::text AS slug, principal.display_label \
         FROM addresses AS address JOIN principals AS principal \
           ON principal.company_id=address.company_id AND principal.id=address.principal_id \
         WHERE address.company_id=$1 AND address.principal_id=$2",
    )
    .bind(company)
    .bind(principal.as_uuid())
    .fetch_one(&mut *db)
    .await?;
    let slug = ChannelSlug::from(address.slug);
    write_canonical_email_binding(
        db,
        CanonicalEmailBinding {
            company_id: company,
            target: AddressTarget::Principal(principal),
            slug: &slug,
            name: &address.display_label,
            created_by: &CreationProvenance::system(),
        },
    )
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
            "That desk address is already answering for another desk in this company.".into(),
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
