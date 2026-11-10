//! The department desk (plan 01-departments 03B; 01DEP-P03-D8, D9): created with its department,
//! and answered by whoever leads it -- an agent lead through the department-owned autonomous
//! binding, a person lead by reading it as a plain team desk.
//!
//! `workflow_provisioning_commands (company, 'department:{id}')` records the desk and the binding
//! the department owns (`result.channel.id`, `result.binding_id`). A lead change touches only that
//! binding, and only the actor's own account is authorized or locked, so the department write can
//! run this after its grant sync (D9 "Where").
use super::*;
use crate::adapters::persistence::agent_publication::publish_initial_on;
use crate::adapters::persistence::company_agent::refusal_error;
use crate::adapters::persistence::department_desks::{
    prelock_ownership_on, set_desk_ownership_on, sync_desk_grants_on,
};
use crate::application::agent::AgentPublicationScope;
use crate::application::ports::channel::ChannelOwnershipWrite;
use crate::application::workflow::channel_reply::{
    ChannelReplySettings, ConfigureChannelReply, ConfigureResponder,
};
use crate::application::workflow::compiler::agent_reference_steps;
use crate::entities::agent_capability::AgentId;
use crate::entities::department::{Department, DepartmentId};
use crate::entities::transport::PrincipalId;

/// The command key that records a department's desk and the binding it owns.
pub(crate) fn department_desk_key(department: DepartmentId) -> String {
    format!("department:{department}")
}

fn owned_by(department: DepartmentId) -> ChannelOwnershipWrite {
    ChannelOwnershipWrite {
        department_id: Some(Some(department)),
        members_only: Some(false),
    }
}

/// The agent a lead principal is, when it is one: its id and slug.
async fn lead_agent_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    lead: Option<PrincipalId>,
) -> AppResult<Option<(Uuid, String)>> {
    let Some(lead) = lead else {
        return Ok(None);
    };
    Ok(sqlx::query_as(
        "SELECT agent.id, agent.slug::text FROM principals AS principal \
         JOIN agents AS agent ON agent.company_id = principal.company_id \
          AND agent.id = principal.agent_id \
         WHERE principal.company_id = $1 AND principal.id = $2 AND principal.kind = 'agent'",
    )
    .bind(company.as_uuid())
    .bind(lead.as_uuid())
    .fetch_optional(&mut **tx)
    .await?)
}

/// D8: the new department's desk at its slug and name, owned by it and open to the company. With
/// an agent lead, the autonomous workflow answers it on that agent; otherwise it is a plain team
/// desk. The caller holds the company write lock and has just inserted `department`; its lead
/// role is written after this. A taken address is a conflict and the caller rolls back.
pub(crate) async fn provision_department_desk_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: &Department,
    lead: Option<PrincipalId>,
) -> AppResult<()> {
    let Some((_, agent_slug)) = lead_agent_on(tx, company, lead).await? else {
        return plain_desk_on(tx, templates, company, actor, department).await;
    };
    // Boxed: the desk descends into template compilation and workflow publication.
    let created = Box::pin(assistant_channel_on(
        tx,
        templates,
        company,
        actor,
        AssistantChannelRequest {
            agent_slug,
            channel_slug: department.slug.to_string(),
            channel_name: department.name.clone(),
            reply_mode: ReplyMode::Autonomous,
            command_key: department_desk_key(department.id),
        },
    ))
    .await?
    .map_err(refusal_error)?;
    let desk = created["channel"]["id"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| AppError::Internal("A department desk result has no channel id".into()))?;
    set_desk_ownership_on(tx, company, desk, &owned_by(department.id)).await
}

