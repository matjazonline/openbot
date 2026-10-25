//! The workflow routes' read models (p08 B3b-1): plain company-scoped reads with no locks. A
//! route authorizes before it shows any of them.
use super::binding_rows::association_from_columns;
use super::*;
use crate::application::workflow::reads::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

pub(super) fn revision_of(value: i64) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| invalid())
}

/// Rows fetched one past the page size become the page and its `more` flag.
fn paged<T>(mut rows: Vec<T>, size: i64) -> Page<T> {
    let size = usize::try_from(size).unwrap_or(0);
    let more = rows.len() > size;
    rows.truncate(size);
    Page { items: rows, more }
}

/// A detail list fetched one past [`MAX_LISTED`]: the kept rows and whether any were cut.
pub(super) fn bounded<T>(mut rows: Vec<T>) -> (Vec<T>, bool) {
    let cut = rows.len() > MAX_LISTED as usize;
    rows.truncate(MAX_LISTED as usize);
    (rows, cut)
}

fn stale_cursor() -> AppError {
    AppError::BadRequest("The page cursor no longer names a row; start the listing again".into())
}

type DefinitionRow = (Uuid, i64, String, bool, Option<Value>);

fn definition_summary(
    (workflow, revision, title, archived, origin): DefinitionRow,
) -> AppResult<DefinitionSummary> {
    Ok(DefinitionSummary {
        workflow: WorkflowId::new(workflow),
        revision: revision_of(revision)?,
        title,
        archived,
        template_origin: origin,
    })
}

pub(super) type BindingRow = (
    Uuid,
    i64,
    i64,
    bool,
    Option<Uuid>,
    Option<Uuid>,
    Uuid,
    Uuid,
    Value,
    Uuid,
    Option<String>,
);

pub(super) const BINDING_COLUMNS: &str = "binding.id, binding.state_revision, binding.configuration_revision, \
    binding.active, binding.channel_id, binding.thread_id, version.workflow_id, revision.version_id, \
    revision.params, revision.run_as_user_id, revision.message_trigger";
pub(super) const BINDING_FROM: &str = "FROM workflow_bindings AS binding \
    JOIN workflow_binding_revisions AS revision ON revision.company_id = binding.company_id \
      AND revision.binding_id = binding.id AND revision.revision = binding.configuration_revision \
    JOIN workflow_versions AS version ON version.company_id = revision.company_id \
      AND version.id = revision.version_id";

pub(super) fn binding_summary(row: BindingRow) -> AppResult<BindingSummary> {
    let (
        id,
        state,
        configuration,
        active,
        channel,
        thread,
        workflow,
        version,
        params,
        run_as,
        trigger,
    ) = row;
    Ok(BindingSummary {
        binding: WorkflowBindingId::new(id),
        state_revision: revision_of(state)?,
        configuration_revision: revision_of(configuration)?,
        active,
        association: association_from_columns(channel, thread)?,
        workflow: WorkflowId::new(workflow),
        version: VersionId::new(version),
        params,
        run_as,
        message_trigger: trigger
            .map(|trigger| trigger.parse().map_err(|_| invalid()))
            .transpose()?,
    })
}

type RunRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    Option<Uuid>,
    Option<Uuid>,
    String,
    Option<String>,
    i64,
    Uuid,
    Option<Uuid>,
    Option<Uuid>,
    DateTime<Utc>,
    DateTime<Utc>,
);

const RUN_COLUMNS: &str = "id, binding_id, workflow_id, version_id, channel_id, thread_id, state, \
    waiting_reason, revision, actor_id, parent_run_id, rerun_of_run_id, created_at, deadline";

