use super::*;
use binding::{BindingConfiguration, ConfiguredBinding};
use serde_json::{Value, json};

/// Versioned, unambiguous logical source identity. Trigger UUID is deliberately
/// excluded for externally identified events, allowing redelivery command aliases.
pub(super) struct SourceKey(String);
impl SourceKey {
    pub(super) fn from_trigger(trigger: &TriggerRef) -> Self {
        let source = match trigger.source() {
            TriggerSource::Manual => json!(["manual", trigger.trigger_id().as_uuid()]),
            TriggerSource::Message { message_id } => json!(["message", message_id.as_uuid()]),
            TriggerSource::Schedule {
                schedule_id,
                occurrence_id,
            } => {
                json!(["schedule", schedule_id.as_uuid(), occurrence_id.as_uuid()])
            }
            TriggerSource::Child { parent } => {
                let execution = parent.execution();
                let action = match parent {
                    ChildCause::Execution(_) => None,
                    ChildCause::Action(action) => Some(action.action_id().as_uuid()),
                };
                json!([
                    "child",
                    execution.run_id().as_uuid(),
                    execution.execution_id().as_uuid(),
                    execution.step_id().as_str(),
                    action
                ])
            }
        };
        Self(format!("v1:{source}"))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

#[async_trait]
impl WorkflowBindings for PostgresPersistence {
    async fn admission_binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
        key: &IdempotencyKey,
        trigger: &TriggerRef,
    ) -> AppResult<Option<Arc<ConfiguredBinding>>> {
        if trigger.company_id() != company {
            return Err(conflict());
        }
        let source = SourceKey::from_trigger(trigger);
        let mut tx = self.pool.begin().await?;
        // The command/source lookup, saved snapshot, and fallback head/version
        // must describe one committed snapshot. This read grants no authority;
        // the admission writer must reauthorize and close selection races.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *tx)
            .await?;
        let saved = saved_run(&mut tx, company, binding, key, trigger, &source).await?;
        let selected = if let Some(run) = saved {
            Some(read_saved_binding(&mut tx, company, binding, run).await?)
        } else {
            current_binding(&mut tx, company, binding).await?
        };
        tx.commit().await?;
        Ok(selected)
    }
}

#[derive(sqlx::FromRow)]
struct CommandRow {
    run_id: Uuid,
    binding_id: Uuid,
    source_key: String,
    trigger_id: Uuid,
}

async fn saved_run(
    db: &mut PgConnection,
    company: CompanyId,
    binding: WorkflowBindingId,
    key: &IdempotencyKey,
    trigger: &TriggerRef,
    source: &SourceKey,
) -> AppResult<Option<Uuid>> {
    let command = sqlx::query_as::<_, CommandRow>(
        "SELECT command.run_id, admission.binding_id, admission.source_key, command.trigger_id \
         FROM workflow_admission_commands AS command JOIN workflow_admissions AS admission \
           ON admission.company_id = command.company_id AND admission.run_id = command.run_id \
         WHERE command.company_id = $1 AND command.command_key = $2",
    )
    .bind(company.as_uuid())
    .bind(key.as_str())
    .fetch_optional(&mut *db)
    .await?;
    if let Some(command) = command {
        if command.binding_id != binding.as_uuid()
            || command.source_key != source.as_str()
            || command.trigger_id != trigger.trigger_id().as_uuid()
        {
            return Err(conflict());
        }
        return Ok(Some(command.run_id));
    }
    Ok(sqlx::query_scalar(
        "SELECT run_id FROM workflow_admissions \
         WHERE company_id = $1 AND binding_id = $2 AND source_key = $3",
    )
    .bind(company.as_uuid())
    .bind(binding.as_uuid())
    .bind(source.as_str())
    .fetch_optional(db)
    .await?)
}

async fn current_binding(
    db: &mut PgConnection,
    company: CompanyId,
    binding: WorkflowBindingId,
) -> AppResult<Option<Arc<ConfiguredBinding>>> {
    let Some(state) = binding_rows::read_binding(db, company, binding).await? else {
        return Ok(None);
    };
    if !state.active {
        return Ok(None);
    }
    let definition = state.configuration.bundle().compiled().graph().definition();
    let selectable: bool = sqlx::query_scalar(
        "SELECT NOT archived FROM workflow_definitions WHERE company_id = $1 AND id = $2",
    )
    .bind(company.as_uuid())
    .bind(definition.workflow_id.as_uuid())
    .fetch_one(db)
    .await?;
    Ok(selectable.then_some(state.configuration))
}

#[derive(sqlx::FromRow)]
struct SavedBindingRow {
    workflow_id: Uuid,
    version_id: Uuid,
    binding_revision: i64,
    bundle: Vec<u8>,
    params: Value,
    resources: Value,
}

pub(super) async fn read_saved_binding(
    db: &mut PgConnection,
    company: CompanyId,
    binding: WorkflowBindingId,
    run: Uuid,
) -> AppResult<Arc<ConfiguredBinding>> {
    let row = sqlx::query_as::<_, SavedBindingRow>(
        "SELECT workflow_id, version_id, binding_revision, bundle, params, resources \
         FROM workflow_runs WHERE company_id = $1 AND id = $2 AND binding_id = $3",
    )
    .bind(company.as_uuid())
    .bind(run)
    .bind(binding.as_uuid())
    .fetch_optional(db)
    .await?
    .ok_or_else(invalid)?;
    let bundle = restore_bundle(
        &row.bundle,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )?;
    let definition = bundle.compiled().graph().definition();
    if bundle.company_id() != company
        || definition.workflow_id.as_uuid() != row.workflow_id
        || definition.version_id.as_uuid() != row.version_id
    {
        return Err(invalid());
    }
    let revision =
        BindingRevision::new(u64::try_from(row.binding_revision).map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
    ConfiguredBinding::new(
        BindingConfiguration {
            company_id: company,
            id: binding,
            revision,
            params: row.params,
            resources: serde_json::from_value(row.resources).map_err(|_| invalid())?,
        },
        bundle,
    )
    .map(Arc::new)
    .map_err(|_| invalid())
}