/// A plain team desk through the owned-desk create sequence `ChannelPersistence::create` uses,
/// recorded under `department:{id}` with no binding.
async fn plain_desk_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: &Department,
) -> AppResult<()> {
    let slug = department.slug.to_string();
    if let Some(ProvisioningRefusal::Conflict(message)) = taken_slug(tx, company, &slug).await? {
        return Err(AppError::Conflict(message));
    }
    let (add_3rd_party, company_slug): (bool, String) =
        sqlx::query_as("SELECT default_add_3rd_party, slug::text FROM companies WHERE id = $1")
            .bind(company.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    let ownership = owned_by(department.id);
    let mut write = ChannelWrite {
        name: department.name.clone(),
        slug,
        enabled: true,
        add_3rd_party,
        created_by: Some(CreationProvenance::user(actor.user_id())),
        ownership: Some(ownership),
        ..ChannelWrite::default()
    };
    write.normalize()?;
    let desk = Uuid::new_v4();
    prelock_ownership_on(tx, company, None, &ownership).await?;
    // In a savepoint: a desk created meanwhile by a writer that takes no company lock claims the
    // address with a unique violation, which is then the conflict (as `provision_guarded`).
    let mut savepoint = sqlx::Acquire::begin(&mut **tx).await?;
    match insert_plain_desk_on(&mut savepoint, company, desk, &write).await {
        Ok(()) => savepoint.commit().await?,
        Err(error) => {
            savepoint.rollback().await?;
            return Err(match taken_slug(tx, company, &write.slug).await? {
                Some(ProvisioningRefusal::Conflict(message)) => AppError::Conflict(message),
                _ => error,
            });
        }
    }
    let address = crate::entities::channel::Channel::address_for(
        &crate::entities::value_objects::ChannelSlug::new(write.slug.clone()),
        &crate::entities::value_objects::CompanySlug::from(company_slug),
        &templates.app_domain,
    )
    .to_string();
    let digest = format!(
        "{:x}",
        Sha256::digest(json!({"department": department.id, "desk": desk}).to_string())
    );
    sqlx::query(
        "INSERT INTO workflow_provisioning_commands \
         (company_id, command_key, request_digest, actor_id, result) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(company.as_uuid())
    .bind(department_desk_key(department.id))
    .bind(digest)
    .bind(actor.user_id())
    .bind(json!({
        "channel": {"id": desk.to_string(), "slug": write.slug, "address": address},
        "binding_id": null,
    }))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_plain_desk_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    desk: Uuid,
    write: &ChannelWrite,
) -> AppResult<()> {
    insert_channel_row_on(tx, company.as_uuid(), desk, write).await?;
    insert_channel_slugs(tx, company.as_uuid(), desk, &write.slug, &write.alias_slugs).await?;
    write_channel_email_binding(tx, company.as_uuid(), desk, write).await?;
    sync_desk_grants_on(tx, company, desk).await
}

/// The department's recorded desk and the binding it owns, while the desk still belongs to it.
struct DepartmentDesk {
    channel: RelatedChannelId,
    slug: String,
    binding: Option<WorkflowBindingId>,
}

async fn department_desk_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    department: DepartmentId,
) -> AppResult<Option<DepartmentDesk>> {
    let record: Option<Value> = sqlx::query_scalar(
        "SELECT result FROM workflow_provisioning_commands \
         WHERE company_id = $1 AND command_key = $2",
    )
    .bind(company.as_uuid())
    .bind(department_desk_key(department))
    .fetch_optional(&mut **tx)
    .await?;
    let Some(record) = record else {
        return Ok(None);
    };
    let id = |value: &Value| value.as_str().and_then(|id| Uuid::parse_str(id).ok());
    let desk = id(&record["channel"]["id"])
        .ok_or_else(|| AppError::Internal("A department desk record has no channel id".into()))?;
    let owner: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT department_id FROM channels WHERE company_id = $1 AND id = $2")
            .bind(company.as_uuid())
            .bind(desk)
            .fetch_optional(&mut **tx)
            .await?;
    if owner.flatten() != Some(department.as_uuid()) {
        return Ok(None);
    }
    Ok(Some(DepartmentDesk {
        channel: RelatedChannelId::new(desk),
        slug: record["channel"]["slug"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        binding: id(&record["binding_id"]).map(WorkflowBindingId::new),
    }))
}

/// D9: after a lead write, the department desk's owned binding follows the lead. An agent lead
/// gets it re-pointed, reconfigured to run as `actor`, activated and selected, or provisioned on
/// a plain desk; a person lead or none gets it paused and, when selected, unselected. A responder
/// a user chose, a detached desk, or a binding already in the desired state changes nothing. The
/// caller holds the company write lock and has synced the department's grants.
pub(crate) async fn follow_department_lead_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    lead: Option<PrincipalId>,
) -> AppResult<()> {
    let Some(desk) = department_desk_on(tx, company, department).await? else {
        return Ok(());
    };
    let settings = super::channel_reply::settings_on(tx, company, desk.channel).await?;
    let selected = settings
        .as_ref()
        .and_then(|settings| settings.responder.as_ref());
    let owned = match desk.binding {
        Some(binding) => super::binding_rows::read_binding(tx, company, binding).await?,
        None => None,
    };
    let Some((agent, slug)) = lead_agent_on(tx, company, lead).await? else {
        // A person lead, or none: the owned binding stops answering.
        let Some(state) = owned.filter(|state| state.active) else {
            return Ok(());
        };
        guard_on(tx, company, actor, &desk, settings.as_ref()).await?;
        // In the gate's order: the binding rows, then the desk row and its settings.
        let paused = provisioned_activation(actor, &state, false).map_err(lifecycle_error)?;
        super::bindings::save_binding_on(tx, &paused).await?;
        if let Some(settings) = &settings
            && selected.is_some_and(|selected| selected.binding == state.configuration.id())
        {
            super::channel_reply::configure_reply_on(
                tx,
                &ConfigureChannelReply {
                    company,
                    channel: desk.channel,
                    actor,
                    expected: Some(settings.revision),
                    responder: None,
                    approval_policy: None,
                },
            )
            .await?;
        }
        return Ok(());
    };
    if selected.is_some_and(|selected| Some(selected.binding) != desk.binding) {
        return Ok(()); // a user chose who answers this desk
    }
    let Some(state) = owned else {
        return provision_on_desk_on(
            tx, templates, company, actor, department, &desk, &settings, &slug, agent,
        )
        .await;
    };
    let runs = runs_agent(&state, &slug);
    let current = selected
        .is_some_and(|selected| selected.configuration_revision == state.configuration.revision());
    if state.active && current && runs {
        return Ok(());
    }
    guard_on(tx, company, actor, &desk, settings.as_ref()).await?;
    let mut state = if runs {
        state
    } else {
        Box::pin(repoint_on(
            tx, templates, company, actor, &state, &slug, agent,
        ))
        .await?
    };
    if state.configuration.run_as() != actor {
        let bundle = state.configuration.bundle().clone();
        let change = provisioned_reconfiguration(actor, &state, bundle).map_err(lifecycle_error)?;
        super::bindings::save_binding_on(tx, &change).await?;
        state = change.state().clone();
    }
    if !state.active {
        let change = provisioned_activation(actor, &state, true).map_err(lifecycle_error)?;
        super::bindings::save_binding_on(tx, &change).await?;
        state = change.state().clone();
    }
    select_on(tx, company, actor, &desk, settings.as_ref(), &state).await
}

/// Whether the binding's version runs `slug` in its reply step.
fn runs_agent(state: &BindingState, slug: &str) -> bool {
    agent_reference_steps(
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
        state.configuration.bundle().compiled().source(),
    )
    .is_ok_and(|steps| {
        steps.iter().any(|step| {
            step.id.as_str() == REPLY_AGENT_STEP
                && step.key.as_ref().is_some_and(|key| key.as_str() == slug)
        })
    })
}

/// The refusals that keep a lead change from acting on a desk it should not (D9 "Run-as and
/// refusals"): an approval policy (its reviewers would be authorized under these locks), and a
/// desk the actor cannot see.
async fn guard_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    desk: &DepartmentDesk,
    settings: Option<&ChannelReplySettings>,
) -> AppResult<()> {
    if settings.is_some_and(|settings| settings.approval_policy.is_some()) {
        return Err(AppError::BadRequest(
            "The department desk asks reviewers to approve replies; remove that policy or detach the desk before changing the lead.".into(),
        ));
    }
    match super::association::authorize_association(
        tx,
        company,
        actor,
        RelatedAssociation::Channel(desk.channel),
    )
    .await
    {
        Err(AppError::NotFound(_)) => Err(AppError::BadRequest(format!(
            "You cannot see the department desk '{}', so its replies cannot follow the new lead.",
            desk.slug
        ))),
        other => other,
    }
}

