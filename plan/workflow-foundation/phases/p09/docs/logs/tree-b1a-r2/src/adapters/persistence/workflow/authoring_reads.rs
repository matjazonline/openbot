//! The authoring pages' reads (p09 B1a): plain company-scoped reads with no locks. A route
//! authorizes before it shows any of them.
use super::reads::{
    BINDING_COLUMNS, BINDING_FROM, BindingRow, binding_summary, bounded, revision_of,
};
use super::*;
use crate::application::workflow::reads::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct OperationRow {
    connection_id: Uuid,
    connection_label: String,
    connection_enabled: bool,
    name: String,
    snapshot: Value,
    approval_required: bool,
    /// The row's position among the connections offering `name`.
    rank: i64,
}

impl From<OperationRow> for OperationOption {
    fn from(row: OperationRow) -> Self {
        Self {
            connection: row.connection_id,
            connection_label: row.connection_label,
            connection_enabled: row.connection_enabled,
            name: row.name,
            snapshot: row.snapshot,
            approval_required: row.approval_required,
        }
    }
}

#[derive(sqlx::FromRow)]
struct VersionSourceRow {
    id: Uuid,
    draft_revision: i64,
    title: String,
    source: String,
    content_hash: String,
    created_at: DateTime<Utc>,
}

/// Each needed name's first [`MAX_CONNECTIONS_PER_TOOL`] connections, one past the bound so a
/// cut name is known. `{operations}`/`{connections}`/`{label}`/`{live}` name the kind's tables.
fn options_sql(operations: &str, connections: &str, label: &str, live: &str) -> String {
    format!(
        "SELECT connection_id, connection_label, connection_enabled, name, snapshot, \
                approval_required, rank FROM ( \
           SELECT operation.connection_id, connection.{label} AS connection_label, \
                  connection.enabled AS connection_enabled, operation.name, operation.snapshot, \
                  operation.approval_required, \
                  row_number() OVER (PARTITION BY operation.name \
                                     ORDER BY connection.{label}, operation.connection_id) AS rank \
             FROM {operations} AS operation \
             JOIN {connections} AS connection ON connection.company_id = operation.company_id \
              AND connection.id = operation.connection_id{live} \
            WHERE operation.company_id = $1 AND operation.name = ANY($2)) AS ranked \
          WHERE rank <= $3 ORDER BY name, rank"
    )
}

impl PostgresPersistence {
    async fn options(
        &self,
        sql: &str,
        company: CompanyId,
        names: &[String],
        cut: &mut std::collections::BTreeSet<String>,
    ) -> AppResult<Vec<OperationOption>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<OperationRow> = sqlx::query_as(sql)
            .bind(company.as_uuid())
            .bind(names)
            .bind(MAX_CONNECTIONS_PER_TOOL + 1)
            .fetch_all(self.pool())
            .await?;
        let mut options = Vec::with_capacity(rows.len());
        for row in rows {
            if row.rank > MAX_CONNECTIONS_PER_TOOL {
                cut.insert(row.name);
            } else {
                options.push(row.into());
            }
        }
        Ok(options)
    }
}

