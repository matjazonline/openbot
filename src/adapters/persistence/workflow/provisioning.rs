//! Built-in provisioning (p08 D2): the action authority, the local effect, the requester rule
//! and `provision_on`, the one-transaction assistant setup the `create_agent_channel` tool and
//! the "Create assistant channel" command share.
//!
//! Lock order: the company row first (`prelock_on` on the tool path, `authorize_company` on the
//! command path), then shared locks on members and principals, then only rows this transaction
//! inserts. No existing channel, schedule or occurrence is touched.
use super::action_dispatch::{LockedActionAuthority, SqlActionAuthority, SqlLocalAction};
use super::http::begin_http;
use super::http::runtime::{context, facts};
use super::*;
use crate::adapters::persistence::agent::insert_agent_on;
use crate::adapters::persistence::channel::{
    insert_channel_row_on, insert_channel_slugs, write_channel_email_binding,
};
use crate::adapters::persistence::participant::create_agent_principal_on;
use crate::application::use_cases::agent::{AgentWrite, model_allowed};
use crate::application::use_cases::channel::ChannelWrite;
use crate::application::workflow::actions::*;
use crate::application::workflow::binding::{ResourceReadiness, ResourceStatus};
use crate::application::workflow::compiler::{SourceDecoder, rebase_workflow_id, retarget_agent};
use crate::application::workflow::provisioning::*;
use crate::entities::creation::CreationProvenance;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Current authority for the built-in provisioning target: the run actor must still be the
/// company owner or an admin, and the snapshot is exactly the trusted built-in. There is never
/// an approval (P08-Q5).
pub struct ProvisioningActionAuthority;
#[async_trait]
impl SqlActionAuthority for ProvisioningActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        let request = authority.action.request();
        if provisioning_target(&request.target)? != request.scope.company.as_uuid() {
            return Err(missing());
        }
        super::authority::authorize_company(tx, request.scope.company, authority.actor).await?;
        let kind =
            ProvisioningKind::parse(request.contract.contract.name.as_str()).ok_or_else(missing)?;
        Ok(LockedActionAuthority {
            resource: provisioning_status(request.scope.company)?,
            policy: CurrentActionPolicy {
                tool: builtin_provisioning_snapshot(request.scope.company, kind)?,
                approval_required: false,
            },
        })
    }
}

fn provisioning_status(company: CompanyId) -> AppResult<ResourceStatus> {
    Ok(ResourceStatus {
        id: RuntimeResourceId::new(company.as_uuid()),
        company_id: company,
        kind: TypeName::parse(PROVISIONING).map_err(|_| invalid())?,
        supported_contracts: [LIST_COMPANY_AGENTS, CREATE_AGENT_CHANNEL]
            .into_iter()
            .map(|name| TypeName::parse(name).map_err(|_| invalid()))
            .collect::<AppResult<BTreeSet<_>>>()?,
        authorized: true,
        readiness: ResourceReadiness::Ready,
    })
}

fn provisioning_target(target: &ActionTarget) -> AppResult<Uuid> {
    match target {
        ActionTarget::Local {
            resource,
            resource_kind,
        } if resource_kind.as_str() == PROVISIONING => Ok(*resource),
        _ => Err(missing()),
    }
}

#[async_trait]
impl ProvisioningPreparation for PostgresPersistence {
    /// The frozen request for one call; the trusted built-in must be in the run's bundle.
    async fn provisioning_tool_request(
        &self,
        scope: ActionScope,
        kind: ProvisioningKind,
        arguments: Value,
    ) -> AppResult<ActionRequest> {
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let tool = facts
            .bundle
            .snapshots()
            .tools
            .iter()
            .find(|tool| {
                tool.contract.name.as_str() == kind.name() && tool.contract.connection.is_none()
            })
            .cloned()
            .ok_or_else(missing)?;
        if !is_trusted_provisioning_snapshot(&tool)? {
            return Err(missing());
        }
        let request = ActionRequest {
            scope,
            operation_key: tool.contract.name.clone(),
            target: ActionTarget::Local {
                resource: scope.company.as_uuid(),
                resource_kind: TypeName::parse(PROVISIONING).map_err(|_| invalid())?,
            },
            contract: tool,
            arguments,
            context: context(&facts),
        };
        tx.commit().await?;
        Ok(request)
    }
}

