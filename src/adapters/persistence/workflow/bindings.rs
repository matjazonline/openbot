use super::*;
use binding_rows::{read_binding, read_version};

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
        let mut tx = management::begin(
            self,
            command.state().configuration.company_id(),
            command.actor(),
        )
        .await?;
        save_binding_on(&mut tx, &command).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// One binding change on the caller's transaction: company lock and association access, then the
/// change's checks, head, revision and event. `save_binding` and assistant provisioning (p08
/// D2.3) both write through it.
pub(super) async fn save_binding_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
) -> AppResult<()> {
    let state = command.state();
    let configuration = &state.configuration;
    let company = configuration.company_id();
    management::authorize_association_on(tx, company, command.actor(), state.association).await?;
    if let Some(AddressTarget::Principal(principal)) = state.association.target() {
        let agent: bool =
            sqlx::query_scalar("SELECT kind='agent' FROM principals WHERE company_id=$1 AND id=$2")
                .bind(company.as_uuid())
                .bind(principal.as_uuid())
                .fetch_one(&mut **tx)
                .await?;
        if !agent {
            return Err(AppError::BadRequest(
                "Workflow bindings require an agent's direct address".into(),
            ));
        }
    }
    // All workflow lifecycle mutations serialize on company before their heads.
    let current = read_binding(tx, company, configuration.id()).await?;
    let change = validate_change(command, current.as_ref())?;
    // Plan08 02B.4: deactivating stops work and stays allowed on an archived unit's desk;
    // configuring or activating there is refused.
    if !matches!(change, Change::Deactivate)
        && let Some(AddressTarget::Desk(desk)) = state.association.target()
    {
        crate::adapters::persistence::department::require_active_desk_on(tx, company, desk).await?;
    }
    if state.active {
        super::role_binding::check_on(tx, configuration, state.association).await?;
    }
    if !matches!(change, Change::Deactivate) {
        verify_selection(tx, configuration).await?;
        let reach = resources::association_reach_on(tx, company, state.association).await?;
        resources::check_readiness(
            tx,
            resources::ReadinessSelection::of(
                configuration.as_ref(),
                reach,
                crate::app_error::UnitCheckStage::Binding,
            ),
        )
        .await?;
        check_run_as(tx, command, current.as_ref()).await?;
        if state.active && configuration.message_trigger().is_some() {
            check_message_binding_cap(tx, command).await?;
        }
    }
    if current.is_some() && matches!(change, Change::Configuration) {
        schedules::revalidate_schedules(tx, configuration, state.association).await?;
    }
    write_head(tx, command).await?;
    if matches!(change, Change::Configuration) {
        insert_configuration(tx, command).await?;
    }
    let acting =
        management::association_role_on(tx, company, command.actor(), state.association).await?;
    sqlx::query("INSERT INTO workflow_binding_events (company_id, binding_id, state_revision, configuration_revision, event_kind, actor_id, acting_role) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(company.as_uuid()).bind(configuration.id().as_uuid()).bind(revision(state.revision.get())?)
        .bind(revision(configuration.revision().get())?).bind(change.event()).bind(command.actor().user_id())
        .bind(acting.map(|role| role.to_string()))
        .execute(&mut **tx).await.map_err(write_error)?;
    Ok(())
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
        || new.run_as() != old.run_as()
        || new.message_trigger() != old.message_trigger()
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
    authority::supported_publication(tx, &stored, workflow_tools::ToolCheck::Run).await
}

async fn write_head(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
) -> AppResult<()> {
    let state = command.state();
    let configuration = &state.configuration;
    let association = association_columns(state.association);
    let count = match command.expected() {
        None => sqlx::query("INSERT INTO workflow_bindings (company_id, id, state_revision, configuration_revision, active, channel_id, thread_id, principal_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
            .bind(configuration.company_id().as_uuid()).bind(configuration.id().as_uuid()).bind(revision(state.revision.get())?)
            .bind(revision(configuration.revision().get())?).bind(state.active).bind(association.channel_id).bind(association.thread_id).bind(association.principal_id)
            .execute(&mut **tx).await.map_err(write_error)?.rows_affected(),
        // `activated_at` marks when the current activation began; configuring an active binding
        // keeps it, so message eligibility (`activated_at <= accepted`) is unchanged (p08 D1.2).
        Some(expected) => sqlx::query("UPDATE workflow_bindings SET state_revision = $3, configuration_revision = $4, active = $5, activated_at = CASE WHEN $5 THEN COALESCE(activated_at, clock_timestamp()) END WHERE company_id = $1 AND id = $2 AND state_revision = $6")
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
    sqlx::query("INSERT INTO workflow_binding_revisions (company_id, binding_id, revision, version_id, params, resources, actor_id, run_as_user_id, message_trigger, escalation) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)")
        .bind(configuration.company_id().as_uuid()).bind(configuration.id().as_uuid()).bind(revision(configuration.revision().get())?)
        .bind(configuration.bundle().compiled().graph().definition().version_id.as_uuid()).bind(configuration.params())
        .bind(serde_json::to_value(configuration.resources()).map_err(|_| invalid())?).bind(command.actor().user_id())
        .bind(configuration.run_as().user_id()).bind(configuration.message_trigger().map(MessageTrigger::as_str))
        .bind(configuration.escalation().map(serde_json::to_value).transpose().map_err(|_| invalid())?)
        .execute(&mut **tx).await.map_err(write_error)?;
    Ok(())
}

/// The run-as must be a current owner/admin who can see the association, and only the company
/// owner may name somebody other than themselves; keeping the stored run-as is not a choice
/// (p08 D1.1, the legacy schedule attribution rule).
async fn check_run_as(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
    current: Option<&BindingState>,
) -> AppResult<()> {
    let state = command.state();
    let company = state.configuration.company_id();
    let run_as = state.configuration.run_as();
    let refused = |error: AppError| match error {
        AppError::NotFound(_) => {
            AppError::BadRequest("The binding's run-as must currently manage its unit".into())
        }
        other => other,
    };
    management::authorize_association_on(&mut *tx, company, run_as, state.association)
        .await
        .map_err(refused)?;
    let chosen = current.is_none_or(|previous| previous.configuration.run_as() != run_as);
    if !chosen || run_as == command.actor() {
        return Ok(());
    }
    let owner: bool = sqlx::query_scalar("SELECT user_id = $2 FROM companies WHERE id = $1")
        .bind(company.as_uuid())
        .bind(command.actor().user_id())
        .fetch_one(&mut **tx)
        .await?;
    if !owner {
        return Err(AppError::BadRequest(
            "Only the company owner may make a binding run as somebody else".into(),
        ));
    }
    Ok(())
}

/// One channel admits at most `MAX_MESSAGE_BINDINGS_PER_CHANNEL` message-triggered bindings at
/// once, checked for every change that leaves one active (p08 D1.2). The channel lock follows
/// the company lock `authorize_company` already holds.
async fn check_message_binding_cap(
    tx: &mut Transaction<'_, Postgres>,
    command: &PreparedBinding,
) -> AppResult<()> {
    let state = command.state();
    let company = state.configuration.company_id();
    let Some(target) = state.association.target() else {
        return Err(AppError::BadRequest(
            "A message trigger needs an inbox or thread association".into(),
        ));
    };
    let association = association_columns(state.association);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('workflow_message_bindings:' || $1::text || ':' || $2, 0))")
        .bind(company.as_uuid()).bind(target.to_string()).execute(&mut **tx).await?;
    let others: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
           AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
         WHERE binding.company_id = $1 AND binding.channel_id IS NOT DISTINCT FROM $2::uuid AND binding.active \
           AND configuration.message_trigger IS NOT NULL AND binding.id <> $3 \
           AND binding.principal_id IS NOT DISTINCT FROM $4::uuid",
    )
    .bind(company.as_uuid())
    .bind(association.channel_id)
    .bind(state.configuration.id().as_uuid())
    .bind(association.principal_id)
    .fetch_one(&mut **tx)
    .await?;
    if others >= MAX_MESSAGE_BINDINGS_PER_CHANNEL {
        return Err(AppError::BadRequest(format!(
            "A channel can have at most {MAX_MESSAGE_BINDINGS_PER_CHANNEL} active message-triggered bindings"
        )));
    }
    Ok(())
}
