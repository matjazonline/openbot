//! The authoring pages' reads (p09 B1a): plain company-scoped reads with no locks. A route
//! authorizes before it shows any of them.
use super::reads::{
    BINDING_COLUMNS, BINDING_FROM, BindingRow, binding_summary, bounded, revision_of,
};
use super::*;
use crate::application::workflow::reads::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

type OperationRow = (Uuid, String, bool, String, Value, bool);

fn operation(
    (connection, label, enabled, name, snapshot, approval_required): OperationRow,
) -> OperationOption {
    OperationOption {
        connection,
        connection_label: label,
        connection_enabled: enabled,
        name,
        snapshot,
        approval_required,
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
        let row: Option<(Uuid, i64, String, String, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, draft_revision, title, source, content_hash, created_at \
               FROM workflow_versions WHERE company_id = $1 AND workflow_id = $2 AND id = $3",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .bind(version.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        row.map(|(id, draft, title, source, hash, created)| {
            Ok(VersionSource {
                version: VersionId::new(id),
                draft_revision: revision_of(draft)?,
                title,
                source,
                content_hash: hash,
                created_at: created,
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

    async fn publication_options(&self, company: CompanyId) -> AppResult<PublicationOptions> {
        let mcp: Vec<OperationRow> = sqlx::query_as(
            "SELECT operation.connection_id, connection.slug, connection.enabled, operation.name, \
                    operation.snapshot, operation.approval_required \
               FROM workflow_mcp_operations AS operation \
               JOIN company_mcp_connections AS connection \
                 ON connection.company_id = operation.company_id \
                AND connection.id = operation.connection_id AND connection.deleted_at IS NULL \
              WHERE operation.company_id = $1 \
              ORDER BY operation.name, connection.slug, operation.connection_id LIMIT $2",
        )
        .bind(company.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let http: Vec<OperationRow> = sqlx::query_as(
            "SELECT operation.connection_id, connection.origin, connection.enabled, operation.name, \
                    operation.snapshot, operation.approval_required \
               FROM workflow_http_operations AS operation \
               JOIN workflow_http_connections AS connection \
                 ON connection.company_id = operation.company_id \
                AND connection.id = operation.connection_id \
              WHERE operation.company_id = $1 \
              ORDER BY operation.name, connection.origin, operation.connection_id LIMIT $2",
        )
        .bind(company.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (mcp, mcp_cut) = bounded(mcp);
        let (http, http_cut) = bounded(http);
        Ok(PublicationOptions {
            mcp: mcp.into_iter().map(operation).collect(),
            http: http.into_iter().map(operation).collect(),
            truncated: mcp_cut || http_cut,
        })
    }
}