/// What this deployment provisions with: the shipped templates and refused step kinds the
/// workflow is compiled against, and the domain channel addresses are on (`APP_DOMAIN_NAME`).
#[derive(Clone)]
pub struct ProvisioningDeployment {
    catalogue: Arc<TemplateCatalogue>,
    refused_kinds: &'static [&'static str],
    app_domain: String,
}
impl ProvisioningDeployment {
    pub fn new(
        catalogue: Arc<TemplateCatalogue>,
        refused_kinds: &'static [&'static str],
        app_domain: impl Into<String>,
    ) -> Self {
        Self {
            catalogue,
            refused_kinds,
            app_domain: app_domain.into(),
        }
    }
    pub fn catalogue(&self) -> &TemplateCatalogue {
        &self.catalogue
    }
}

/// The first production `SqlLocalAction` (p08 D2.2a). Every write uses the action transaction,
/// so the effect, its marker and its receipt commit together.
pub struct ProvisioningEffects {
    templates: ProvisioningDeployment,
}
impl ProvisioningEffects {
    pub fn new(templates: ProvisioningDeployment) -> Self {
        Self { templates }
    }
}

#[async_trait]
impl SqlLocalAction for ProvisioningEffects {
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
        subject: &ApprovalSubject,
    ) -> AppResult<Value> {
        let request = action.request();
        let company = request.scope.company;
        let kind =
            ProvisioningKind::parse(request.contract.contract.name.as_str()).ok_or_else(missing)?;
        let requester = match requester_on(tx, request.scope).await? {
            Some(requester) => requester,
            None => return Ok(ProvisioningRefusal::Forbidden.tool_error()),
        };
        if let Err(refusal) = authorize_requester(tx, company, requester).await? {
            return Ok(refusal.tool_error());
        }
        match kind {
            ProvisioningKind::ListAgents => list_on(tx, company).await,
            ProvisioningKind::CreateAgentChannel => {
                let spec = CreateAgentChannel::parse(&request.arguments)?;
                let provisioning = Provisioning {
                    company,
                    requester,
                    agent: AssistantAgent::New(Box::new(spec.clone())),
                    channel_slug: spec
                        .channel_slug
                        .clone()
                        .unwrap_or_else(|| spec.slug.clone()),
                    channel_name: spec
                        .channel_name
                        .clone()
                        .unwrap_or_else(|| spec.name.clone()),
                    reply_mode: spec.reply_mode,
                    key: ProvisioningKey::invocation(subject.invocation.as_uuid()),
                };
                Ok(
                    match provision_guarded(tx, &self.templates, &provisioning).await? {
                        Ok(provisioned) => provisioned.to_json(),
                        Err(refusal) => refusal.tool_error(),
                    },
                )
            }
        }
    }
}

/// The requester's own owner/admin check, on its member and principal rows (`FOR SHARE` through
/// commit). Its not-found answer is the refusal; any other error propagates (F046).
async fn authorize_requester(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    requester: WorkflowActor,
) -> AppResult<Provisioned<()>> {
    match super::authority::authorize_company(tx, company, requester).await {
        Ok(()) => Ok(Ok(())),
        Err(AppError::NotFound(_)) => Ok(Err(ProvisioningRefusal::Forbidden)),
        Err(error) => Err(error),
    }
}

