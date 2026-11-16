//! Fresh native workflow, binding, and responder in the existing provisioning transaction.
use super::*;
use crate::entities::address::AddressTarget;
use crate::entities::transport::PrincipalId;

pub(super) enum WorkflowTarget {
    NewDesk(Uuid),
    ExistingDesk(Uuid),
    Agent { agent: Uuid, principal: PrincipalId },
}

impl WorkflowTarget {
    fn association(&self) -> RelatedAssociation {
        match *self {
            Self::NewDesk(id) | Self::ExistingDesk(id) => {
                RelatedAssociation::Channel(RelatedChannelId::new(id))
            }
            Self::Agent { principal, .. } => RelatedAssociation::Principal(principal),
        }
    }

    fn selection(&self) -> Option<AddressTarget> {
        match *self {
            Self::NewDesk(id) => Some(AddressTarget::Desk(id)),
            Self::ExistingDesk(_) => None,
            Self::Agent { principal, .. } => Some(AddressTarget::Principal(principal)),
        }
    }

    fn owner(&self) -> Option<Uuid> {
        match *self {
            Self::Agent { agent, .. } => Some(agent),
            Self::NewDesk(_) | Self::ExistingDesk(_) => None,
        }
    }
}

pub(super) struct WorkflowProvisioning<'a> {
    pub company: CompanyId,
    pub requester: WorkflowActor,
    pub agent_slug: &'a str,
    pub reply_mode: ReplyMode,
    pub steps: &'a [CustomStep],
    pub key: &'a ProvisioningKey,
}

impl Provisioning {
    pub(super) fn workflow<'a>(&'a self, agent_slug: &'a str) -> WorkflowProvisioning<'a> {
        WorkflowProvisioning {
            company: self.company,
            requester: self.requester,
            agent_slug,
            reply_mode: self.reply_mode,
            steps: &self.steps,
            key: &self.key,
        }
    }
}

pub(crate) struct ProvisionedAssistant {
    pub(super) agent_id: Uuid,
    pub(super) agent_slug: String,
    pub(super) channel_id: Uuid,
    pub(super) channel_slug: String,
    pub(super) address: String,
    pub(super) workflow: WorkflowId,
    pub(super) version: VersionId,
    pub(super) binding: WorkflowBindingId,
    pub(super) reply_mode: ReplyMode,
}
impl ProvisionedAssistant {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "agent": {"id": self.agent_id.to_string(), "slug": self.agent_slug},
            "channel": {"id": self.channel_id.to_string(), "slug": self.channel_slug,
                "address": self.address},
            "workflow_id": self.workflow.as_uuid().to_string(),
            "version_id": self.version.as_uuid().to_string(),
            "binding_id": self.binding.as_uuid().to_string(),
            "reply_mode": self.reply_mode.as_str(),
        })
    }
}

pub(crate) struct NativeWorkflow {
    pub workflow: WorkflowId,
    pub version: VersionId,
    pub binding: WorkflowBindingId,
}