/// The owned binding's bound version re-targeted to `slug`: a new draft revision of exactly that
/// source, a new published version, and the binding reconfigured onto it as `actor`.
async fn repoint_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    state: &BindingState,
    slug: &str,
    agent: Uuid,
) -> AppResult<BindingState> {
    let bound = state.configuration.bundle();
    let definition = bound.compiled().graph().definition();
    if !definition.resources.is_empty() {
        return Err(AppError::BadRequest(
            "The department desk's workflow uses resources; change its lead in the workflow editor or detach the desk first.".into(),
        ));
    }
    let workflow = definition.workflow_id;
    let bound_source = bound.compiled().source();
    let current = super::definitions::read_draft(tx, company, workflow)
        .await?
        .ok_or_else(missing)?;
    if current.archived || current.draft.source() != bound_source {
        return Err(AppError::BadRequest(
            "The department desk's workflow has edits its desk does not run; publish them and bind the desk to the new version, or restore the bound version's content, before changing the lead.".into(),
        ));
    }
    let decoder = crate::adapters::workflow_source::WorkflowSourceDecoder;
    let source = retarget_agent(&decoder, bound_source, REPLY_AGENT_STEP, slug).map_err(|_| {
        AppError::BadRequest(
            "The department desk's workflow has no agent reply step to hand to the new lead; edit its workflow or detach the desk first.".into(),
        )
    })?;
    publish_initial_on(
        tx,
        AgentPublicationScope {
            company,
            actor,
            agent: AgentId::new(agent),
        },
    )
    .await?;
    let previous = current.draft;
    let draft = CompanyWorkflowDraft::saved(
        company,
        workflow,
        previous
            .revision()
            .get()
            .checked_add(1)
            .and_then(DraftRevision::new)
            .ok_or_else(missing)?,
        DraftContent {
            title: previous.title().into(),
            description: previous.description().into(),
            source,
        },
        previous.origin().cloned(),
    );
    super::definitions::save_draft_on(tx, &draft, actor, Some(previous.revision()), false).await?;
    let version = VersionId::new(Uuid::new_v4());
    let bundle = match compiled(tx, templates, &decoder, &draft, version).await? {
        Ok(bundle) => Arc::new(bundle),
        Err(refusal) => return Err(refusal_error(refusal)),
    };
    let publication = provisioned_publication(
        PublishDraft {
            target: DefinitionTarget {
                company,
                actor,
                workflow,
            },
            expected: draft.revision(),
            version,
            key: IdempotencyKey::parse(format!("department-lead:{}", version.as_uuid()))?,
        },
        draft,
        bundle,
    )
    .map_err(lifecycle_error)?;
    let published = super::definitions::publish_on(tx, &publication).await?;
    let change = provisioned_reconfiguration(actor, state, published).map_err(lifecycle_error)?;
    super::bindings::save_binding_on(tx, &change).await?;
    Ok(change.state().clone())
}