/// Who asked for this run (p08 D2.2, F037, F048), from the root run's stored admission source:
///
/// - a message root: the triggering message's person author, only when its intent's origin is
///   `application` (signed in; email authenticates a domain, not a member);
/// - a manual or scheduled root: the root's actor (the authenticated caller, or the binding
///   run-as captured on the occurrence);
/// - a child: its root's requester.
///
/// Anything else (a root deeper than 64 levels, a source of another kind, a message root without
/// its intent) has no requester, and provisioning is forbidden.
async fn requester_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
) -> AppResult<Option<WorkflowActor>> {
    let root: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "WITH RECURSIVE chain(id, parent, actor, trigger, depth) AS ( \
           SELECT id, parent_run_id, actor_id, trigger_id, 0 FROM workflow_runs \
           WHERE company_id = $1 AND id = $2 \
           UNION ALL \
           SELECT run.id, run.parent_run_id, run.actor_id, run.trigger_id, chain.depth + 1 \
           FROM workflow_runs AS run JOIN chain ON run.company_id = $1 AND run.id = chain.parent \
           WHERE chain.depth < 64) \
         SELECT id, actor, trigger FROM chain WHERE parent IS NULL",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let Some((root, actor, trigger)) = root else {
        return Ok(None);
    };
    let sources: Vec<String> = sqlx::query_scalar(
        "SELECT source_key FROM workflow_admissions WHERE company_id = $1 AND run_id = $2",
    )
    .bind(scope.company.as_uuid())
    .bind(root)
    .fetch_all(&mut **tx)
    .await?;
    let [source] = sources.as_slice() else {
        return Ok(None);
    };
    match requester_source(source) {
        RequesterSource::RootActor => Ok(Some(WorkflowActor::authenticated(actor)?)),
        RequesterSource::None => Ok(None),
        RequesterSource::Message(message) => {
            let author: Option<Uuid> = sqlx::query_scalar(
                "SELECT principal.user_id FROM workflow_message_intents AS intent \
                 JOIN messages AS message ON message.company_id = intent.company_id \
                   AND message.id = intent.message_id \
                 JOIN principals AS principal ON principal.company_id = message.company_id \
                   AND principal.id = message.author_principal_id \
                 WHERE intent.company_id = $1 AND intent.id = $2 AND intent.message_id = $3 \
                   AND intent.origin = 'application' AND principal.kind = 'person'",
            )
            .bind(scope.company.as_uuid())
            .bind(trigger)
            .bind(message)
            .fetch_optional(&mut **tx)
            .await?
            .flatten();
            author.map(WorkflowActor::authenticated).transpose()
        }
    }
}

/// Whom a root's stored admission source (`SourceKey`, `v1:` + JSON) makes the requester.
#[derive(Debug, PartialEq, Eq)]
enum RequesterSource {
    /// Manual or scheduled: the root's actor.
    RootActor,
    /// The triggering message, whose author counts only from the app.
    Message(Uuid),
    /// Any other or unreadable source: nobody (fail closed).
    None,
}
fn requester_source(source_key: &str) -> RequesterSource {
    let Some(source) = source_key
        .strip_prefix("v1:")
        .and_then(|source| serde_json::from_str::<Value>(source).ok())
    else {
        return RequesterSource::None;
    };
    match source.as_array().map(Vec::as_slice) {
        Some([kind, _]) if kind == "manual" => RequesterSource::RootActor,
        Some([kind, _, _]) if kind == "schedule" => RequesterSource::RootActor,
        Some([kind, message]) if kind == "message" => message
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .map_or(RequesterSource::None, RequesterSource::Message),
        _ => RequesterSource::None,
    }
}

/// One listed agent: slug, name, description, provider and model.
type ListedAgent = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The company's agents by slug and the models it can run, for the Builder (D2.1). Bounded
/// (F050): names and descriptions are cut, and entries stop at `MAX_LISTING_BYTES`, models
/// first because creation needs one; `truncated` says whether anything was left out.
async fn list_on(tx: &mut Transaction<'_, Postgres>, company: CompanyId) -> AppResult<Value> {
    let mut budget = MAX_LISTING_BYTES;
    let mut truncated = false;
    let mut fits = |entry: &Value| {
        let size = entry.to_string().len() + 1;
        let fit = size <= budget;
        if fit {
            budget -= size;
        }
        fit
    };
    let mut models = Vec::new();
    for (provider, model) in ready_models_on(tx, company).await? {
        let entry = json!({"provider": provider, "model": model});
        if models.len() == MAX_LISTED_MODELS || !fits(&entry) {
            truncated = true;
            break;
        }
        models.push(entry);
    }
    let rows: Vec<ListedAgent> = sqlx::query_as(
        "SELECT slug::text, name, description, provider, model FROM agents \
         WHERE company_id = $1 ORDER BY slug LIMIT $2",
    )
    .bind(company.as_uuid())
    .bind(MAX_LISTED_AGENTS as i64 + 1)
    .fetch_all(&mut **tx)
    .await?;
    truncated |= rows.len() > MAX_LISTED_AGENTS;
    let mut agents = Vec::new();
    for (slug, name, description, provider, model) in rows.into_iter().take(MAX_LISTED_AGENTS) {
        let entry = json!({
            "slug": slug,
            "name": name.chars().take(MAX_LISTED_NAME_CHARS).collect::<String>(),
            "description": description
                .map(|text| text.chars().take(MAX_LISTED_DESCRIPTION_CHARS).collect::<String>()),
            "provider": provider, "model": model,
        });
        if !fits(&entry) {
            truncated = true;
            break;
        }
        agents.push(entry);
    }
    Ok(json!({"agents": agents, "models": models, "truncated": truncated}))
}

