//! All catalog mutations serialize on the company row, then the agent row. This single lock order
//! makes selection/grant bounds and revision comparisons atomic even for competing writers.
use super::{PostgresPersistence, credentials::envelope::CredentialContext};
use crate::{
    app_error::{AppError, AppResult},
    entities::mcp::*,
    use_cases::mcp::*,
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    access_level: i16,
    company_id: Uuid,
    id: Uuid,
    slug: String,
    endpoint_url: String,
    enabled: bool,
    auth_type: String,
    integration: Option<serde_json::Value>,
    revision: i64,
    credential_revision: i64,
    discovery_json: serde_json::Value,
    secret_set: bool,
    tool_grants: Vec<String>,
}
const CONNECTION_SELECT: &str = "SELECT connection.company_id, connection.id, connection.access_level::smallint AS access_level, connection.slug,
    connection.endpoint_url, connection.enabled, connection.auth_type,
    CASE WHEN connection.integration_grant_id IS NOT NULL THEN jsonb_build_object('account',connection.integration_account_id,'grant',connection.integration_grant_id,'authorization',connection.integration_authorization,'grant_revision',connection.integration_grant_revision) END AS integration, connection.revision,
    connection.credential_revision, connection.discovery_json,
    EXISTS(SELECT 1 FROM company_mcp_credentials AS credential WHERE credential.company_id = connection.company_id AND credential.connection_id = connection.id) AS secret_set,
    ARRAY(SELECT grant_row.tool_name FROM company_mcp_tool_grants AS grant_row WHERE grant_row.company_id = connection.company_id AND grant_row.connection_id = connection.id AND grant_row.revoked_at IS NULL ORDER BY grant_row.tool_name) AS tool_grants
    FROM company_mcp_connections AS connection";
impl TryFrom<ConnectionRow> for CompanyMcpConnection {
    type Error = AppError;
    fn try_from(row: ConnectionRow) -> AppResult<Self> {
        let auth = match row.auth_type.as_str() {
            "none" => McpAuth::None,
            "bearer" => McpAuth::Bearer,
            "integration" => McpAuth::Integration(
                serde_json::from_value(
                    row.integration
                        .ok_or_else(|| invalid("Missing integration reference"))?,
                )
                .map_err(|_| invalid("Invalid integration reference"))?,
            ),
            _ => return Err(invalid("Stored MCP auth is invalid")),
        };
        let mut write = McpConnectionWrite {
            slug: row.slug.into(),
            endpoint: row.endpoint_url.try_into().map_err(AppError::BadRequest)?,
            enabled: row.enabled,
            auth: if matches!(auth, McpAuth::Integration(_)) {
                McpAuth::None
            } else {
                auth.clone()
            },
            discovered_tools: Vec::new(),
            tool_grants: Vec::new(),
        };
        // Validate ordinary settings strictly before the optional catalog is considered.
        write.validate()?;
        match read_catalog(row.discovery_json, row.tool_grants) {
            Ok(catalog) => {
                write.discovered_tools = catalog.tools;
                write.tool_grants = catalog.grants;
            }
            Err(error) => tracing::warn!(connection_id = %row.id, error,
                "Stored MCP discovery was quarantined"),
        }
        Ok(Self {
            access_level: crate::entities::access_level::AccessLevel::try_from(row.access_level)
                .map_err(|_| invalid("Invalid connection access level"))?,
            company_id: row.company_id,
            id: row.id,
            slug: write.slug,
            endpoint: write.endpoint,
            enabled: write.enabled,
            auth,
            secret_set: row.secret_set,
            revision: row.revision,
            credential_revision: row.credential_revision,
            discovered_tools: write.discovered_tools,
            tool_grants: write.tool_grants,
        })
    }
}
struct ReadCatalog {
    tools: Vec<McpDiscoveredTool>,
    grants: Vec<McpToolName>,
}

