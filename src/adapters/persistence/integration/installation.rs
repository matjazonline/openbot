//! Provider accounts a company has installed.

use async_trait::async_trait;
use uuid::Uuid;

use super::{InstallationDb, encoded_provenance, installation_select};
use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    entities::transport::{
        ExternalTenantKey, InstallationId, InstallationStatus, IntegrationInstallation,
        TransportKind,
    },
    use_cases::integration::{
        InstallationPersistence, InstallationStatusChange, InstallationWrite,
    },
};

#[async_trait]
impl InstallationPersistence for PostgresPersistence {
    /// Install, or refresh the installation this company already holds for the same external
    /// account.
    ///
    /// The conflict target is `(transport, external_tenant_key)` — the deployment-wide key — and
    /// the `DO UPDATE` re-asserts `company_id` in its `WHERE`. So a workspace re-installing into
    /// the company that already owns it refreshes scopes, name and status, while a workspace
    /// installing into a *second* company updates nothing and returns no row. That is the
    /// difference between a re-install and one tenant quietly adopting another's workspace, and it
    /// is settled by the database rather than by a read-then-write race.
    async fn install(&self, write: InstallationWrite) -> AppResult<IntegrationInstallation> {
        let actor = encoded_provenance(&write.actor)?;
        let query = format!(
            r#"INSERT INTO integration_installations
                   (id, company_id, transport, external_tenant_key, display_name, status,
                    granted_scopes, installed_by, updated_by)
               VALUES ($1, $2, $3, $4, $5, 'active', $6, $7, $7)
               ON CONFLICT (transport, external_tenant_key) DO UPDATE
                   SET display_name = EXCLUDED.display_name,
                       granted_scopes = EXCLUDED.granted_scopes,
                       status = 'active',
                       updated_by = EXCLUDED.updated_by,
                       updated_at = CURRENT_TIMESTAMP,
                       revoked_by = NULL,
                       revoked_at = NULL
                   WHERE integration_installations.company_id = EXCLUDED.company_id
               RETURNING {}"#,
            installation_columns_for_returning()
        );

        let installed = sqlx::query_as::<_, InstallationDb>(&query)
            .bind(Uuid::new_v4())
            .bind(write.company_id)
            .bind(write.transport.as_str())
            .bind(write.external_tenant_key.as_str())
            .bind(&write.display_name)
            .bind(&write.granted_scopes)
            .bind(&actor)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?;

        match installed {
            Some(row) => row.try_into(),
            None => Err(AppError::Conflict(format!(
                "That {} workspace is already installed by another company.",
                write.transport
            ))),
        }
    }

    async fn get_installation(
        &self,
        company_id: Uuid,
        installation_id: InstallationId,
    ) -> AppResult<Option<IntegrationInstallation>> {
        let query = format!(
            "{} WHERE installation.company_id = $1 AND installation.id = $2",
            installation_select()
        );
        sqlx::query_as::<_, InstallationDb>(&query)
            .bind(company_id)
            .bind(installation_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }

    /// An inbound provider event names a workspace before it names a company, so this is the one
    /// lookup that is deliberately not company-scoped. `(transport, external_tenant_key)` is
    /// unique across the deployment, which is what makes the answer unambiguous.
    async fn find_installation_by_tenant(
        &self,
        transport: TransportKind,
        external_tenant_key: &ExternalTenantKey,
    ) -> AppResult<Option<IntegrationInstallation>> {
        let query = format!(
            "{} WHERE installation.transport = $1 AND installation.external_tenant_key = $2",
            installation_select()
        );
        sqlx::query_as::<_, InstallationDb>(&query)
            .bind(transport.as_str())
            .bind(external_tenant_key.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .map(TryInto::try_into)
            .transpose()
    }

    async fn list_installations(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<IntegrationInstallation>> {
        let query = format!(
            "{} WHERE installation.company_id = $1 \
             ORDER BY installation.installed_at DESC, installation.id DESC",
            installation_select()
        );
        sqlx::query_as::<_, InstallationDb>(&query)
            .bind(company_id)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    /// Move an installation between statuses, recording who did it.
    ///
    /// Revocation is the transition the database insists carries an actor and a time, so the
    /// revocation columns are written from the same actor in the same statement rather than by a
    /// follow-up update that could be lost.
    async fn set_installation_status(
        &self,
        change: InstallationStatusChange,
    ) -> AppResult<IntegrationInstallation> {
        let actor = encoded_provenance(&change.actor)?;
        let revoking = change.status == InstallationStatus::Revoked;
        let query = format!(
            r#"UPDATE integration_installations AS installation
               SET status = $3,
                   updated_by = $4,
                   updated_at = CURRENT_TIMESTAMP,
                   revoked_by = CASE WHEN $5 THEN $4 ELSE NULL END,
                   revoked_at = CASE WHEN $5 THEN CURRENT_TIMESTAMP ELSE NULL END
               WHERE installation.company_id = $1 AND installation.id = $2
               RETURNING {}"#,
            installation_columns_for_returning()
        );

        sqlx::query_as::<_, InstallationDb>(&query)
            .bind(change.company_id)
            .bind(change.installation_id.as_uuid())
            .bind(change.status.as_str())
            .bind(&actor)
            .bind(revoking)
            .fetch_optional(&self.pool)
            .await
            .map_err(AppError::from)?
            .ok_or_else(|| {
                AppError::NotFound("That integration installation was not found.".into())
            })?
            .try_into()
    }
}

/// `RETURNING` has no `FROM` to alias, so the shared column list loses its qualifier here. Derived
/// from the same constant rather than retyped, so a new column reaches both places at once.
fn installation_columns_for_returning() -> String {
    super::INSTALLATION_COLUMNS.replace("installation.", "")
}