/// Every enabled model of a connection that has a key, default connection first, each in the
/// order the connection stores them.
async fn ready_models_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
) -> AppResult<Vec<(String, String)>> {
    let connections: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT provider, models FROM company_model_connections \
         WHERE company_id = $1 AND api_key IS NOT NULL ORDER BY is_default DESC, provider",
    )
    .bind(company.as_uuid())
    .fetch_all(&mut **tx)
    .await?;
    Ok(connections
        .into_iter()
        .flat_map(|(provider, models)| {
            models
                .into_iter()
                .map(move |model| (provider.clone(), model))
        })
        .collect())
}

/// Which agent the assistant channel answers with.
pub(crate) enum AssistantAgent {
    /// `create_agent_channel`: a new agent from the Builder's spec.
    New(Box<CreateAgentChannel>),
    /// "Create assistant channel": an existing company agent.
    Existing(String),
}

/// The seed every provisioned id derives from: the action invocation for the tool, or the
/// company and client command key for the UI command (D2.3 step 7).
pub(crate) struct ProvisioningKey([u8; 32]);
impl ProvisioningKey {
    fn invocation(invocation: Uuid) -> Self {
        Self(
            Sha256::digest([b"p08.provisioning.tool:".as_slice(), invocation.as_bytes()].concat())
                .into(),
        )
    }
    fn command(company: CompanyId, command_key: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"p08.provisioning.command:");
        hasher.update(company.as_uuid().as_bytes());
        hasher.update(command_key.as_bytes());
        Self(hasher.finalize().into())
    }
    /// `UUIDv8(SHA-256("p08.provisioning" ‖ seed ‖ role))`.
    fn id(&self, role: &str) -> Uuid {
        let digest =
            Sha256::digest([b"p08.provisioning".as_slice(), &self.0, role.as_bytes()].concat());
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        // RFC 9562 version 8 (custom), variant 10, as the manual trigger id.
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Uuid::from_bytes(bytes)
    }
}

pub(crate) struct Provisioning {
    pub company: CompanyId,
    pub requester: WorkflowActor,
    pub agent: AssistantAgent,
    pub channel_slug: String,
    pub channel_name: String,
    pub reply_mode: ReplyMode,
    pub key: ProvisioningKey,
}

pub(crate) struct ProvisionedAssistant {
    agent_id: Uuid,
    agent_slug: String,
    channel_id: Uuid,
    channel_slug: String,
    address: String,
    workflow: WorkflowId,
    version: VersionId,
    binding: WorkflowBindingId,
    reply_mode: ReplyMode,
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

/// `provision_on` in a savepoint (D2.2a). A refusal rolls the savepoint back and is returned. On
/// an error, the savepoint is rolled back and the slugs are checked again: a slug that another
/// creator (one not holding the company lock) took meanwhile is the conflict refusal; anything
/// else propagates, so the caller's whole transaction rolls back (F046).
pub(crate) async fn provision_guarded(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    provisioning: &Provisioning,
) -> AppResult<Provisioned<ProvisionedAssistant>> {
    let mut savepoint = sqlx::Acquire::begin(&mut **tx).await?;
    match provision_on(&mut savepoint, templates, provisioning).await {
        Ok(Ok(provisioned)) => {
            savepoint.commit().await?;
            Ok(Ok(provisioned))
        }
        Ok(Err(refusal)) => {
            savepoint.rollback().await?;
            Ok(Err(refusal))
        }
        Err(error) => {
            savepoint.rollback().await?;
            match taken_slug(tx, provisioning).await? {
                Some(refusal) => Ok(Err(refusal)),
                None => Err(error),
            }
        }
    }
}

/// The conflict refusal when the new agent's slug or the channel's address is already taken in
/// the company (channel aliases included).
async fn taken_slug(
    tx: &mut Transaction<'_, Postgres>,
    provisioning: &Provisioning,
) -> AppResult<Option<ProvisioningRefusal>> {
    let company = provisioning.company.as_uuid();
    if let AssistantAgent::New(spec) = &provisioning.agent {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM agents WHERE company_id = $1 AND slug = $2)",
        )
        .bind(company)
        .bind(spec.slug.trim().to_lowercase())
        .fetch_one(&mut **tx)
        .await?;
        if taken {
            return Ok(Some(ProvisioningRefusal::Conflict(format!(
                "An agent named '{}' already exists in this company",
                spec.slug.trim()
            ))));
        }
    }
    let taken: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM channel_slugs WHERE company_id = $1 AND slug = $2)",
    )
    .bind(company)
    .bind(provisioning.channel_slug.trim().to_lowercase())
    .fetch_one(&mut **tx)
    .await?;
    Ok(taken.then(|| {
        ProvisioningRefusal::Conflict(format!(
            "The address '{}' is already used by a channel in this company",
            provisioning.channel_slug.trim()
        ))
    }))
}

