//! SQLx adapters for provider installations, their credentials, and channel bindings.
//!
//! Split three ways along the three ports in `use_cases::integration`, because the credential
//! reader is the one that must never be reachable from a broad projection: there is no column list
//! in this module that selects `integration_credentials.envelope` alongside anything else.
//!
//! Every statement here names `company_id` explicitly, including the ones where the id would be
//! derivable from a primary key. A binding id arrives from a URL or a provider payload, and a
//! query that trusts it alone is one guessed UUID away from a cross-tenant read.

mod binding;
mod credential;
pub(crate) mod email_binding;
mod installation;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

use std::str::FromStr;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        transport::{
            BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingAuditEvent,
            BindingAuditEventId, BindingAuditMetadata, BindingChangeReason, BindingDeliveryPolicy,
            BindingStatus, ChannelBinding, ChannelBindingId, EndpointNamespace,
            ExternalEndpointKey, ExternalTenantKey, InstallationId, IntegrationInstallation,
        },
    },
};

/// The columns behind [`IntegrationInstallation`]. No credential column appears here, and none
/// may be added: this list feeds every list and detail projection in the product.
const INSTALLATION_COLUMNS: &str = "\
    installation.id, installation.company_id, installation.transport, \
    installation.external_tenant_key, installation.display_name, installation.status, \
    installation.granted_scopes, installation.installed_by, installation.installed_at, \
    installation.updated_by, installation.updated_at, installation.revoked_by, \
    installation.revoked_at";

const BINDING_COLUMNS: &str = "\
    binding.id, binding.company_id, binding.channel_id, binding.installation_id, \
    binding.transport, binding.namespace, binding.external_endpoint_key, binding.display_label, \
    binding.access_policy, binding.delivery_policy, binding.status, binding.disabled_reason, \
    binding.created_by, binding.access_snapshot, binding.created_at, binding.updated_at";

const AUDIT_EVENT_COLUMNS: &str = "\
    event.id, event.company_id, event.binding_id, event.action, event.reason, event.actor, \
    event.metadata, event.created_at";

fn installation_select() -> String {
    format!("SELECT {INSTALLATION_COLUMNS} FROM integration_installations AS installation")
}

fn binding_select() -> String {
    format!("SELECT {BINDING_COLUMNS} FROM channel_bindings AS binding")
}

/// Bindings that are carrying traffic *right now*.
///
/// The installation join is the enforcement point for "a binding on an installed transport needs
/// a usable installation". Revoking an installation therefore stops its bindings in the same
/// instant, with no mass `UPDATE` to run and nothing to go stale if that update half-succeeds.
/// A deployment binding has no installation and passes the join on the `IS NULL` arm.
fn active_binding_select() -> String {
    format!(
        "SELECT {BINDING_COLUMNS} \
         FROM channel_bindings AS binding \
         LEFT JOIN integration_installations AS installation \
             ON installation.company_id = binding.company_id \
            AND installation.id = binding.installation_id \
         WHERE binding.status = 'active' \
           AND (binding.installation_id IS NULL OR installation.status = 'active')"
    )
}

#[derive(sqlx::FromRow)]
struct InstallationDb {
    id: Uuid,
    company_id: Uuid,
    transport: String,
    external_tenant_key: String,
    display_name: String,
    status: String,
    granted_scopes: Vec<String>,
    installed_by: serde_json::Value,
    installed_at: DateTime<Utc>,
    updated_by: serde_json::Value,
    updated_at: DateTime<Utc>,
    revoked_by: Option<serde_json::Value>,
    revoked_at: Option<DateTime<Utc>>,
}

impl TryFrom<InstallationDb> for IntegrationInstallation {
    type Error = AppError;

