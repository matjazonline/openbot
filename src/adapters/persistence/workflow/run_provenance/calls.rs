use super::*;
use crate::application::agent::AgentToolOrigin;
use crate::application::workflow::run_views::{CallProvenance, CallProvenancePage, CallRecordKind};
use std::collections::{BTreeSet, HashMap};

#[derive(sqlx::FromRow)]
struct OriginRow {
    record: Uuid,
    kind: String,
    origin: serde_json::Value,
}

// Inputs come only from the already bounded action/receipt and child lists. Project metadata
// out of the saved checkpoint in SQL, without loading arguments, results or provider messages.
const ORIGINS: &str = "WITH origins AS ( \
    SELECT intent.id AS record, 'action'::text AS kind, (intent.operation->'context'->'origin') - 'kind' AS origin \
    FROM workflow_action_intents AS intent \
    WHERE intent.company_id=$1 AND intent.run_id=$2 AND intent.id=ANY($3) AND intent.origin_kind='agent' \
    UNION ALL \
    SELECT (invocation.value->'call'->>'invocation_id')::uuid AS record, 'child'::text AS kind, tool.value->'origin' AS origin \
    FROM task_harness_runs AS harness \
    CROSS JOIN LATERAL jsonb_array_elements(harness.checkpoint->'invocations') AS invocation(value) \
    JOIN LATERAL jsonb_array_elements(harness.workflow_prepared->'tools') AS tool(value) \
      ON tool.value->>'declared'=invocation.value->'call'->>'tool_id' \
    WHERE harness.company_id=$1 AND harness.workflow_run_id=$2 \
      AND harness.workflow_execution_id=ANY($6) \
      AND invocation.value->'call'->>'invocation_id'=ANY($4) \
    ) SELECT record, kind, origin FROM origins ORDER BY kind, record LIMIT $5";

impl PostgresPersistence {
    pub(in crate::adapters::persistence::workflow) async fn call_provenance(
        &self,
        company: CompanyId,
        run: RunId,
        actions: &[CallRecordScope],
        children: &[CallRecordScope],
    ) -> AppResult<CallProvenancePage> {
        if actions.is_empty() && children.is_empty() {
            return Ok(CallProvenancePage::default());
        }
        // Each source is an existing bounded run list; reject misuse of this internal reader.
        if actions.len() > crate::application::workflow::reads::MAX_LISTED as usize
            || children.len() > crate::application::workflow::reads::MAX_LISTED as usize
        {
            return Err(invalid_provenance());
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            let child_ids: Vec<_> = children
                .iter()
                .map(|child| child.record.to_string())
                .collect();
            let action_ids: Vec<_> = actions.iter().map(|action| action.record).collect();
            let executions: Vec<_> = actions
                .iter()
                .chain(children)
                .map(|entry| entry.execution)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let mut rows: Vec<OriginRow> = sqlx::query_as(ORIGINS)
                .bind(company.as_uuid())
                .bind(run.as_uuid())
                .bind(&action_ids)
                .bind(&child_ids)
                .bind(MAX_STEP_ITEMS + 1)
                .bind(&executions)
                .fetch_all(self.pool())
                .await?;
            let truncated = rows.len() as i64 > MAX_STEP_ITEMS;
            rows.truncate(MAX_STEP_ITEMS as usize);
            let origins = rows
                .into_iter()
                .map(|row| {
                    let origin: AgentToolOrigin =
                        serde_json::from_value(row.origin).map_err(|_| invalid_provenance())?;
                    let kind = match row.kind.as_str() {
                        "action" => CallRecordKind::ActionInvocation,
                        "child" => CallRecordKind::ChildToolCall,
                        _ => return Err(invalid_provenance()),
                    };
                    Ok(SelectedOrigin {
                        record: row.record,
                        kind,
                        origin,
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            let definitions = self.call_definitions(company, run, &origins).await?;
            project_calls(
                origins,
                definitions,
                &children
                    .iter()
                    .map(|child| child.record)
                    .collect::<Vec<_>>(),
                truncated,
            )
        })
        .await
        .map_err(|_| invalid_provenance())?
    }

    async fn call_definitions(
        &self,
        company: CompanyId,
        run: RunId,
        origins: &[SelectedOrigin],
    ) -> AppResult<HashMap<AgentRevisionId, SelectedAgentSummary>> {
        let revisions: Vec<_> = origins
            .iter()
            .map(|entry| entry.origin.revision.as_uuid())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if revisions.is_empty() {
            return Ok(HashMap::new());
        }
        let sql = format!(
            "{REVISIONS} AND selection.revision_id=ANY($4) ORDER BY selection.agent_id LIMIT $5"
        );
        let rows: Vec<RevisionRow> = sqlx::query_as(&sql)
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .bind(crate::application::agent::MAX_AGENT_DEFINITION_BYTES as i32)
            .bind(&revisions)
            .bind(MAX_STEP_ITEMS + 1)
            .fetch_all(self.pool())
            .await?;
        if rows.len() != revisions.len() {
            return Err(invalid_provenance());
        }
        let mut definitions = HashMap::with_capacity(rows.len());
        for row in rows {
            let summary = row.summary(company)?;
            definitions.insert(summary.revision_id, summary);
            tokio::task::yield_now().await;
        }
        Ok(definitions)
    }
}

struct SelectedOrigin {
    record: Uuid,
    kind: CallRecordKind,
    origin: AgentToolOrigin,
}

fn project_calls(
    origins: Vec<SelectedOrigin>,
    definitions: HashMap<AgentRevisionId, SelectedAgentSummary>,
    children: &[Uuid],
    truncated: bool,
) -> AppResult<CallProvenancePage> {
    let mut identities = BTreeSet::new();
    let items = origins
        .into_iter()
        .map(|entry| {
            if !identities.insert((entry.kind, entry.record)) {
                return Err(invalid_provenance());
            }
            let agent = definitions
                .get(&entry.origin.revision)
                .ok_or_else(invalid_provenance)?;
            if agent.agent_id != entry.origin.agent || agent.manifest != entry.origin.manifest {
                return Err(invalid_provenance());
            }
            let tool = agent
                .bindings
                .iter()
                .find(|tool| tool.binding == entry.origin.binding)
                .ok_or_else(invalid_provenance)?
                .clone();
            Ok(CallProvenance {
                kind: entry.kind,
                record: entry.record,
                origin: entry.origin,
                tool,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    if !truncated
        && children
            .iter()
            .any(|child| !identities.contains(&(CallRecordKind::ChildToolCall, *child)))
    {
        return Err(invalid_provenance());
    }
    Ok(CallProvenancePage { items, truncated })
}