/// Provision an assistant: the agent (new), a team channel, the reply-mode workflow and its
/// active binding, all as the requester, on the caller's transaction (D2.3). The caller holds
/// the company lock and has authorized the requester.
async fn provision_on(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    provisioning: &Provisioning,
) -> AppResult<Provisioned<ProvisionedAssistant>> {
    let company = provisioning.company;
    let requester = provisioning.requester;
    let key = &provisioning.key;
    let created_by = CreationProvenance::user(requester.user_id());

    // 1. Pre-checks under the company lock.
    if let Some(refusal) = taken_slug(tx, provisioning).await? {
        return Ok(Err(refusal));
    }

    // 2. The agent.
    let (agent_id, agent_slug) = match &provisioning.agent {
        AssistantAgent::New(spec) => {
            let mut write = match agent_write(tx, company, spec, &created_by).await? {
                Ok(write) => write,
                Err(refusal) => return Ok(Err(refusal)),
            };
            if let Err(error) = write.normalize() {
                return Ok(Err(refused(error)));
            }
            if let Some(refusal) = malformed("agent slug", &write.slug) {
                return Ok(Err(refusal));
            }
            let id = key.id("agent");
            insert_agent_on(tx, id, Some(company.as_uuid()), &write).await?;
            create_agent_principal_on(tx, company.as_uuid(), id, &write.name).await?;
            (id, write.slug)
        }
        AssistantAgent::Existing(slug) => {
            let agent: Option<(Uuid, String, Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT id, slug::text, provider, model FROM agents \
                 WHERE company_id = $1 AND slug = $2 FOR KEY SHARE",
            )
            .bind(company.as_uuid())
            .bind(slug.trim().to_lowercase())
            .fetch_optional(&mut **tx)
            .await?;
            match agent {
                None => {
                    return Ok(Err(ProvisioningRefusal::Invalid(format!(
                        "No agent named '{}' exists in this company",
                        slug.trim()
                    ))));
                }
                Some((_, _, None, _) | (_, _, _, None)) => {
                    return Ok(Err(ProvisioningRefusal::Invalid(
                        "Choose the agent's provider and model first: a workflow runs an agent only on an explicit model".into(),
                    )));
                }
                Some((id, slug, _, _)) => (id, slug),
            }
        }
    };

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
    insert_channel_slugs(
        tx,
        company.as_uuid(),
        channel_id,
        &channel.slug,
        &channel.alias_slugs,
    )
    .await?;
    write_channel_email_binding(tx, company.as_uuid(), channel_id, &channel).await?;

    // 4. The workflow, published from the reply-mode template.
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
    let source = retarget_agent(&decoder, template.source(), REPLY_AGENT_STEP, &agent_slug)
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
    super::definitions::insert_draft_on(tx, &draft, requester, "copied").await?;
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

    // 5. The binding: configured, then activated, as the requester.
    let binding = WorkflowBindingId::new(key.id("binding"));
    let params = match provisioning.reply_mode {
        ReplyMode::Autonomous => json!({}),
        ReplyMode::Reviewed => json!({"reviewer": requester.user_id().to_string()}),
    };
    let changes = provisioned_binding(
        BindingTarget {
            company,
            actor: requester,
            binding,
        },
        RelatedAssociation::Channel(RelatedChannelId::new(channel_id)),
        params,
        MessageTrigger::Direct,
        bundle,
    )
    .map_err(lifecycle_error)?;
    for change in &changes {
        super::bindings::save_binding_on(tx, change).await?;
    }

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
        workflow,
        version,
        binding,
        reply_mode: provisioning.reply_mode,
    }))
}