pub(super) async fn provision_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    provisioning: &Provisioning,
) -> AppResult<Provisioned<ProvisionedAssistant>> {
    let company = provisioning.company;
    let requester = provisioning.requester;
    let key = &provisioning.key;
    let created_by = CreationProvenance::user(requester.user_id());

    // 1. Pre-checks under the company lock.
    if let Some(refusal) = taken_slug(tx, company, &provisioning.channel_slug).await? {
        return Ok(Err(refusal));
    }

    let agent = match provision_agent_on(tx, provisioning).await? {
        Ok(agent) => agent,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let agent_id = agent.id.as_uuid();
    let agent_slug = agent.slug.0;

    // 3. The team channel.
    let mut channel = channel_write(tx, provisioning, &created_by).await?;
    if let Err(error) = channel.normalize() {
        return Ok(Err(refused(error)));
    }
    if let Some(refusal) = malformed("channel address", &channel.slug) {
        return Ok(Err(refusal));
    }
    let channel_id = key.id("channel");
    let company_slug: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id = $1")
        .bind(company.as_uuid())
        .fetch_one(&mut **tx)
        .await?;
    insert_channel_row_on(tx, company.as_uuid(), channel_id, &channel).await?;
    insert_desk_addresses(
        tx,
        company.as_uuid(),
        channel_id,
        &channel.slug,
        &channel.alias_slugs,
    )
    .await?;
    write_channel_email_binding(tx, company.as_uuid(), channel_id, &channel).await?;

    let native = match provision_workflow_on(
        tx,
        templates,
        provisioning.workflow(&agent_slug),
        WorkflowTarget::NewDesk(channel_id),
    )
    .await?
    {
        Ok(created) => created,
        Err(refusal) => return Ok(Err(refusal)),
    };

    Ok(Ok(ProvisionedAssistant {
        agent_id,
        address: crate::entities::channel::Channel::address_for(
            &crate::entities::value_objects::ChannelSlug::new(channel.slug.clone()),
            &crate::entities::value_objects::CompanySlug::from(company_slug),
            &templates.app_domain,
        )
        .to_string(),
        agent_slug,
        channel_id,
        channel_slug: channel.slug,
        workflow: native.workflow,
        version: native.version,
        binding: native.binding,
        reply_mode: provisioning.reply_mode,
    }))
}

/// Publish the reply workflow and its typed binding. Existing department desks select their
/// replacement separately; new desks and agent principals bootstrap their first responder here.
pub(super) async fn provision_workflow_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    provisioning: WorkflowProvisioning<'_>,
    target: WorkflowTarget,
) -> AppResult<Provisioned<NativeWorkflow>> {
    let company = provisioning.company;
    let requester = provisioning.requester;
    let key = &provisioning.key;
    // 4. The workflow: the reply mode's steps on the agent base, plus the spec's own, published
    // as a copy of the reply-mode template.
    let workflow = WorkflowId::new(key.id("workflow"));
    let version = VersionId::new(key.id("version"));
    let decoder = crate::adapters::workflow_source::WorkflowSourceDecoder;
    let template_id = match provisioning.reply_mode {
        ReplyMode::Autonomous => AUTONOMOUS_RESPONSE,
        ReplyMode::Reviewed => REVIEWED_SUPPORT,
    };
    let template = templates
        .catalogue
        .template(template_id)
        .ok_or_else(|| AppError::Internal(format!("Template {template_id} is not shipped")))?;
    let source = provisioning
        .reply_mode
        .agent_workflow()
        .with_steps(provisioning.steps.to_vec())
        .compose(&decoder, AGENT_BASE_SOURCE)
        .and_then(|source| decoder.encode(&source))
        .and_then(|source| {
            retarget_agent(&decoder, &source, REPLY_AGENT_STEP, provisioning.agent_slug)
        })
        .and_then(|source| rebase_workflow_id(&decoder, &source, workflow));
    let source = match source {
        Ok(source) => source,
        Err(diagnostic) => {
            return Ok(Err(ProvisioningRefusal::Invalid(
                diagnostic.message.to_string(),
            )));
        }
    };
    let draft = CompanyWorkflowDraft::from_template(company, workflow, template, source);
    super::definitions::insert_draft_on(tx, &draft, requester, "copied", target.owner()).await?;
    let bundle = match compiled(tx, templates, &decoder, &draft, version).await? {
        Ok(bundle) => Arc::new(bundle),
        Err(refusal) => return Ok(Err(refusal)),
    };
    let publication = provisioned_publication(
        PublishDraft {
            target: DefinitionTarget {
                company,
                actor: requester,
                workflow,
            },
            expected: draft.revision(),
            version,
            key: IdempotencyKey::parse(format!("provision:{}", version.as_uuid()))?,
        },
        draft,
        bundle,
    )
    .map_err(lifecycle_error)?;
    let bundle = super::definitions::publish_on(tx, &publication).await?;

    let binding = provision_binding_on(tx, &provisioning, target, bundle).await?;
    Ok(Ok(NativeWorkflow {
        workflow,
        version,
        binding,
    }))
}

async fn provision_binding_on(
    tx: &mut Transaction<'_, Postgres>,
    provisioning: &WorkflowProvisioning<'_>,
    target: WorkflowTarget,
    bundle: Arc<PublishedBundle>,
) -> AppResult<WorkflowBindingId> {
    let company = provisioning.company;
    let requester = provisioning.requester;
    let key = &provisioning.key;
    // 5. The binding: configured, then activated, as the requester.
    let binding = WorkflowBindingId::new(key.id("binding"));
    let params = match provisioning.reply_mode {
        ReplyMode::Autonomous => json!({}),
        ReplyMode::Reviewed => {
            let principal: Uuid = sqlx::query_scalar(
                "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
            )
            .bind(company.as_uuid())
            .bind(requester.user_id())
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(missing)?;
            json!({"reviewer": principal.to_string()})
        }
    };
    let changes = provisioned_binding(
        BindingTarget {
            company,
            actor: requester,
            binding,
        },
        target.association(),
        params,
        MessageTrigger::Direct,
        bundle,
    )
    .map_err(lifecycle_error)?;
    for change in &changes {
        super::bindings::save_binding_on(tx, change).await?;
    }

    if let Some(selected) = target.selection() {
        super::channel_reply::bootstrap_native_on(tx, company, selected, requester, binding)
            .await?;
    }
    Ok(binding)
}

pub(crate) struct AgentWorkflowOwner<'a> {
    pub agent: Uuid,
    pub principal: PrincipalId,
    pub slug: &'a str,
}

/// Provision the agent's owned autonomous workflow without creating a shared intake desk.
pub(crate) async fn provision_agent_workflow_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    company: CompanyId,
    actor: WorkflowActor,
    owner: AgentWorkflowOwner<'_>,
) -> AppResult<Provisioned<NativeWorkflow>> {
    let key = ProvisioningKey::agent(owner.agent);
    provision_workflow_on(
        tx,
        templates,
        WorkflowProvisioning {
            company,
            requester: actor,
            agent_slug: owner.slug,
            reply_mode: ReplyMode::Autonomous,
            steps: &[],
            key: &key,
        },
        WorkflowTarget::Agent {
            agent: owner.agent,
            principal: owner.principal,
        },
    )
    .await
}
