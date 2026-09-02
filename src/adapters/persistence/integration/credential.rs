//! The narrow credential boundary.
//!
//! This is the only module that names `integration_credentials.envelope`. Every statement here
//! addresses exactly one row by its full `(company, installation, kind)` key, and the transport it
//! authenticates against is read from the installation in the same statement rather than taken
//! from the caller — so a caller cannot present a scope whose transport does not match the account
//! the credential actually belongs to.

use async_trait::async_trait;
use secrecy::SecretString;
use tracing::warn;

use crate::{
    adapters::persistence::{PostgresPersistence, credentials::CredentialContext},
    app_error::{AppError, AppResult},
    use_cases::integration::{CredentialScope, InstallationCredentialStore},
};

#[async_trait]
impl InstallationCredentialStore for PostgresPersistence {
    async fn store_credential(
        &self,
        scope: &CredentialScope,
        secret: SecretString,
    ) -> AppResult<()> {
        let cipher = self.credential_cipher()?;
        let envelope = cipher.seal_envelope(&context(scope), &secret)?;

        // The insert is scoped to an installation that actually speaks this transport, so a scope
        // whose transport is wrong writes nothing rather than storing a credential that could
        // never be opened again.
        let stored = sqlx::query(
            r#"INSERT INTO integration_credentials
                   (company_id, installation_id, credential_kind, envelope)
               SELECT installation.company_id, installation.id, $3, $4
               FROM integration_installations AS installation
               WHERE installation.company_id = $1
                 AND installation.id = $2
                 AND installation.transport = $5
               ON CONFLICT (company_id, installation_id, credential_kind) DO UPDATE
                   SET envelope = EXCLUDED.envelope,
                       updated_at = CURRENT_TIMESTAMP"#,
        )
        .bind(scope.company_id)
        .bind(scope.installation_id.as_uuid())
        .bind(scope.kind.as_str())
        .bind(&envelope)
        .bind(scope.transport.as_str())
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?
        .rows_affected();

        if stored == 0 {
            return Err(AppError::NotFound(format!(
                "No {} installation of that id exists for this company.",
                scope.transport
            )));
        }
        Ok(())
    }

    /// `None` only when no credential of that kind is stored.
    ///
    /// A row that exists but fails to authenticate against `scope` is an error, deliberately: a
    /// tampered or misplaced envelope answering "this installation has no token" would send the
    /// caller down a re-authorization path that silently papers over a corrupted store.
    async fn read_credential(&self, scope: &CredentialScope) -> AppResult<Option<SecretString>> {
        let cipher = self.credential_cipher()?;
        let envelope: Option<String> = sqlx::query_scalar(
            r#"SELECT credential.envelope
               FROM integration_credentials AS credential
               JOIN integration_installations AS installation
                   ON installation.company_id = credential.company_id
                  AND installation.id = credential.installation_id
               WHERE credential.company_id = $1
                 AND credential.installation_id = $2
                 AND credential.credential_kind = $3
                 AND installation.transport = $4"#,
        )
        .bind(scope.company_id)
        .bind(scope.installation_id.as_uuid())
        .bind(scope.kind.as_str())
        .bind(scope.transport.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;

        let Some(envelope) = envelope else {
            return Ok(None);
        };
        cipher
            .open_envelope(&context(scope), &envelope)
            .map(Some)
            .inspect_err(|error| {
                // Identifiers and the failure class only; `EnvelopeError` is written so that its
                // own text carries neither ciphertext nor key material.
                warn!(
                    company_id = %scope.company_id,
                    installation_id = %scope.installation_id,
                    credential_kind = scope.kind.as_str(),
                    failure = %error,
                    "stored integration credential failed to open"
                );
            })
    }

    async fn delete_credential(&self, scope: &CredentialScope) -> AppResult<bool> {
        let deleted = sqlx::query(
            r#"DELETE FROM integration_credentials AS credential
               WHERE credential.company_id = $1
                 AND credential.installation_id = $2
                 AND credential.credential_kind = $3"#,
        )
        .bind(scope.company_id)
        .bind(scope.installation_id.as_uuid())
        .bind(scope.kind.as_str())
        .execute(&self.pool)
        .await
        .map_err(AppError::from)?
        .rows_affected();

        Ok(deleted > 0)
    }
}

/// The authenticated context for one credential row, built in exactly one place so a reader and a
/// writer cannot disagree about what the tag covers.
fn context(scope: &CredentialScope) -> CredentialContext {
    CredentialContext::integration_credential(
        scope.company_id,
        scope.installation_id.as_uuid(),
        scope.transport,
        scope.kind,
    )
}