/// The Builder's spec as an agent write: an explicit model the company can run, grantable tools
/// and the company's skills by exact slug. Its refusals are the deterministic validations.
async fn agent_write(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    spec: &CreateAgentChannel,
    created_by: &CreationProvenance,
) -> AppResult<Provisioned<AgentWrite>> {
    let ready: Vec<_> = ready_models_on(tx, company)
        .await?
        .into_iter()
        .map(
            |(provider, model)| crate::entities::company::CompanyModelConnection {
                provider: provider.into(),
                models: vec![model.into()],
                is_default: false,
                has_api_key: true,
            },
        )
        .collect();
    if !model_allowed(&ready, &spec.provider, &spec.model) {
        return Ok(Err(ProvisioningRefusal::Invalid(format!(
            "Model '{}' of provider '{}' is not enabled with a key in this company",
            spec.model, spec.provider
        ))));
    }
    let mut tools = Vec::with_capacity(spec.tools.len());
    for name in &spec.tools {
        let Ok(name) = TypeName::parse(name) else {
            return Ok(Err(ProvisioningRefusal::Invalid(format!(
                "'{name}' is not a tool name"
            ))));
        };
        if is_builtin_provisioning_tool(name.as_str()) {
            return Ok(Err(ProvisioningRefusal::Invalid(format!(
                "{name} cannot be granted to a created agent"
            ))));
        }
        if !tools.contains(&name) {
            tools.push(name);
        }
    }
    let mut skill_ids = Vec::with_capacity(spec.skills.len());
    for slug in &spec.skills {
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM skills WHERE company_id = $1 AND slug = $2 FOR KEY SHARE",
        )
        .bind(company.as_uuid())
        .bind(slug)
        .fetch_optional(&mut **tx)
        .await?;
        match id {
            Some(id) if !skill_ids.contains(&id) => skill_ids.push(id),
            Some(_) => {}
            None => {
                return Ok(Err(ProvisioningRefusal::Invalid(format!(
                    "No skill named '{slug}' exists in this company"
                ))));
            }
        }
    }
    Ok(Ok(AgentWrite {
        name: spec.name.clone(),
        slug: spec.slug.clone(),
        provider: Some(spec.provider.clone()),
        model: Some(spec.model.clone()),
        system_prompt: Some(spec.system_prompt.clone()),
        description: spec.description.clone(),
        workflow_tools: Some(tools),
        skill_ids,
        created_by: Some(created_by.clone()),
        ..AgentWrite::default()
    }))
}

/// A team channel (never public, P08-Q4) on the company's channel defaults. The assistant's
/// agent answers it through the workflow binding published next, not through the channel.
async fn channel_write(
    tx: &mut Transaction<'_, Postgres>,
    provisioning: &Provisioning,
    created_by: &CreationProvenance,
) -> AppResult<ChannelWrite> {
    let add_3rd_party: bool =
        sqlx::query_scalar("SELECT default_add_3rd_party FROM companies WHERE id = $1")
            .bind(provisioning.company.as_uuid())
            .fetch_one(&mut **tx)
            .await?;
    Ok(ChannelWrite {
        name: provisioning.channel_name.clone(),
        slug: provisioning.channel_slug.clone(),
        participant_emails: None,
        enabled: true,
        add_3rd_party,
        created_by: Some(created_by.clone()),
        ..ChannelWrite::default()
    })
}

