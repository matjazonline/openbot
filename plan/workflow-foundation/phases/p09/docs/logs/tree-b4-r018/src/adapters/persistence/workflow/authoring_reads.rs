//! The authoring pages' reads (p09 B1a): plain company-scoped reads with no locks. A route
//! authorizes before it shows any of them.
use super::reads::{BINDING_COLUMNS, BINDING_FROM, BindingRow, binding_summary, revision_of};
use super::*;
use crate::application::workflow::reads::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

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
    /// A page of binding rows with what each reaches: its workflow's title, its channel's name,
    /// its current resources, and its first schedules by name.
    async fn binding_uses(
        &self,
        company: CompanyId,
        mut rows: Vec<BindingRow>,
        limit: i64,
    ) -> AppResult<WorkflowUsage> {
        let truncated = rows.len() as i64 > limit;
        rows.truncate(limit as usize);
        let bindings = rows
            .into_iter()
            .map(binding_summary)
            .collect::<AppResult<Vec<_>>>()?;
        let ids: Vec<Uuid> = bindings.iter().map(|b| b.binding.as_uuid()).collect();
        let workflows: Vec<Uuid> = bindings.iter().map(|b| b.workflow.as_uuid()).collect();
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
        let names: HashMap<Uuid, String> =
            sqlx::query_as("SELECT id, name FROM channels WHERE company_id = $1 AND id = ANY($2)")
                .bind(company.as_uuid())
                .bind(&channels)
                .fetch_all(self.pool())
                .await?
                .into_iter()
                .collect();
        let titles: HashMap<Uuid, String> = sqlx::query_as(
            "SELECT id, title FROM workflow_definitions WHERE company_id = $1 AND id = ANY($2)",
        )
        .bind(company.as_uuid())
        .bind(&workflows)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .collect();
        let mut resources: HashMap<Uuid, BTreeMap<String, Uuid>> = HashMap::new();
        let stored: Vec<(Uuid, Value)> = sqlx::query_as(
            "SELECT binding.id, revision.resources FROM workflow_bindings AS binding \
               JOIN workflow_binding_revisions AS revision \
                 ON revision.company_id = binding.company_id AND revision.binding_id = binding.id \
                AND revision.revision = binding.configuration_revision \
              WHERE binding.company_id = $1 AND binding.id = ANY($2)",
        )
        .bind(company.as_uuid())
        .bind(&ids)
        .fetch_all(self.pool())
        .await?;
        for (binding, value) in stored {
            resources.insert(
                binding,
                serde_json::from_value(value).map_err(|_| invalid())?,
            );
        }
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
        let mut by_binding: HashMap<Uuid, Vec<ScheduleUse>> = HashMap::new();
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
                    let id = binding.binding.as_uuid();
                    let mut schedules = by_binding.remove(&id).unwrap_or_default();
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
                        workflow_title: titles
                            .get(&binding.workflow.as_uuid())
                            .cloned()
                            .unwrap_or_default(),
                        resources: resources.remove(&id).unwrap_or_default(),
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
        self.binding_uses(company, rows, MAX_LISTED).await
    }

    async fn binding_use(
        &self,
        company: CompanyId,
        binding: Uuid,
    ) -> AppResult<Option<BindingUse>> {
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} \
              WHERE binding.company_id = $1 AND binding.id = $2"
        ))
        .bind(company.as_uuid())
        .bind(binding)
        .fetch_all(self.pool())
        .await?;
        Ok(self.binding_uses(company, rows, 1).await?.bindings.pop())
    }

    async fn channel_usage(&self, company: CompanyId, channel: Uuid) -> AppResult<WorkflowUsage> {
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} \
              WHERE binding.company_id = $1 AND binding.channel_id = $2 \
              ORDER BY binding.id LIMIT $3"
        ))
        .bind(company.as_uuid())
        .bind(channel)
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        self.binding_uses(company, rows, MAX_LISTED).await
    }

    async fn agent_usage(&self, company: CompanyId, agent: Uuid) -> AppResult<WorkflowUsage> {
        // `agent_ids` is a display index (gate P09-B1b-D1): this listing is all that reads it.
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} \
              WHERE binding.company_id = $1 AND $2 = ANY(version.agent_ids) \
              ORDER BY binding.id LIMIT $3"
        ))
        .bind(company.as_uuid())
        .bind(agent)
        .bind(MAX_AGENT_USES + 1)
        .fetch_all(self.pool())
        .await?;
        self.binding_uses(company, rows, MAX_AGENT_USES).await
    }

    async fn binding_options(&self, company: CompanyId) -> AppResult<BindingOptions> {
        let bound = MAX_BINDING_OPTIONS + 1;
        let choices = |rows: Vec<(Uuid, String, bool, String)>| {
            let truncated = rows.len() as i64 > MAX_BINDING_OPTIONS;
            let options = rows
                .into_iter()
                .take(MAX_BINDING_OPTIONS as usize)
                .map(|(id, label, enabled, detail)| ChoiceOption {
                    id,
                    label,
                    enabled,
                    detail,
                })
                .collect::<Vec<_>>();
            (options, truncated)
        };
        let read = |sql: &'static str| {
            sqlx::query_as::<_, (Uuid, String, bool, String)>(sql)
                .bind(company.as_uuid())
                .bind(bound)
                .fetch_all(self.pool())
        };
        let (channels, channels_cut) = choices(
            read(
                "SELECT id, name, true, '' FROM channels WHERE company_id = $1 \
                 ORDER BY name, id LIMIT $2",
            )
            .await?,
        );
        let (http, http_cut) = choices(
            read(
                "SELECT id, origin, enabled, '' FROM workflow_http_connections \
                 WHERE company_id = $1 ORDER BY origin, id LIMIT $2",
            )
            .await?,
        );
        let (mcp, mcp_cut) = choices(
            read(
                "SELECT id, slug::text, enabled, '' FROM company_mcp_connections \
                 WHERE company_id = $1 AND deleted_at IS NULL ORDER BY slug, id LIMIT $2",
            )
            .await?,
        );
        let (people, people_cut) = choices(
            read(
                "SELECT member.user_id, account.username::text, true, member.role \
                   FROM company_members AS member \
                   JOIN users AS account ON account.id = member.user_id \
                  WHERE member.company_id = $1 \
                  ORDER BY member.role = 'member', account.username, member.user_id LIMIT $2",
            )
            .await?,
        );
        Ok(BindingOptions {
            channels,
            http,
            mcp,
            people,
            truncated: channels_cut || http_cut || mcp_cut || people_cut,
        })
    }

    async fn agent_tool_grants(
        &self,
        company: CompanyId,
        agent: Uuid,
    ) -> AppResult<Option<Vec<String>>> {
        Ok(sqlx::query_scalar(
            "SELECT workflow_tools FROM agents WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(agent)
        .fetch_optional(self.pool())
        .await?)
    }

    async fn tool_targets(
        &self,
        company: CompanyId,
        versions: &[VersionId],
    ) -> AppResult<Vec<ToolTarget>> {
        let ids: Vec<Uuid> = versions.iter().map(|version| version.as_uuid()).collect();
        let rows: Vec<(Uuid, Uuid, String, i64, bool)> = sqlx::query_as(
            "SELECT version.id, version.workflow_id, definition.title, version.draft_revision, \
                    definition.archived \
               FROM workflow_versions AS version \
               JOIN workflow_definitions AS definition ON definition.company_id = version.company_id \
                AND definition.id = version.workflow_id \
              WHERE version.company_id = $1 AND version.id = ANY($2)",
        )
        .bind(company.as_uuid())
        .bind(&ids)
        .fetch_all(self.pool())
        .await?;
        rows.into_iter()
            .map(|(version, workflow, title, draft_revision, archived)| {
                Ok(ToolTarget {
                    version: VersionId::new(version),
                    workflow: WorkflowId::new(workflow),
                    workflow_title: title,
                    draft_revision: revision_of(draft_revision)?,
                    archived,
                })
            })
            .collect()
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
