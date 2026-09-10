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
    company_id: Uuid,
    id: Uuid,
    slug: String,
    endpoint_url: String,
    enabled: bool,
    auth_type: String,
    revision: i64,
    credential_revision: i64,
    discovery_json: serde_json::Value,
    secret_set: bool,
    tool_grants: Vec<String>,
}
const CONNECTION_SELECT: &str = "SELECT connection.company_id, connection.id, connection.slug,
    connection.endpoint_url, connection.enabled, connection.auth_type, connection.revision,
    connection.credential_revision, connection.discovery_json,
    EXISTS(SELECT 1 FROM company_mcp_credentials AS credential WHERE credential.company_id = connection.company_id AND credential.connection_id = connection.id) AS secret_set,
    ARRAY(SELECT grant_row.tool_name FROM company_mcp_tool_grants AS grant_row WHERE grant_row.company_id = connection.company_id AND grant_row.connection_id = connection.id ORDER BY grant_row.tool_name) AS tool_grants
    FROM company_mcp_connections AS connection";
impl TryFrom<ConnectionRow> for CompanyMcpConnection {
    type Error = AppError;
    fn try_from(row: ConnectionRow) -> AppResult<Self> {
        let auth = match row.auth_type.as_str() {
            "none" => McpAuth::None,
            "bearer" => McpAuth::Bearer,
            _ => return Err(invalid("Stored MCP auth is invalid")),
        };
        let write = McpConnectionWrite {
            slug: row.slug.into(),
            endpoint: row.endpoint_url.try_into().map_err(AppError::BadRequest)?,
            enabled: row.enabled,
            auth: auth.clone(),
            discovered_tools: serde_json::from_value(row.discovery_json)
                .map_err(|_| invalid("Stored MCP discovery is invalid"))?,
            tool_grants: row
                .tool_grants
                .into_iter()
                .map(McpToolName::try_from)
                .collect::<Result<_, _>>()
                .map_err(AppError::BadRequest)?,
        };
        write.validate()?;
        Ok(Self {
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
    sqlx::query_as::<_, ConnectionRow>(&format!("{CONNECTION_SELECT} WHERE connection.company_id = $1 AND connection.id = $2 AND connection.deleted_at IS NULL"))
        .bind(company).bind(id).fetch_optional(db).await?.ok_or_else(conflict)?.try_into()
}
async fn replace_grants(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    id: Uuid,
    grants: &[McpToolName],
) -> AppResult<()> {
    sqlx::query("DELETE FROM company_mcp_tool_grants WHERE company_id = $1 AND connection_id = $2")
        .bind(company)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    let names: Vec<&str> = grants.iter().map(McpToolName::as_str).collect();
    sqlx::query("INSERT INTO company_mcp_tool_grants (company_id, connection_id, tool_name) SELECT $1, $2, unnest($3::text[])")
        .bind(company).bind(id).bind(names).execute(&mut **tx).await?;
    Ok(())
}
/// Called after tentative changes, before commit. Error rolls the whole operation back.
async fn validate_effective_grants(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
) -> AppResult<()> {
    let invalid_agent: bool = sqlx::query_scalar("SELECT EXISTS(
        SELECT selection.agent_id FROM agent_mcp_selections AS selection
        JOIN agents AS agent ON agent.company_id = selection.company_id AND agent.id = selection.agent_id
        JOIN company_mcp_connections AS connection ON connection.company_id = selection.company_id AND connection.id = selection.connection_id
        JOIN company_mcp_tool_grants AS grant_row ON grant_row.company_id = connection.company_id AND grant_row.connection_id = connection.id
        WHERE selection.company_id = $1 AND connection.enabled AND connection.deleted_at IS NULL
        GROUP BY selection.agent_id, agent.harness_kind HAVING count(*) > $2 OR agent.harness_kind <> 'rig')")
        .bind(company).bind(MAX_EFFECTIVE_MCP_TOOLS as i64).fetch_one(&mut **tx).await?;
    if invalid_agent {
        return Err(invalid(
            "Enabled MCP grants require Rig and at most 32 effective tools per agent",
        ));
    }
    Ok(())
}
async fn selection_on(
    db: &mut PgConnection,
    company: Uuid,
    agent: Uuid,
) -> AppResult<AgentMcpSelection> {
    // One statement gives the revision and its set the same MVCC snapshot, without locking
    // the company row on every runtime read. Writers still serialize and compare revisions.
    let (revision, ids): (i64, Vec<Uuid>) = sqlx::query_as(
        r#"SELECT COALESCE(revision.revision, 1),
                  ARRAY(SELECT selection.connection_id FROM agent_mcp_selections AS selection
                        WHERE selection.company_id = agent.company_id AND selection.agent_id = agent.id
                        ORDER BY selection.connection_id)
           FROM agents AS agent
           LEFT JOIN agent_mcp_selection_revisions AS revision
             ON revision.company_id = agent.company_id AND revision.agent_id = agent.id
           WHERE agent.company_id = $1 AND agent.id = $2"#,
    ).bind(company).bind(agent).fetch_optional(db).await?.ok_or_else(conflict)?;
    Ok(AgentMcpSelection {
        company_id: company,
        agent_id: agent,
        revision,
        connection_ids: ids,
    })
}

#[async_trait]
impl McpPersistence for PostgresPersistence {
    async fn mcp_selecting_agents(
        &self,
        company_id: Uuid,
        connection_id: Uuid,
    ) -> AppResult<Vec<McpSelectingAgent>> {
        let rows = sqlx::query_as::<_, (Uuid, String)>("SELECT agent.id, agent.name FROM agents AS agent JOIN agent_mcp_selections AS selection ON selection.company_id = agent.company_id AND selection.agent_id = agent.id WHERE selection.company_id = $1 AND selection.connection_id = $2 ORDER BY agent.id LIMIT 1001")
            .bind(company_id).bind(connection_id).fetch_all(&self.pool).await?;
        if rows.len() > 1000 {
            return Err(invalid("MCP usage list exceeds 1000 agents"));
        }
        Ok(rows
            .into_iter()
            .map(|(id, name)| McpSelectingAgent { id, name })
            .collect())
    }

    async fn list_mcp_connections(&self, company_id: Uuid) -> AppResult<Vec<CompanyMcpConnection>> {
        let rows = sqlx::query_as::<_, ConnectionRow>(&format!("{CONNECTION_SELECT} WHERE connection.company_id = $1 AND connection.deleted_at IS NULL ORDER BY connection.slug LIMIT 65"))
            .bind(company_id).fetch_all(&self.pool).await?;
        if rows.len() > MAX_MCP_CONNECTIONS {
            return Err(invalid("Stored MCP catalog exceeds 64 definitions"));
        }
        rows.into_iter().map(TryInto::try_into).collect()
    }
    async fn mcp_connections_by_ids(
        &self,
        company_id: Uuid,
        connection_ids: &[Uuid],
    ) -> AppResult<Vec<CompanyMcpConnection>> {
        if connection_ids.len() > MAX_MCP_SELECTIONS {
            return Err(invalid("An agent may select at most 8 MCP connections"));
        }
        if connection_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, ConnectionRow>(&format!(
            "{CONNECTION_SELECT} WHERE connection.company_id = $1 AND connection.id = ANY($2) AND connection.deleted_at IS NULL ORDER BY connection.slug"
        )).bind(company_id).bind(connection_ids).fetch_all(&self.pool).await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
    async fn create_mcp_connection(
        &self,
        company_id: Uuid,
        write: McpConnectionWrite,
    ) -> AppResult<CompanyMcpConnection> {
        write.validate()?;
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
        if old.revision != expected_revision {
            return Err(conflict());
        }
        let rebind = old.endpoint != write.endpoint || old.auth != write.auth;
        if rebind {
            sqlx::query(
                "DELETE FROM company_mcp_credentials WHERE company_id = $1 AND connection_id = $2",
            )
            .bind(company_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE company_mcp_connections SET slug = $3, endpoint_url = $4, enabled = $5, auth_type = $6, discovery_json = $7, revision = revision + 1, credential_revision = credential_revision + $8 WHERE company_id = $1 AND id = $2")
            .bind(company_id).bind(id).bind(write.slug.as_str()).bind(write.endpoint.as_str()).bind(write.enabled).bind(write.auth.as_str())
            .bind(serde_json::to_value(&write.discovered_tools).map_err(|_| invalid("Invalid discovery"))?).bind(i64::from(rebind)).execute(&mut *tx).await?;
        replace_grants(&mut tx, company_id, id, &write.tool_grants).await?;
        validate_effective_grants(&mut tx, company_id).await?;
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
        tx.commit().await?;
        Ok(())
    }
    async fn agent_mcp_selection(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<AgentMcpSelection> {
        selection_on(&mut *self.pool.acquire().await?, company_id, agent_id).await
    }
    async fn replace_agent_mcp_selection(
        &self,
        selection: AgentMcpSelection,
        connection_ids: Option<Vec<Uuid>>,
    ) -> AppResult<AgentMcpSelection> {
        let mut tx = self.pool.begin().await?;
        lock_company(&mut tx, selection.company_id).await?;
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM agents WHERE company_id = $1 AND id = $2 FOR UPDATE",
        )
        .bind(selection.company_id)
        .bind(selection.agent_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(conflict)?;
        let old = selection_on(&mut tx, selection.company_id, selection.agent_id).await?;
        if old.revision != selection.revision {
            return Err(conflict());
        }
        let Some(mut ids) = connection_ids else {
            tx.commit().await?;
            return Ok(old);
        };
        if ids.len() > MAX_MCP_SELECTIONS {
            return Err(invalid("An agent may select at most 8 MCP connections"));
        }
        ids.sort_unstable();
        ids.dedup();
        for id in &ids {
            let connection = get_connection(&mut tx, selection.company_id, *id).await?;
            if !connection.enabled && !old.connection_ids.contains(id) {
                return Err(invalid("New MCP selections must be enabled"));
            }
        }
        sqlx::query("DELETE FROM agent_mcp_selections WHERE company_id = $1 AND agent_id = $2")
            .bind(selection.company_id)
            .bind(selection.agent_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO agent_mcp_selections (company_id, agent_id, connection_id) SELECT $1, $2, unnest($3::uuid[])")
            .bind(selection.company_id).bind(selection.agent_id).bind(&ids).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO agent_mcp_selection_revisions (company_id, agent_id, revision) VALUES ($1, $2, 2) ON CONFLICT (company_id, agent_id) DO UPDATE SET revision = agent_mcp_selection_revisions.revision + 1")
            .bind(selection.company_id).bind(selection.agent_id).execute(&mut *tx).await?;
        validate_effective_grants(&mut tx, selection.company_id).await?;
        let result = selection_on(&mut tx, selection.company_id, selection.agent_id).await?;
        tx.commit().await?;
        Ok(result)
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