/// Compile the draft against the message built-ins and the agents captured on this transaction.
/// The capture's and the compiler's refusals are deterministic validations (`BadRequest` and
/// diagnostics), so they are the refusal; every other error propagates.
async fn compiled(
    tx: &mut Transaction<'_, Postgres>,
    templates: &ProvisioningDeployment,
    decoder: &impl SourceDecoder,
    draft: &CompanyWorkflowDraft,
    version: VersionId,
) -> AppResult<Provisioned<PublishedBundle>> {
    let decoded = match decoder.decode(draft.source()) {
        Ok(decoded) => decoded,
        Err(diagnostic) => {
            return Ok(Err(ProvisioningRefusal::Invalid(
                diagnostic.message.to_string(),
            )));
        }
    };
    // As the production directory nests them: the message built-ins (the caller holds the
    // company lock and authorized the requester), then the agents. Provisioned templates select
    // no HTTP or MCP operation and call no child.
    let mut captured = super::message::message_builtins(draft.company())?;
    let selection = match super::agent_publication::agent_selection(draft) {
        Ok(selection) => selection,
        Err(AppError::BadRequest(message)) => {
            return Ok(Err(ProvisioningRefusal::Invalid(message)));
        }
        Err(error) => return Err(error),
    };
    if let Some(selection) = selection {
        match super::agent_publication::capture_agents_on(
            tx,
            draft.company(),
            &selection,
            &mut captured,
        )
        .await
        {
            Ok(()) => {}
            Err(AppError::BadRequest(message)) => {
                return Ok(Err(ProvisioningRefusal::Invalid(message)));
            }
            Err(error) => return Err(error),
        }
    }
    match compile_bundle(decoded, draft, version, captured, templates.refused_kinds) {
        Ok(bundle) => Ok(Ok(bundle)),
        Err(LifecycleError::Validation(diagnostic)) => Ok(Err(ProvisioningRefusal::Invalid(
            diagnostic.message.to_string(),
        ))),
        Err(LifecycleError::Application(error)) => Err(error),
    }
}

/// The address format the `agents_slug_format` and `channel_slugs_format` constraints hold
/// (`^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$`), checked before the insert so a model-supplied slug is
/// refused rather than failing the action.
fn malformed(noun: &str, slug: &str) -> Option<ProvisioningRefusal> {
    let bytes = slug.as_bytes();
    let edge = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let valid = bytes.first().is_some_and(edge)
        && bytes.last().is_some_and(edge)
        && bytes.iter().all(|byte| edge(byte) || *byte == b'-');
    (!valid).then(|| {
        ProvisioningRefusal::Invalid(format!(
            "The {noun} '{slug}' must use lowercase letters, digits and inner hyphens"
        ))
    })
}

/// A pure validator's refusal (`normalize`): its message is the reason.
fn refused(error: AppError) -> ProvisioningRefusal {
    ProvisioningRefusal::Invalid(match error {
        AppError::BadRequest(message) => message,
        other => other.to_string(),
    })
}

fn lifecycle_error(error: LifecycleError) -> AppError {
    match error {
        LifecycleError::Application(error) => error,
        LifecycleError::Validation(diagnostic) => AppError::Internal(format!(
            "Provisioned workflow failed a lifecycle check: {}",
            diagnostic.message
        )),
    }
}

/// "Create assistant channel" over the shared provisioning (D2.4): one transaction under the
/// company lock that reads the command record first, provisions with the existing agent, and
/// records the result with the request digest.
pub struct AssistantChannelService {
    persistence: PostgresPersistence,
    templates: ProvisioningDeployment,
}
impl AssistantChannelService {
    pub fn new(persistence: PostgresPersistence, templates: ProvisioningDeployment) -> Self {
        Self {
            persistence,
            templates,
        }
    }
}