fn read_catalog(
    value: serde_json::Value,
    grants: Vec<String>,
) -> Result<ReadCatalog, &'static str> {
    let tools: Vec<McpDiscoveredTool> =
        serde_json::from_value(value).map_err(|_| "Malformed stored MCP discovery")?;
    let grants = grants
        .into_iter()
        .map(McpToolName::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Malformed stored MCP grants")?;
    validate_catalog(&tools, &grants).map_err(|_| "Invalid stored MCP discovery catalog")?;
    Ok(ReadCatalog { tools, grants })
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
fn conflict() -> AppError {
    AppError::Conflict("MCP configuration changed or is unavailable; reload before saving".into())
}
async fn lock_company(tx: &mut Transaction<'_, Postgres>, company_id: Uuid) -> AppResult<()> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(company_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(conflict)?;
    Ok(())
}
async fn get_connection(
    db: &mut PgConnection,
    company: Uuid,
    id: Uuid,
) -> AppResult<CompanyMcpConnection> {
    connection_on(db, company, id).await?.ok_or_else(conflict)
}

/// Caller holds the company lock when using these facts as transaction authority.
pub(super) async fn connection_on(
    db: &mut PgConnection,
    company: Uuid,
    id: Uuid,
) -> AppResult<Option<CompanyMcpConnection>> {
    sqlx::query_as::<_, ConnectionRow>(&format!("{CONNECTION_SELECT} WHERE connection.company_id = $1 AND connection.id = $2 AND connection.deleted_at IS NULL"))
        .bind(company).bind(id).fetch_optional(db).await?.map(TryInto::try_into).transpose()
}

/// Publication cannot quarantine invalid stored discovery into an apparently empty catalogue.
/// Keep the ordinary settings-page projection separate; capture locks and validates the source.
pub(super) async fn publication_connection_on(
    db: &mut PgConnection,
    company: Uuid,
    id: Uuid,
) -> AppResult<Option<CompanyMcpConnection>> {
    let row = sqlx::query_as::<_, ConnectionRow>(&format!(
        "{CONNECTION_SELECT} WHERE connection.company_id=$1 AND connection.id=$2 \
         AND connection.deleted_at IS NULL FOR SHARE OF connection"
    ))
    .bind(company)
    .bind(id)
    .fetch_optional(db)
    .await?;
    row.map(|row| {
        read_catalog(row.discovery_json.clone(), row.tool_grants.clone()).map_err(invalid)?;
        row.try_into()
    })
    .transpose()
}

async fn replace_grants(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    id: Uuid,
    grants: &[McpToolName],
) -> AppResult<()> {
    // Only removed permissions retire approvals. Retained keys keep historical binding FKs;
    // regranting a key never restores a reviewed operation or an old agent grant.
    let names: Vec<&str> = grants.iter().map(McpToolName::as_str).collect();
    sqlx::query("UPDATE company_mcp_tool_grants SET revoked_at=clock_timestamp() WHERE company_id = $1 AND connection_id = $2 AND revoked_at IS NULL AND NOT (tool_name = ANY($3::text[]))")
        .bind(company).bind(id).bind(&names).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO company_mcp_tool_grants (company_id, connection_id, tool_name) SELECT $1, $2, unnest($3::text[]) ON CONFLICT (company_id, connection_id, tool_name) DO UPDATE SET revoked_at=NULL WHERE company_mcp_tool_grants.revoked_at IS NOT NULL")
        .bind(company).bind(id).bind(&names).execute(&mut **tx).await?;
    Ok(())
}
#[async_trait]
impl McpPersistence for PostgresPersistence {
    async fn set_mcp_level(
        &self,
        command: crate::application::workflow::connection_level::ConnectionLevelCommand,
    ) -> AppResult<CompanyMcpConnection> {
        use crate::entities::access_level::{LevelTarget, may_assign_level};
        let mut tx = super::workflow::http::begin_http(self).await?;
        let actor =
            super::workflow::authority::authorize_company(&mut tx, command.company, command.actor)
                .await?;
        let old = get_connection(&mut tx, command.company.as_uuid(), command.connection).await?;
        if old.revision != command.expected_revision {
            return Err(conflict());
        }
        may_assign_level(
            actor,
            LevelTarget::Assignable(old.access_level),
            command.access_level,
        )
        .map_err(|e| invalid(&e.to_string()))?;
        sqlx::query("UPDATE company_mcp_connections SET access_level=$3::smallint,revision=revision+1 WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL")
            .bind(command.company.as_uuid()).bind(command.connection).bind(command.access_level.as_i16()).execute(&mut *tx).await?;
        let result = get_connection(&mut tx, command.company.as_uuid(), command.connection).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn list_mcp_connections(&self, company_id: Uuid) -> AppResult<Vec<CompanyMcpConnection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(&format!("{CONNECTION_SELECT} WHERE connection.company_id = $1 AND connection.deleted_at IS NULL ORDER BY connection.slug LIMIT 65"))
            .bind(company_id).fetch_all(&self.pool).await?;
        if rows.len() > MAX_MCP_CONNECTIONS {
            return Err(invalid("Stored MCP catalog exceeds 64 definitions"));
        }
        rows.into_iter().map(TryInto::try_into).collect()
    }
    async fn create_mcp_connection(
        &self,
        company_id: Uuid,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        write.validate()?;
        if matches!(write.auth, McpAuth::Integration(_)) {
            return Err(invalid("Confirm an integration binding through its owner"));
        }
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, company_id).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM company_mcp_connections WHERE company_id = $1 AND deleted_at IS NULL")
            .bind(company_id).fetch_one(&mut *tx).await?;
        if count >= MAX_MCP_CONNECTIONS as i64 {
            return Err(invalid(
                "A company may configure at most 64 MCP connections",
            ));
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO company_mcp_connections (company_id, id, slug, endpoint_url, enabled, auth_type, discovery_json) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(company_id).bind(id).bind(write.slug.as_str()).bind(write.endpoint.as_str())
            .bind(write.enabled).bind(write.auth.as_str()).bind(serde_json::to_value(&write.discovered_tools).map_err(|_| invalid("Invalid discovery"))?).execute(&mut *tx).await?;
        replace_grants(&mut tx, company_id, id, &write.tool_grants).await?;
        let result = get_connection(&mut tx, company_id, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn update_mcp_connection(
        &self,
        company_id: Uuid,
        id: Uuid,
        expected_revision: i64,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        write.validate()?;
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, company_id).await?;
        let old = get_connection(&mut tx, company_id, id).await?;
        if matches!(write.auth, McpAuth::Integration(_)) && write.auth != old.auth {
            return Err(invalid("Confirm an integration binding through its owner"));
        }
        if old.revision != expected_revision {
            return Err(conflict());
        }
        if old.endpoint != write.endpoint {
            return Err(crate::use_cases::mcp::endpoint_fixed());
        }
        let rebind = old.auth != write.auth;
        if rebind {
            sqlx::query(
                "DELETE FROM company_mcp_credentials WHERE company_id = $1 AND connection_id = $2",
            )
            .bind(company_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE company_mcp_connections SET slug = $3, endpoint_url = $4, enabled = $5, auth_type = $6, integration_account_id=CASE WHEN $6='integration' THEN integration_account_id ELSE NULL END,integration_grant_id=CASE WHEN $6='integration' THEN integration_grant_id ELSE NULL END,integration_authorization=CASE WHEN $6='integration' THEN integration_authorization ELSE NULL END,integration_grant_revision=CASE WHEN $6='integration' THEN integration_grant_revision ELSE NULL END, discovery_json = $7, revision = revision + 1, credential_revision = credential_revision + $8 WHERE company_id = $1 AND id = $2")
            .bind(company_id).bind(id).bind(write.slug.as_str()).bind(write.endpoint.as_str()).bind(write.enabled).bind(write.auth.as_str())
            .bind(serde_json::to_value(&write.discovered_tools).map_err(|_| invalid("Invalid discovery"))?).bind(i64::from(rebind)).execute(&mut *tx).await?;
        replace_grants(&mut tx, company_id, id, &write.tool_grants).await?;
        let result = get_connection(&mut tx, company_id, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn delete_mcp_connection(
        &self,
        company_id: Uuid,
        id: Uuid,
        expected_revision: i64,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, company_id).await?;
        let changed = sqlx::query("UPDATE company_mcp_connections SET deleted_at = CURRENT_TIMESTAMP, enabled = false, revision = revision + 1, credential_revision = credential_revision + 1 WHERE company_id = $1 AND id = $2 AND revision = $3 AND deleted_at IS NULL")
            .bind(company_id).bind(id).bind(expected_revision).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(conflict());
        }
        sqlx::query(
            "DELETE FROM company_mcp_credentials WHERE company_id = $1 AND connection_id = $2",
        )
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        // The soft-deleted row stays for audit; its workflow approvals do not.
        sqlx::query(
            "UPDATE workflow_mcp_operations SET revoked_at=coalesce(revoked_at,clock_timestamp()) WHERE company_id = $1 AND connection_id = $2",
        )
        .bind(company_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl McpCredentialPersistence for PostgresPersistence {
    async fn replace_mcp_token(
        &self,
        company_id: Uuid,
        id: Uuid,
        expected_revision: i64,
        token: Option<SecretString>,
    ) -> AppResult<i64> {
        if token.as_ref().is_some_and(|token| {
            token.expose_secret().is_empty()
                || token.expose_secret().len() > MAX_MCP_SECRET_BYTES
                || token.expose_secret().chars().any(char::is_control)
        }) {
            return Err(invalid(
                "MCP bearer token must contain 1–16384 bytes without control characters",
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, company_id).await?;
        let connection = get_connection(&mut tx, company_id, id).await?;
        if connection.revision != expected_revision {
            return Err(conflict());
        }
        if token.is_some() && connection.auth != McpAuth::Bearer {
            return Err(invalid(
                "Select bearer authentication before setting a token",
            ));
        }
        match token {
            Some(token) => {
                let envelope = self.credential_cipher()?.seal_envelope(
                    &CredentialContext::company_mcp_credential(company_id, id),
                    &token,
                )?;
                sqlx::query("INSERT INTO company_mcp_credentials (company_id, connection_id, envelope) VALUES ($1, $2, $3) ON CONFLICT (company_id, connection_id) DO UPDATE SET envelope = EXCLUDED.envelope")
                    .bind(company_id).bind(id).bind(envelope).execute(&mut *tx).await?;
            }
            None => {
                sqlx::query("DELETE FROM company_mcp_credentials WHERE company_id = $1 AND connection_id = $2").bind(company_id).bind(id).execute(&mut *tx).await?;
            }
        }
        let revision = sqlx::query_scalar("UPDATE company_mcp_connections SET revision = revision + 1, credential_revision = credential_revision + 1 WHERE company_id = $1 AND id = $2 RETURNING revision")
            .bind(company_id).bind(id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(revision)
    }
    async fn mcp_token(
        &self,
        company_id: Uuid,
        id: Uuid,
        expected_revision: i64,
    ) -> AppResult<Option<SecretString>> {
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, company_id).await?;
        let connection = get_connection(&mut tx, company_id, id).await?;
        if connection.revision != expected_revision {
            return Err(conflict());
        }
        if matches!(connection.auth, McpAuth::Integration(_)) {
            return Err(AppError::ServiceUnavailable(
                "Integration discovery requires its owner configuration context".into(),
            ));
        }
        let envelope: Option<String> = sqlx::query_scalar("SELECT envelope FROM company_mcp_credentials WHERE company_id = $1 AND connection_id = $2")
            .bind(company_id).bind(id).fetch_optional(&mut *tx).await?;
        let token = envelope
            .map(|envelope| {
                self.credential_cipher()?.open_envelope(
                    &CredentialContext::company_mcp_credential(company_id, id),
                    &envelope,
                )
            })
            .transpose()?;
        tx.commit().await?;
        Ok(token)
    }
}