#[async_trait]
impl AuthoringReads for PostgresPersistence {
    async fn version_source(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
        version: VersionId,
    ) -> AppResult<Option<VersionSource>> {
        let row: Option<VersionSourceRow> = sqlx::query_as(
            "SELECT id, draft_revision, title, source, content_hash, created_at \
               FROM workflow_versions WHERE company_id = $1 AND workflow_id = $2 AND id = $3",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .bind(version.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        row.map(|row| {
            Ok(VersionSource {
                version: VersionId::new(row.id),
                draft_revision: revision_of(row.draft_revision)?,
                title: row.title,
                source: row.source,
                content_hash: row.content_hash,
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn workflow_usage(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<WorkflowUsage> {
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} \
              WHERE binding.company_id = $1 AND version.workflow_id = $2 \
              ORDER BY binding.id LIMIT $3"
        ))
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (rows, truncated) = bounded(rows);
        let bindings = rows
            .into_iter()
            .map(binding_summary)
            .collect::<AppResult<Vec<_>>>()?;
        let ids: Vec<Uuid> = bindings.iter().map(|b| b.binding.as_uuid()).collect();
        let channels: Vec<Uuid> = bindings
            .iter()
            .filter_map(|b| match b.association {
                RelatedAssociation::Company => None,
                RelatedAssociation::Channel(channel)
                | RelatedAssociation::Thread {
                    channel_id: channel,
                    ..
                } => Some(channel.as_uuid()),
            })
            .collect();
        let names: std::collections::HashMap<Uuid, String> =
            sqlx::query_as("SELECT id, name FROM channels WHERE company_id = $1 AND id = ANY($2)")
                .bind(company.as_uuid())
                .bind(&channels)
                .fetch_all(self.pool())
                .await?
                .into_iter()
                .collect();
        // Each binding's first schedules by name, one past the bound to flag the cut.
        let schedules: Vec<(Uuid, Uuid, String, bool)> = sqlx::query_as(
            "SELECT binding.id, schedule.id, schedule.name, schedule.enabled \
               FROM unnest($2::uuid[]) AS binding(id) \
               CROSS JOIN LATERAL ( \
                 SELECT id, name, enabled FROM channel_schedules \
                  WHERE company_id = $1 AND binding_id = binding.id \
                  ORDER BY name, id LIMIT $3) AS schedule",
        )
        .bind(company.as_uuid())
        .bind(&ids)
        .bind(MAX_SCHEDULES_PER_BINDING + 1)
        .fetch_all(self.pool())
        .await?;
        let mut by_binding: std::collections::HashMap<Uuid, Vec<ScheduleUse>> =
            std::collections::HashMap::new();
        for (binding, schedule, name, enabled) in schedules {
            by_binding.entry(binding).or_default().push(ScheduleUse {
                schedule,
                name,
                enabled,
            });
        }
        Ok(WorkflowUsage {
            bindings: bindings
                .into_iter()
                .map(|binding| {
                    let mut schedules = by_binding
                        .remove(&binding.binding.as_uuid())
                        .unwrap_or_default();
                    let schedules_truncated = schedules.len() > MAX_SCHEDULES_PER_BINDING as usize;
                    schedules.truncate(MAX_SCHEDULES_PER_BINDING as usize);
                    let channel_name = match binding.association {
                        RelatedAssociation::Company => None,
                        RelatedAssociation::Channel(channel)
                        | RelatedAssociation::Thread {
                            channel_id: channel,
                            ..
                        } => names.get(&channel.as_uuid()).cloned(),
                    };
                    BindingUse {
                        binding,
                        channel_name,
                        schedules,
                        schedules_truncated,
                    }
                })
                .collect(),
            truncated,
        })
    }

    async fn publication_options(
        &self,
        company: CompanyId,
        mcp_tools: &[String],
        http_tools: &[String],
    ) -> AppResult<PublicationOptions> {
        if mcp_tools.len() > MAX_OPTION_NAMES || http_tools.len() > MAX_OPTION_NAMES {
            return Err(AppError::BadRequest(format!(
                "A draft names at most {MAX_OPTION_NAMES} tools of each kind"
            )));
        }
        let (mut mcp_cut, mut http_cut) = Default::default();
        let mcp = self
            .options(
                &options_sql(
                    "workflow_mcp_operations",
                    "company_mcp_connections",
                    "slug",
                    " AND connection.deleted_at IS NULL",
                ),
                company,
                mcp_tools,
                &mut mcp_cut,
            )
            .await?;
        let http = self
            .options(
                &options_sql(
                    "workflow_http_operations",
                    "workflow_http_connections",
                    "origin",
                    "",
                ),
                company,
                http_tools,
                &mut http_cut,
            )
            .await?;
        Ok(PublicationOptions {
            mcp,
            http,
            mcp_cut,
            http_cut,
        })
    }
}