#[async_trait]
impl AssistantChannels for AssistantChannelService {
    async fn create_assistant_channel(
        &self,
        company: CompanyId,
        caller: WorkflowActor,
        request: AssistantChannelRequest,
    ) -> AppResult<Provisioned<Value>> {
        if request.command_key.is_empty() || request.command_key.len() > MAX_COMMAND_KEY_BYTES {
            return Err(AppError::BadRequest(
                "A command key of 1 to 128 bytes is required".into(),
            ));
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(
                json!({
                    "caller": caller.user_id(),
                    "agent_slug": request.agent_slug,
                    "channel_slug": request.channel_slug,
                    "channel_name": request.channel_name,
                    "reply_mode": request.reply_mode.as_str(),
                })
                .to_string()
            )
        );
        let mut tx = self.persistence.pool().begin().await?;
        super::authority::authorize_company(&mut tx, company, caller).await?;
        let stored: Option<(String, Value)> = sqlx::query_as(
            "SELECT request_digest, result FROM workflow_provisioning_commands \
             WHERE company_id = $1 AND command_key = $2",
        )
        .bind(company.as_uuid())
        .bind(&request.command_key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((stored, result)) = stored {
            if stored != digest {
                return Err(AppError::Conflict(
                    "This command key was already used for a different request".into(),
                ));
            }
            tx.commit().await?;
            return Ok(Ok(result));
        }
        let provisioning = Provisioning {
            company,
            requester: caller,
            agent: AssistantAgent::Existing(request.agent_slug),
            channel_slug: request.channel_slug,
            channel_name: request.channel_name,
            reply_mode: request.reply_mode,
            key: ProvisioningKey::command(company, &request.command_key),
        };
        let provisioned = match provision_guarded(&mut tx, &self.templates, &provisioning).await? {
            Ok(provisioned) => provisioned.to_json(),
            Err(refusal) => {
                tx.rollback().await?;
                return Ok(Err(refusal));
            }
        };
        sqlx::query(
            "INSERT INTO workflow_provisioning_commands \
             (company_id, command_key, request_digest, actor_id, result) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(company.as_uuid())
        .bind(&request.command_key)
        .bind(&digest)
        .bind(caller.user_id())
        .bind(&provisioned)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Ok(provisioned))
    }

    async fn assistant_channel_result(
        &self,
        company: CompanyId,
        caller: WorkflowActor,
        command_key: &str,
    ) -> AppResult<Option<Value>> {
        Ok(sqlx::query_scalar(
            "SELECT command.result FROM workflow_provisioning_commands AS command \
             JOIN companies AS company ON company.id = command.company_id \
             WHERE command.company_id = $1 AND command.command_key = $2 \
               AND (company.user_id = $3 OR EXISTS (SELECT 1 FROM company_members AS member \
                    WHERE member.company_id = $1 AND member.user_id = $3 \
                      AND member.role = 'admin'))",
        )
        .bind(company.as_uuid())
        .bind(command_key)
        .bind(caller.user_id())
        .fetch_optional(self.persistence.pool())
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::super::admission_binding::SourceKey;
    use super::*;
    use crate::domain::workflow::{ScheduleId, ScheduleOccurrenceId, TriggerSource};

    fn key(source: TriggerSource) -> String {
        let company = CompanyId::new(Uuid::from_u128(1));
        let trigger = TriggerRef::new(company, TriggerId::new(Uuid::from_u128(2)), source).unwrap();
        SourceKey::from_trigger(&trigger).as_str().to_owned()
    }

    /// F048: every source admission writes maps to its requester rule, and anything else is
    /// nobody.
    #[test]
    fn requester_source_matches_each_stored_admission_source() {
        let message = Uuid::from_u128(3);
        assert_eq!(
            requester_source(&key(TriggerSource::Manual)),
            RequesterSource::RootActor
        );
        assert_eq!(
            requester_source(&key(TriggerSource::Schedule {
                schedule_id: ScheduleId::new(Uuid::from_u128(4)),
                occurrence_id: ScheduleOccurrenceId::new(Uuid::from_u128(5)),
            })),
            RequesterSource::RootActor
        );
        assert_eq!(
            requester_source(&key(TriggerSource::Message {
                message_id: crate::entities::message::CanonicalMessageId::new(message),
            })),
            RequesterSource::Message(message)
        );
        for unknown in [
            // A child is never a root, so its own source names nobody.
            "v1:[\"child\",\"r\",\"e\",\"s\",null,1]",
            "v1:[\"webhook\",\"x\"]",
            "v1:[\"manual\"]",
            "v1:[\"message\",\"not-a-uuid\"]",
            "v2:[\"manual\",\"x\"]",
            "v1:{\"manual\":1}",
            "",
        ] {
            assert_eq!(
                requester_source(unknown),
                RequesterSource::None,
                "{unknown}"
            );
        }
    }
}