fn run_summary(row: RunRow) -> AppResult<RunSummary> {
    let (
        id,
        binding,
        workflow,
        version,
        channel,
        thread,
        state,
        waiting,
        revision,
        actor,
        parent,
        rerun,
        created,
        deadline,
    ) = row;
    Ok(RunSummary {
        run: RunId::new(id),
        binding: WorkflowBindingId::new(binding),
        workflow: WorkflowId::new(workflow),
        version: VersionId::new(version),
        association: association_from_columns(channel, thread)?,
        state,
        waiting_reason: waiting,
        revision: revision_of(revision)?,
        actor,
        parent: parent.map(RunId::new),
        rerun_of: rerun.map(RunId::new),
        created_at: created,
        deadline,
    })
}

#[async_trait]
impl WorkflowReads for PostgresPersistence {
    async fn list_definitions(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<DefinitionSummary>> {
        let cursor: Option<(String, Uuid)> = match page.after {
            None => None,
            Some(after) => Some(
                sqlx::query_as(
                    "SELECT title, id FROM workflow_definitions WHERE company_id = $1 AND id = $2",
                )
                .bind(company.as_uuid())
                .bind(after)
                .fetch_optional(self.pool())
                .await?
                .ok_or_else(stale_cursor)?,
            ),
        };
        let rows = sqlx::query_as::<_, DefinitionRow>(
            "SELECT id, revision, title, archived, template_origin FROM workflow_definitions \
             WHERE company_id = $1 AND ($2::text IS NULL OR (title, id) > ($2, $3)) \
             ORDER BY title, id LIMIT $4",
        )
        .bind(company.as_uuid())
        .bind(cursor.as_ref().map(|(title, _)| title.as_str()))
        .bind(cursor.as_ref().map(|(_, id)| *id))
        .bind(page.size + 1)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(definition_summary)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn definition(
        &self,
        company: CompanyId,
        workflow: WorkflowId,
    ) -> AppResult<Option<DefinitionDetail>> {
        let row: Option<(Uuid, i64, String, bool, Option<Value>, String, String)> = sqlx::query_as(
            "SELECT id, revision, title, archived, template_origin, description, source \
                 FROM workflow_definitions WHERE company_id = $1 AND id = $2",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .fetch_optional(self.pool())
        .await?;
        let Some((id, revision, title, archived, origin, description, source)) = row else {
            return Ok(None);
        };
        let versions: Vec<(Uuid, i64, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, draft_revision, content_hash, created_at FROM workflow_versions \
             WHERE company_id = $1 AND workflow_id = $2 ORDER BY created_at DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(workflow.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (versions, versions_truncated) = bounded(versions);
        Ok(Some(DefinitionDetail {
            summary: definition_summary((id, revision, title, archived, origin))?,
            description,
            source,
            versions: versions
                .into_iter()
                .map(|(version, draft, hash, created)| {
                    Ok(VersionSummary {
                        version: VersionId::new(version),
                        draft_revision: revision_of(draft)?,
                        content_hash: hash,
                        created_at: created,
                    })
                })
                .collect::<AppResult<_>>()?,
            versions_truncated,
        }))
    }

    async fn list_bindings(
        &self,
        company: CompanyId,
        page: PageRequest,
    ) -> AppResult<Page<BindingSummary>> {
        let rows = sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} WHERE binding.company_id = $1 \
             AND ($2::uuid IS NULL OR binding.id > $2) ORDER BY binding.id LIMIT $3"
        ))
        .bind(company.as_uuid())
        .bind(page.after)
        .bind(page.size + 1)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .map(binding_summary)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn binding_summary(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingSummary>> {
        sqlx::query_as::<_, BindingRow>(&format!(
            "SELECT {BINDING_COLUMNS} {BINDING_FROM} WHERE binding.company_id = $1 AND binding.id = $2"
        ))
        .bind(company.as_uuid())
        .bind(binding.as_uuid())
        .fetch_optional(self.pool())
        .await?
        .map(binding_summary)
        .transpose()
    }

    async fn list_runs(
        &self,
        company: CompanyId,
        filter: &RunFilter,
        page: PageRequest,
    ) -> AppResult<Page<RunSummary>> {
        let cursor: Option<(DateTime<Utc>, Uuid)> = match page.after {
            None => None,
            Some(after) => Some(
                sqlx::query_as(
                    "SELECT created_at, id FROM workflow_runs WHERE company_id = $1 AND id = $2",
                )
                .bind(company.as_uuid())
                .bind(after)
                .fetch_optional(self.pool())
                .await?
                .ok_or_else(stale_cursor)?,
            ),
        };
        // Only the filters present: a generic plan over `$n IS NULL OR ...` would not use the
        // `workflow_runs_newest` / `workflow_runs_binding_newest` keyset indexes.
        let mut query = sqlx::QueryBuilder::<Postgres>::new(format!(
            "SELECT {RUN_COLUMNS} FROM workflow_runs WHERE company_id = "
        ));
        query.push_bind(company.as_uuid());
        if let Some(binding) = filter.binding {
            query
                .push(" AND binding_id = ")
                .push_bind(binding.as_uuid());
        }
        if let Some(state) = &filter.state {
            query.push(" AND state = ").push_bind(state.clone());
        }
        if let Some((created, id)) = cursor {
            query
                .push(" AND (created_at, id) < (")
                .push_bind(created)
                .push(", ")
                .push_bind(id)
                .push(")");
        }
        query
            .push(" ORDER BY created_at DESC, id DESC LIMIT ")
            .push_bind(page.size + 1);
        let rows = query
            .build_query_as::<RunRow>()
            .fetch_all(self.pool())
            .await?
            .into_iter()
            .map(run_summary)
            .collect::<AppResult<Vec<_>>>()?;
        Ok(paged(rows, page.size))
    }

    async fn run_summary(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunSummary>> {
        sqlx::query_as::<_, RunRow>(&format!(
            "SELECT {RUN_COLUMNS} FROM workflow_runs WHERE company_id = $1 AND id = $2"
        ))
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .fetch_optional(self.pool())
        .await?
        .map(run_summary)
        .transpose()
    }

    async fn run_detail(&self, company: CompanyId, run: RunId) -> AppResult<Option<RunDetail>> {
        let Some(summary) = self.run_summary(company, run).await? else {
            return Ok(None);
        };
        let executions: Vec<(
            Uuid,
            String,
            i64,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT id, step_id, activation, activated_at, completed_at, committed_route \
                 FROM workflow_executions WHERE company_id = $1 AND run_id = $2 \
                 ORDER BY activation DESC, id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (executions, executions_truncated) = bounded(executions);
        let waits: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            DateTime<Utc>,
            Option<Uuid>,
            Option<Uuid>,
        )> = sqlx::query_as(
            "SELECT wait.id, wait.execution_id, wait.reason, wait.state, wait.deadline, \
                   decision.id, wait.child_run_id \
                 FROM workflow_waits AS wait \
                 LEFT JOIN workflow_decisions AS decision ON decision.company_id = wait.company_id \
                   AND decision.wait_id = wait.id \
                 WHERE wait.company_id = $1 AND wait.run_id = $2 \
                 ORDER BY wait.created_at DESC, wait.id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (waits, waits_truncated) = bounded(waits);
        let (actions, actions_truncated) = self.actions(company, run).await?;
        Ok(Some(RunDetail {
            summary,
            executions: executions
                .into_iter()
                .map(
                    |(execution, step, activation, activated, completed, route)| ExecutionSummary {
                        execution,
                        step,
                        activation,
                        activated_at: activated,
                        completed_at: completed,
                        route,
                    },
                )
                .collect(),
            executions_truncated,
            waits: waits
                .into_iter()
                .map(
                    |(wait, execution, reason, state, deadline, decision, child)| WaitSummary {
                        wait,
                        execution,
                        reason,
                        state,
                        deadline,
                        decision,
                        child_run: child.map(RunId::new),
                    },
                )
                .collect(),
            waits_truncated,
            actions,
            actions_truncated,
        }))
    }

    async fn decision_company(&self, decision: Uuid) -> AppResult<Option<CompanyId>> {
        Ok(
            sqlx::query_scalar::<_, Uuid>(
                "SELECT company_id FROM workflow_decisions WHERE id = $1",
            )
            .bind(decision)
            .fetch_optional(self.pool())
            .await?
            .map(CompanyId::new),
        )
    }
}

type ActionRow = (Uuid, Uuid, Uuid, String, String, i32, DateTime<Utc>, bool);

impl PostgresPersistence {
    /// The run's newest remote and message dispatches (P08-D3 §D3.4): the evidence-conflict
    /// check once per distinct execution, and message destinations in one read.
    async fn actions(
        &self,
        company: CompanyId,
        run: RunId,
    ) -> AppResult<(Vec<ActionSummary>, bool)> {
        let rows: Vec<ActionRow> = sqlx::query_as(
            "SELECT dispatch.id, dispatch.execution_id, dispatch.invocation_id, \
                    dispatch.argument_digest, dispatch.effect_kind, dispatch.attempt_number, \
                    dispatch.created_at, \
                    EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt \
                           WHERE receipt.company_id = dispatch.company_id \
                             AND receipt.invocation_id = dispatch.invocation_id) \
               FROM workflow_action_dispatches AS dispatch \
              WHERE dispatch.company_id = $1 AND dispatch.run_id = $2 \
                AND dispatch.effect_kind IN ('remote', 'message') \
              ORDER BY dispatch.created_at DESC, dispatch.id DESC LIMIT $3",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .bind(MAX_LISTED + 1)
        .fetch_all(self.pool())
        .await?;
        let (rows, truncated) = bounded(rows);
        let executions = rows
            .iter()
            .map(|row| row.1)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let conflicted: std::collections::HashMap<Uuid, bool> = sqlx::query_as(
            "SELECT execution, workflow_action_has_evidence_conflict($1, execution) \
               FROM unnest($2::uuid[]) AS execution",
        )
        .bind(company.as_uuid())
        .bind(&executions)
        .fetch_all(self.pool())
        .await?
        .into_iter()
        .collect();
        let messages = rows
            .iter()
            .filter(|row| row.4 == "message")
            .map(|row| row.2)
            .collect::<Vec<_>>();
        let mut destinations: std::collections::HashMap<Uuid, Vec<DestinationSummary>> =
            std::collections::HashMap::new();
        if !messages.is_empty() {
            let rows: Vec<(Uuid, i16, Uuid, String, i64)> = sqlx::query_as(
                "SELECT target.invocation_id, target.ordinal, delivery.id, delivery.status, \
                        (SELECT count(*) FROM message_delivery_parts AS part \
                          WHERE part.delivery_id = delivery.id) \
                   FROM workflow_message_destinations AS target \
                   JOIN message_deliveries AS delivery \
                     ON delivery.company_id = target.company_id AND delivery.id = target.delivery_id \
                  WHERE target.company_id = $1 AND target.run_id = $2 \
                    AND target.invocation_id = ANY($3) \
                  ORDER BY target.invocation_id, target.ordinal",
            )
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(&messages)
            .fetch_all(self.pool())
            .await?;
            for (invocation, ordinal, delivery, status, parts) in rows {
                destinations
                    .entry(invocation)
                    .or_default()
                    .push(DestinationSummary {
                        ordinal,
                        delivery,
                        status,
                        parts,
                    });
            }
        }
        let actions = rows
            .into_iter()
            .map(
                |(marker, execution, invocation, digest, kind, attempt, created, receipt)| {
                    ActionSummary {
                        marker,
                        execution,
                        invocation,
                        argument_digest: digest,
                        destinations: if kind == "message" {
                            destinations.get(&invocation).cloned().unwrap_or_default()
                        } else {
                            Vec::new()
                        },
                        effect_kind: kind,
                        attempt,
                        created_at: created,
                        has_receipt: receipt,
                        evidence_conflict: conflicted.get(&execution).copied().unwrap_or(false),
                    }
                },
            )
            .collect();
        Ok((actions, truncated))
    }
}