/// A plain department desk gets the autonomous workflow and binding on its new agent lead,
/// selected, and recorded as the department's.
#[allow(clippy::too_many_arguments)] // One transaction's whole context; split it and it drifts.
async fn provision_on_desk_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    department: DepartmentId,
    desk: &DepartmentDesk,
    settings: &Option<ChannelReplySettings>,
    slug: &str,
    agent: Uuid,
) -> AppResult<()> {
    guard_on(tx, company, actor, desk, settings.as_ref()).await?;
    publish_initial_on(
        tx,
        AgentPublicationScope {
            company,
            actor,
            agent: AgentId::new(agent),
        },
    )
    .await?;
    let provisioning = Provisioning {
        company,
        requester: actor,
        agent_slug: slug.to_owned(),
        channel_slug: desk.slug.clone(),
        channel_name: String::new(),
        reply_mode: ReplyMode::Autonomous,
        steps: vec![],
        key: ProvisioningKey::command(company, &department_desk_key(department)),
    };
    let native = Box::pin(super::native::provision_workflow_on(
        tx,
        templates,
        &provisioning,
        slug,
        desk.channel.as_uuid(),
        settings.is_none(),
    ))
    .await?
    .map_err(refusal_error)?;
    if settings.is_some() {
        let state = super::binding_rows::read_binding(tx, company, native.binding)
            .await?
            .ok_or_else(missing)?;
        select_on(tx, company, actor, desk, settings.as_ref(), &state).await?;
    }
    sqlx::query(
        "UPDATE workflow_provisioning_commands \
            SET result = jsonb_set(result, '{binding_id}', to_jsonb($3::text)) \
          WHERE company_id = $1 AND command_key = $2",
    )
    .bind(company.as_uuid())
    .bind(department_desk_key(department))
    .bind(native.binding.as_uuid().to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Select `state`'s binding as the desk's responder: the first settings when there are none,
/// else the next revision, keeping the selected contract or the native one.
async fn select_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    desk: &DepartmentDesk,
    settings: Option<&ChannelReplySettings>,
    state: &BindingState,
) -> AppResult<()> {
    let Some(settings) = settings else {
        return super::channel_reply::bootstrap_native_on(
            tx,
            company,
            desk.channel,
            actor,
            state.configuration.id(),
        )
        .await;
    };
    super::channel_reply::configure_reply_on(
        tx,
        &ConfigureChannelReply {
            company,
            channel: desk.channel,
            actor,
            expected: Some(settings.revision),
            responder: Some(ConfigureResponder {
                binding: state.configuration.id(),
                expected_binding_revision: state.revision,
                contract: settings
                    .responder
                    .as_ref()
                    .map_or_else(super::channel_reply::native_contract, |selected| {
                        selected.contract.clone()
                    }),
            }),
            approval_policy: None,
        },
    )
    .await?;
    Ok(())
}
