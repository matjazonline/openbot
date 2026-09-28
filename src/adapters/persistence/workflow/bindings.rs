use super::*;
use binding_rows::{association_columns, read_binding, read_version};

#[derive(Clone, Copy)]
enum Change {
    Configuration,
    Activate,
    Deactivate,
}
impl Change {
    fn event(self) -> &'static str {
        match self {
            Self::Configuration => "configured",
            Self::Activate => "activated",
            Self::Deactivate => "deactivated",
        }
    }
}

#[async_trait]
impl BindingLifecycle for PostgresPersistence {
    async fn binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingState>> {
        read_binding(&mut *self.pool.acquire().await?, company, binding).await
    }

    async fn save_binding(&self, command: PreparedBinding) -> AppResult<()> {
        let state = command.state();
        let configuration = &state.configuration;
        let company = configuration.company_id();
        let mut tx = self.pool.begin().await?;
        authority::authorize_company(&mut tx, company, command.actor()).await?;
        association::authorize_association(&mut tx, company, command.actor(), state.association)
            .await?;
        // All workflow lifecycle mutations serialize on company before their heads.
        let current = read_binding(&mut tx, company, configuration.id()).await?;
        let change = validate_change(&command, current.as_ref())?;
        if !matches!(change, Change::Deactivate) {
            verify_selection(&mut tx, configuration).await?;
            resources::check_readiness(&mut tx, configuration).await?;
        }
        write_head(&mut tx, &command).await?;
        if matches!(change, Change::Configuration) {
            insert_configuration(&mut tx, &command).await?;
        }
        sqlx::query("INSERT INTO workflow_binding_events (company_id, binding_id, state_revision, configuration_revision, event_kind, actor_id) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(company.as_uuid()).bind(configuration.id().as_uuid()).bind(revision(state.revision.get())?)
            .bind(revision(configuration.revision().get())?).bind(change.event()).bind(command.actor().user_id())
            .execute(&mut *tx).await.map_err(write_error)?;
        tx.commit().await?;
        Ok(())
    }
}

fn validate_change(command: &PreparedBinding, current: Option<&BindingState>) -> AppResult<Change> {
    let state = command.state();
    let expected = command.expected().map_or(0, BindingStateRevision::get);
    if state.revision.get() != expected.checked_add(1).ok_or_else(conflict)? {
        return Err(conflict());
    }
    let Some(previous) = current else {
        if command.expected().is_some() || state.configuration.revision().get() != 1 || state.active
        {
            return Err(conflict());
        }
        return Ok(Change::Configuration);
    };
    if command.expected() != Some(previous.revision) || state.association != previous.association {
        return Err(conflict());
    }
    let old = &previous.configuration;
    let new = &state.configuration;
    if new.revision().get() == old.revision().get().checked_add(1).ok_or_else(conflict)? {
        if state.active != previous.active {
            return Err(conflict());
        }
        return Ok(Change::Configuration);
    }
    if new.revision() != old.revision()
        || new.params() != old.params()
        || new.resources() != old.resources()
        || store_bundle(new.bundle())? != store_bundle(old.bundle())?
    {
        return Err(conflict());
    }
    Ok(if state.active {
        Change::Activate
    } else {
        Change::Deactivate
    })
}

pub(super) async fn verify_selection(
    tx: &mut Transaction<'_, Postgres>,
    configuration: &binding::ConfiguredBinding,
) -> AppResult<()> {
    let bundle = configuration.bundle();
    let definition = bundle.compiled().graph().definition();
    let archived: bool = sqlx::query_scalar(
        "SELECT archived FROM workflow_definitions WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(configuration.company_id().as_uuid())
    .bind(definition.workflow_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    if archived {
        return Err(missing());
    }
    let stored = read_version(&mut *tx, configuration.company_id(), definition.version_id).await?;
    if store_bundle(&stored)? != store_bundle(bundle)? {
        return Err(conflict());
    }
    authority::supported_publication(&stored)
}

async fn write_head(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
) -> AppResult<()> {
    let state = command.state();
    let configuration = &state.configuration;
    let (channel, thread) = association_columns(state.association);
    let count = match command.expected() {
        None => sqlx::query("INSERT INTO workflow_bindings (company_id, id, state_revision, configuration_revision, active, channel_id, thread_id) VALUES ($1, $2, $3, $4, $5, $6, $7)")
            .bind(configuration.company_id().as_uuid()).bind(configuration.id().as_uuid()).bind(revision(state.revision.get())?)
            .bind(revision(configuration.revision().get())?).bind(state.active).bind(channel).bind(thread)
            .execute(&mut **tx).await.map_err(write_error)?.rows_affected(),
        Some(expected) => sqlx::query("UPDATE workflow_bindings SET state_revision = $3, configuration_revision = $4, active = $5 WHERE company_id = $1 AND id = $2 AND state_revision = $6")
            .bind(configuration.company_id().as_uuid()).bind(configuration.id().as_uuid()).bind(revision(state.revision.get())?)
            .bind(revision(configuration.revision().get())?).bind(state.active).bind(revision(expected.get())?)
            .execute(&mut **tx).await?.rows_affected(),
    };
    if count != 1 {
        return Err(conflict());
    }
    Ok(())
}

async fn insert_configuration(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
) -> AppResult<()> {
    let configuration = &command.state().configuration;
    sqlx::query("INSERT INTO workflow_binding_revisions (company_id, binding_id, revision, version_id, params, resources, actor_id) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(configuration.company_id().as_uuid()).bind(configuration.id().as_uuid()).bind(revision(configuration.revision().get())?)
        .bind(configuration.bundle().compiled().graph().definition().version_id.as_uuid()).bind(configuration.params())
        .bind(serde_json::to_value(configuration.resources()).map_err(|_| invalid())?).bind(command.actor().user_id())
        .execute(&mut **tx).await.map_err(write_error)?;
    Ok(())
}