    fn try_from(row: InstallationDb) -> AppResult<Self> {
        Ok(Self {
            id: InstallationId::new(row.id),
            company_id: row.company_id,
            transport: parsed(
                &row.transport,
                "integration_installations.transport",
                row.id,
            )?,
            external_tenant_key: bounded(
                ExternalTenantKey::parse(row.external_tenant_key),
                "integration_installations.external_tenant_key",
                row.id,
            )?,
            display_name: row.display_name,
            status: parsed(&row.status, "integration_installations.status", row.id)?,
            granted_scopes: row.granted_scopes,
            installed_by: provenance(
                row.installed_by,
                "integration_installations.installed_by",
                row.id,
            )?,
            installed_at: row.installed_at,
            updated_by: provenance(
                row.updated_by,
                "integration_installations.updated_by",
                row.id,
            )?,
            updated_at: row.updated_at,
            revoked_by: row
                .revoked_by
                .map(|value| provenance(value, "integration_installations.revoked_by", row.id))
                .transpose()?,
            revoked_at: row.revoked_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct BindingDb {
    id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
    installation_id: Option<Uuid>,
    transport: String,
    namespace: String,
    external_endpoint_key: String,
    display_label: String,
    access_policy: String,
    delivery_policy: String,
    status: String,
    disabled_reason: Option<String>,
    created_by: serde_json::Value,
    access_snapshot: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<BindingDb> for ChannelBinding {
    type Error = AppError;

    fn try_from(row: BindingDb) -> AppResult<Self> {
        let access_snapshot: BindingAccessSnapshot = decoded(
            row.access_snapshot,
            "channel_bindings.access_snapshot",
            row.id,
        )?;
        if access_snapshot.version() != 1 {
            return Err(AppError::Internal(format!(
                "Unsupported channel_bindings.access_snapshot version for {}",
                row.id
            )));
        }

        Ok(Self {
            id: ChannelBindingId::new(row.id),
            company_id: row.company_id,
            channel_id: row.channel_id,
            installation_id: row.installation_id.map(InstallationId::new),
            transport: parsed(&row.transport, "channel_bindings.transport", row.id)?,
            namespace: bounded(
                EndpointNamespace::parse(row.namespace),
                "channel_bindings.namespace",
                row.id,
            )?,
            external_endpoint_key: bounded(
                ExternalEndpointKey::parse(row.external_endpoint_key),
                "channel_bindings.external_endpoint_key",
                row.id,
            )?,
            display_label: row.display_label,
            access_policy: parsed::<BindingAccessPolicy>(
                &row.access_policy,
                "channel_bindings.access_policy",
                row.id,
            )?,
            delivery_policy: parsed::<BindingDeliveryPolicy>(
                &row.delivery_policy,
                "channel_bindings.delivery_policy",
                row.id,
            )?,
            status: parsed::<BindingStatus>(&row.status, "channel_bindings.status", row.id)?,
            disabled_reason: row
                .disabled_reason
                .as_deref()
                .map(|reason| {
                    parsed::<BindingChangeReason>(
                        reason,
                        "channel_bindings.disabled_reason",
                        row.id,
                    )
                })
                .transpose()?,
            created_by: provenance(row.created_by, "channel_bindings.created_by", row.id)?,
            access_snapshot,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct AuditEventDb {
    id: Uuid,
    company_id: Uuid,
    binding_id: Uuid,
    action: String,
    reason: Option<String>,
    actor: serde_json::Value,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl TryFrom<AuditEventDb> for BindingAuditEvent {
    type Error = AppError;

    fn try_from(row: AuditEventDb) -> AppResult<Self> {
        let metadata: BindingAuditMetadata =
            decoded(row.metadata, "binding_audit_events.metadata", row.id)?;
        if metadata.version != 1 {
            return Err(AppError::Internal(format!(
                "Unsupported binding_audit_events.metadata version for {}",
                row.id
            )));
        }

        Ok(Self {
            id: BindingAuditEventId::new(row.id),
            company_id: row.company_id,
            binding_id: ChannelBindingId::new(row.binding_id),
            action: parsed::<BindingAuditAction>(
                &row.action,
                "binding_audit_events.action",
                row.id,
            )?,
            reason: row
                .reason
                .as_deref()
                .map(|reason| {
                    parsed::<BindingChangeReason>(reason, "binding_audit_events.reason", row.id)
                })
                .transpose()?,
            actor: provenance(row.actor, "binding_audit_events.actor", row.id)?,
            metadata,
            created_at: row.created_at,
        })
    }
}

/// Parse one stored enum spelling, naming the column and the row when it is not one we know.
fn parsed<T>(value: &str, column: &str, id: Uuid) -> AppResult<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    T::from_str(value)
        .map_err(|error| AppError::Internal(format!("Invalid {column} for {id}: {error}")))
}

fn bounded<T, E: std::fmt::Display>(parsed: Result<T, E>, column: &str, id: Uuid) -> AppResult<T> {
    parsed.map_err(|error| AppError::Internal(format!("Invalid {column} for {id}: {error}")))
}

/// Persisted JSON is untrusted input: manual SQL and older application versions can both write a
/// shape the current type does not accept, so this converts fallibly and names the row.
fn decoded<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    column: &str,
    id: Uuid,
) -> AppResult<T> {
    serde_json::from_value(value)
        .map_err(|error| AppError::Internal(format!("Invalid {column} for {id}: {error}")))
}

fn provenance(value: serde_json::Value, column: &str, id: Uuid) -> AppResult<CreationProvenance> {
    decoded(value, column, id)
}

fn encoded_provenance(actor: &CreationProvenance) -> AppResult<serde_json::Value> {
    serde_json::to_value(actor).map_err(|error| AppError::Internal(error.to_string()))
}

/// The audited transport for a value that is both `NULL`-able and enum-checked in SQL.
fn stored_reason(reason: Option<BindingChangeReason>) -> Option<&'static str> {
    reason.map(BindingChangeReason::as_str)
}
