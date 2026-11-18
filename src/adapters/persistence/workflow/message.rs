//! 04.11 messaging persistence: trusted built-in authority, frozen request and route
//! resolution, and the message publication directory. The publication transaction lives in
//! `message_publish.rs`; receipts, effect waits and conflict parks in `message_receipts.rs`.
use super::http::begin_http;
use super::http::runtime::{RunFacts, context, facts, frozen_inputs};
use super::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::binding::{ResourceReadiness, ResourceStatus};
use crate::application::workflow::lease::ClaimedWorkflow;
use crate::application::workflow::message::*;
use crate::application::workflow::polling::WorkflowStepKind;
use crate::entities::correlation::CorrelationId;
use crate::entities::message::CanonicalMessageId;
use crate::entities::value_objects::{ChannelSlug, CompanySlug, EmailAddress, MessageId};
use serde_json::Value;
use std::collections::BTreeSet;

#[path = "message_publish.rs"]
mod publish;
#[path = "channel_questions.rs"]
pub(super) mod questions;
#[path = "message_receipts.rs"]
pub(crate) mod receipts;
#[path = "message_writer.rs"]
pub(super) mod writer;

pub(super) const MESSAGING: &str = "messaging";

/// Current authority for the built-in messaging target: the run actor must still be the
/// company owner or an admin, and the snapshot must be exactly the trusted built-in.
pub struct MessageActionAuthority;
#[async_trait]
impl action_dispatch::SqlActionAuthority for MessageActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<action_dispatch::LockedActionAuthority> {
        let request = authority.action.request();
        let company = message_target(&request.target)?;
        if company != request.scope.company.as_uuid() {
            return Err(missing());
        }
        let kind =
            MessageKind::parse(request.contract.contract.name.as_str()).ok_or_else(missing)?;
        let current = current_message_on(tx, request.scope, authority.actor, kind).await?;
        let run = run_route_on(tx, request.scope).await?;
        association::authorize_association(
            tx,
            request.scope.company,
            authority.actor,
            run.association,
        )
        .await?;
        if let MessageArguments::Reply { source, .. } =
            MessageArguments::parse(kind, &request.arguments)?
        {
            let source = source_association_on(tx, request.scope.company, source, run.association)
                .await?
                .ok_or_else(missing)?;
            association::authorize_association(tx, request.scope.company, authority.actor, source)
                .await?;
        }
        Ok(current)
    }
}

/// The current company messaging rule is shared by explicit actions and designated finals.
/// Each caller must establish its own invocation grant before using this resource authority.
pub(super) async fn current_message_on(
    db: &mut PgConnection,
    scope: ActionScope,
    actor: WorkflowActor,
    kind: MessageKind,
) -> AppResult<action_dispatch::LockedActionAuthority> {
    let company = scope.company;
    management::authorize_run_with_locks(db, company, scope.run, actor, row_locks::RowLocks::Wait)
        .await?;
    Ok(action_dispatch::LockedActionAuthority {
        resource: messaging_status(company)?,
        policy: CurrentActionPolicy {
            tool: builtin_message_snapshot(company, kind)?,
            approval_required: false,
        },
    })
}

/// Reconciliation of a message invocation records past truth under the company's current
/// authority, which is what owns the messaging resource (04.11-D3 §H.1).
#[async_trait]
impl super::action_reconciliation::SqlReconciliationResources for MessageActionAuthority {
    async fn lock_resource(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        actor: WorkflowActor,
        frozen: &ActionRunAuthority,
    ) -> AppResult<ResourceStatus> {
        let company = frozen.action.scope().company;
        if message_target(&frozen.action.request().target)? != company.as_uuid() {
            return Err(missing());
        }
        super::authority::authorize_company(tx, company, actor).await?;
        messaging_status(company)
    }
}

pub(super) fn messaging_status(company: CompanyId) -> AppResult<ResourceStatus> {
    Ok(ResourceStatus {
        id: RuntimeResourceId::new(company.as_uuid()),
        company_id: company,
        kind: TypeName::parse(MESSAGING).map_err(|_| invalid())?,
        supported_contracts: [MessageKind::Send, MessageKind::Reply, MessageKind::Ask]
            .into_iter()
            .map(|kind| TypeName::parse(kind.name()).map_err(|_| invalid()))
            .collect::<AppResult<BTreeSet<_>>>()?,
        authorized: true,
        readiness: ResourceReadiness::Ready,
    })
}

pub(super) fn message_target(target: &ActionTarget) -> AppResult<Uuid> {
    match target {
        ActionTarget::Local {
            resource,
            resource_kind,
        } if resource_kind.as_str() == MESSAGING => Ok(*resource),
        _ => Err(missing()),
    }
}

/// The frozen request for one message invocation; the built-in snapshot must be in the bundle.
fn message_request(
    scope: ActionScope,
    facts: &RunFacts,
    kind: MessageKind,
    arguments: Value,
) -> AppResult<ActionRequest> {
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
    if !is_trusted_message_snapshot(&tool)? {
        return Err(missing());
    }
    Ok(ActionRequest {
        scope,
        operation_key: tool.contract.name.clone(),
        target: ActionTarget::Local {
            resource: scope.company.as_uuid(),
            resource_kind: TypeName::parse(MESSAGING).map_err(|_| invalid())?,
        },
        contract: tool,
        arguments,
        context: context(facts),
    })
}

#[async_trait]
impl MessageActionPreparation for PostgresPersistence {
    async fn message_step_request(
        &self,
        claim: &ClaimedWorkflow,
        kind: &WorkflowStepKind,
    ) -> AppResult<ActionRequest> {
        let scope = ActionScope {
            company: claim.fence.scope.company,
            run: claim.fence.scope.run,
            execution: claim.fence.scope.execution,
        };
        let message_kind = MessageKind::parse(&kind.0).ok_or_else(missing)?;
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        let mut inputs = frozen_inputs(&mut tx, &facts, claim, kind).await?;
        let question_approval =
            message_kind == MessageKind::Ask && inputs.get("approval").is_some();
        inputs
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("approval");
        let mut request = message_request(scope, &facts, message_kind, inputs)?;
        request.context.approval_required |= question_approval;
        tx.commit().await?;
        Ok(request)
    }

    async fn message_tool_request(
        &self,
        scope: ActionScope,
        call: &MessageToolCall,
    ) -> AppResult<ActionRequest> {
        if call.kind == MessageKind::Ask {
            return Err(missing());
        }
        if matches!(
            call.origin,
            crate::application::agent::ToolCallOrigin::ChannelFinal(_)
        ) {
            return Err(missing());
        }
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
        if let crate::application::agent::ToolCallOrigin::Agent(origin) = call.origin {
            let request = super::agent_origin::request_on(
                &mut tx,
                scope,
                &facts,
                origin,
                &call.call,
                call.arguments.clone(),
            )
            .await?;
            if request.operation_key.as_str() != call.kind.name() {
                return Err(missing());
            }
            tx.commit().await?;
            return Ok(request);
        }
        super::agent_origin::require_workflow_origin(&mut tx, scope, &facts.bundle).await?;
        let kind = call.kind;
        let arguments = call.arguments.clone();
        let request = message_request(scope, &facts, kind, arguments)?;
        tx.commit().await?;
        Ok(request)
    }

    async fn message_routes(
        &self,
        request: &ActionRequest,
        arguments: &MessageArguments,
    ) -> AppResult<MessageRoutes> {
        let mut tx = begin_http(self).await?;
        let run = run_route_on(&mut tx, request.scope).await?;
        let author = message_author_on(
            &mut tx,
            request.scope,
            request.context.origin.into(),
            run.association,
        )
        .await?;
        let targets = match arguments {
            MessageArguments::Send { destinations, .. } => {
                let mut targets = Vec::with_capacity(destinations.len());
                for destination in destinations {
                    targets.push(
                        send_target_on(&mut tx, request.scope.company, &run, destination).await?,
                    );
                }
                targets
            }
            MessageArguments::Reply { source, .. } => {
                vec![reply_target_on(&mut tx, request.scope.company, &run, *source).await?]
            }
        };
        tx.commit().await?;
        Ok(MessageRoutes {
            company_slug: run.company_slug,
            correlation_id: run.correlation_id,
            targets,
            author,
            delivery_policy: run.delivery_policy,
        })
    }
}

enum MessageAuthorSource {
    Workflow,
    Agent(Uuid),
    Final,
}

impl From<crate::application::agent::ToolCallOrigin> for MessageAuthorSource {
    fn from(origin: crate::application::agent::ToolCallOrigin) -> Self {
        match origin {
            crate::application::agent::ToolCallOrigin::Agent(origin) => {
                Self::Agent(origin.agent.as_uuid())
            }
            crate::application::agent::ToolCallOrigin::ChannelFinal(_) => Self::Final,
            crate::application::agent::ToolCallOrigin::Workflow => Self::Workflow,
        }
    }
}

async fn message_author_on(
    db: &mut PgConnection,
    scope: ActionScope,
    origin: MessageAuthorSource,
    association: RelatedAssociation,
) -> AppResult<crate::application::workflow::message::WorkflowMessageAuthor> {
    use crate::application::workflow::message::WorkflowMessageAuthor;
    let actual_agent = match origin {
        MessageAuthorSource::Agent(agent) => Some(agent),
        MessageAuthorSource::Final => sqlx::query_scalar(
            "SELECT agent_id FROM agent_history_exchanges WHERE company_id=$1 AND run_id=$2 AND execution_id=$3",
        ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
            .fetch_optional(&mut *db).await?,
        MessageAuthorSource::Workflow => None,
    };
    let principal = if let Some(agent) = actual_agent {
        Some(
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent'",
            )
            .bind(scope.company.as_uuid())
            .bind(agent)
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(missing)?,
        )
    } else if let Some(AddressTarget::Principal(principal)) = association.target() {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id=$1 AND id=$2 AND kind='agent'",
        )
        .bind(scope.company.as_uuid())
        .bind(principal.as_uuid())
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(missing)
        .map(Some)?
    } else {
        None
    };
    Ok(principal.map_or(WorkflowMessageAuthor::System, |id| {
        WorkflowMessageAuthor::Agent(crate::entities::transport::PrincipalId::new(id))
    }))
}

/// The run's explicit channel/thread scope and the company's slug.
pub(super) struct RunRoute {
    company_slug: CompanySlug,
    correlation_id: CorrelationId,
    association: RelatedAssociation,
    delivery_policy: DeliveryPolicy,
}

async fn run_route_on(tx: &mut PgConnection, scope: ActionScope) -> AppResult<RunRoute> {
    #[derive(sqlx::FromRow)]
    struct RunRow {
        slug: String,
        correlation_id: Uuid,
        channel_id: Option<Uuid>,
        principal_id: Option<Uuid>,
        thread_id: Option<Uuid>,
        delivery_policy: String,
    }
    let row = sqlx::query_as::<_, RunRow>(
        "SELECT company.slug::text,run.correlation_id,run.channel_id,run.principal_id,run.thread_id,run.delivery_policy \
         FROM workflow_runs AS run JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_optional(tx).await?.ok_or_else(missing)?;
    Ok(RunRoute {
        company_slug: CompanySlug::from(row.slug),
        correlation_id: CorrelationId::from(row.correlation_id),
        association: association::association_from_columns(association::AssociationColumns {
            channel_id: row.channel_id,
            principal_id: row.principal_id,
            thread_id: row.thread_id,
        })?,
        delivery_policy: DeliveryPolicy::parse(&row.delivery_policy)?,
    })
}

pub(super) async fn address_route_on(
    tx: &mut PgConnection,
    company: CompanyId,
    target: AddressTarget,
) -> AppResult<Option<MessageSender>> {
    #[derive(sqlx::FromRow)]
    struct SenderRow {
        slug: String,
        name: String,
    }
    let row = sqlx::query_as::<_, SenderRow>(
        "SELECT address.slug::text AS slug, COALESCE(channel.name,principal.display_label) AS name \
         FROM addresses AS address \
         LEFT JOIN channels AS channel ON channel.company_id=address.company_id AND channel.id=address.channel_id \
         LEFT JOIN principals AS principal ON principal.company_id=address.company_id AND principal.id=address.principal_id \
         WHERE address.company_id=$1 AND address.channel_id IS NOT DISTINCT FROM $2::uuid \
           AND address.principal_id IS NOT DISTINCT FROM $3::uuid AND address.is_primary",
    ).bind(company.as_uuid()).bind(target.desk_id()).bind(target.principal_id().map(|id| id.as_uuid()))
        .fetch_optional(tx).await?;
    Ok(row.map(|row| MessageSender {
        target,
        slug: ChannelSlug::from(row.slug),
        name: row.name,
    }))
}

fn refused(reason: &str) -> AppError {
    AppError::BadRequest(format!("Workflow message refused: {reason}"))
}

/// A current member, with a principal, and the address the frozen delivery will use.
pub(super) struct MemberRoute {
    pub principal: Uuid,
    pub email: EmailAddress,
}
pub(super) async fn member_on(
    db: &mut PgConnection,
    company: CompanyId,
    user: Uuid,
    lock: bool,
) -> AppResult<Option<MemberRoute>> {
    let lock = if lock {
        " FOR SHARE OF member, principal"
    } else {
        ""
    };
    let row: Option<(Uuid, String)> = sqlx::query_as(&format!(
        "SELECT principal.id, account.email FROM department_members AS member \
         JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.root_user_id \
         JOIN users AS account ON account.id=member.root_user_id \
         WHERE member.company_id=$1 AND member.root_user_id=$2{lock}"
    ))
    .bind(company.as_uuid())
    .bind(user)
    .fetch_optional(&mut *db)
    .await?;
    Ok(row.map(|(principal, email)| MemberRoute {
        principal,
        email: EmailAddress::from(email),
    }))
}

async fn send_target_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    run: &RunRoute,
    destination: &MessageDestinationInput,
) -> AppResult<RouteTarget> {
    match destination.kind {
        DestinationKind::Channel => {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM channels WHERE company_id=$1 AND id=$2)",
            )
            .bind(company.as_uuid())
            .bind(destination.id)
            .fetch_one(&mut **tx)
            .await?;
            if !exists {
                return Err(refused(
                    "the destination channel does not exist in this company",
                ));
            }
            Ok(RouteTarget::Channel {
                channel: destination.id,
            })
        }
        DestinationKind::User => {
            // Never pick a first or default channel: email needs the run's explicit sender.
            let target = run
                .association
                .target()
                .ok_or_else(|| refused("emailing a user requires the run's inbox"))?;
            let sender = address_route_on(tx, company, target)
                .await?
                .ok_or_else(|| refused("the run channel cannot send email"))?;
            let member = member_on(tx, company, destination.id, false)
                .await?
                .ok_or_else(|| refused("the recipient is not a current member"))?;
            Ok(RouteTarget::User {
                user: destination.id,
                principal: member.principal,
                email: member.email,
                sender,
            })
        }
        DestinationKind::Reply => Err(invalid_message()),
    }
}

/// The exact source's one association within the run's scope; never "the latest".
pub(super) async fn source_association_on(
    tx: &mut PgConnection,
    company: CompanyId,
    source: CanonicalMessageId,
    run: RelatedAssociation,
) -> AppResult<Option<RelatedAssociation>> {
    let target = run.target();
    let rows = sqlx::query_as::<_, association::AssociationColumns>(
        "SELECT association.thread_id,association.channel_id,association.principal_id FROM thread_messages AS association \
         WHERE association.company_id=$1 AND association.message_id=$2 \
           AND ($3::uuid IS NULL OR association.thread_id=$3) \
           AND ($4 OR (association.channel_id IS NOT DISTINCT FROM $5::uuid \
             AND association.principal_id IS NOT DISTINCT FROM $6::uuid)) \
         ORDER BY association.id LIMIT 2",
    ).bind(company.as_uuid()).bind(source.as_uuid()).bind(run.thread_id().map(|id| id.as_uuid()))
        .bind(target.is_none()).bind(target.and_then(AddressTarget::desk_id))
        .bind(target.and_then(AddressTarget::principal_id).map(|id| id.as_uuid())).fetch_all(tx).await?;
    match rows.as_slice() {
        [only] => association::association_from_columns(*only).map(Some),
        _ => Ok(None),
    }
}

#[derive(sqlx::FromRow)]
struct SourceFacts {
    direction: String,
    audience: String,
    rfc_message_id: Option<String>,
    references_list: Option<Vec<String>>,
    sender: Option<String>,
}

async fn reply_target_on(
    tx: &mut PgConnection,
    company: CompanyId,
    run: &RunRoute,
    source: CanonicalMessageId,
) -> AppResult<RouteTarget> {
    let RelatedAssociation::Thread { target, thread_id } =
        source_association_on(tx, company, source, run.association)
            .await?
            .ok_or_else(|| {
                refused("the source message has no unambiguous association in this run's scope")
            })?
    else {
        return Err(invalid_message());
    };
    let thread = thread_id.as_uuid();
    let facts: SourceFacts = sqlx::query_as(
        "SELECT message.direction, message.audience, email.rfc_message_id, email.references_list, \
           (SELECT identity.subject FROM message_participants AS participant \
              JOIN participant_identities AS identity ON identity.company_id=participant.company_id \
                AND identity.id=participant.participant_identity_id \
             WHERE participant.company_id=message.company_id AND participant.message_id=message.id \
               AND participant.kind='sender' AND identity.transport='email' \
             ORDER BY participant.position LIMIT 1) AS sender \
         FROM messages AS message \
         LEFT JOIN email_message_metadata AS email ON email.company_id=message.company_id AND email.message_id=message.id \
         WHERE message.company_id=$1 AND message.id=$2",
    )
    .bind(company.as_uuid())
    .bind(source.as_uuid())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| refused("the source message does not exist"))?;
    if facts.audience == "internal_only" {
        return Ok(RouteTarget::ReplyInternal {
            source,
            thread,
            target,
        });
    }
    let (Some(rfc), Some(sender)) = (facts.rfc_message_id, facts.sender) else {
        return Err(refused(
            "only received email or internal messages can be answered",
        ));
    };
    if facts.direction != "inbound" || facts.audience != "external_conversation" {
        return Err(refused(
            "only received email or internal messages can be answered",
        ));
    }
    let sender_inbox = address_route_on(tx, company, target)
        .await?
        .ok_or_else(|| refused("the source channel cannot send email"))?;
    let relay = source_relay_on(tx, company, source, target).await?;
    Ok(RouteTarget::ReplyEmail {
        source,
        thread,
        sender: sender_inbox,
        recipient: EmailAddress::from(sender),
        in_reply_to: MessageId::from(rfc),
        references: facts
            .references_list
            .unwrap_or_default()
            .into_iter()
            .map(MessageId::from)
            .collect(),
        relay,
    })
}

/// The relay trace the source arrived with on this channel, as its admission intent recorded
/// it: the only place it is kept (p08 D1.6). A source with no intent was never accepted inbound
/// on this channel and starts a chain.
async fn source_relay_on(
    tx: &mut PgConnection,
    company: CompanyId,
    source: CanonicalMessageId,
    target: AddressTarget,
) -> AppResult<Option<SourceRelay>> {
    let recorded: Option<(i32, serde_json::Value)> = sqlx::query_as(
        "SELECT hop_count, trace_targets FROM workflow_message_intents \
         WHERE company_id=$1 AND message_id=$2 AND channel_id IS NOT DISTINCT FROM $3::uuid AND principal_id IS NOT DISTINCT FROM $4::uuid",
    )
    .bind(company.as_uuid())
    .bind(source.as_uuid())
    .bind(target.desk_id())
    .bind(target.principal_id().map(|id| id.as_uuid()))
    .fetch_optional(&mut *tx)
    .await?;
    let Some((hop_count, trace_targets)) = recorded else {
        return Ok(None);
    };
    Ok(Some(SourceRelay {
        hop_count: u32::try_from(hop_count)
            .map_err(|_| refused("the source's recorded relay trace cannot be read"))?,
        trace_targets: serde_json::from_value(trace_targets)
            .map_err(|_| refused("the source relay trace is invalid"))?,
    }))
}

/// Resolve the same exact-source route used by explicit message.reply.
pub(super) async fn final_reply_route_on(
    db: &mut PgConnection,
    scope: ActionScope,
    source: CanonicalMessageId,
) -> AppResult<MessageRoutes> {
    let run = run_route_on(db, scope).await?;
    let target = reply_target_on(db, scope.company, &run, source).await?;
    let author = message_author_on(db, scope, MessageAuthorSource::Final, run.association).await?;
    Ok(MessageRoutes {
        company_slug: run.company_slug,
        correlation_id: run.correlation_id,
        targets: vec![target],
        author,
        delivery_policy: run.delivery_policy,
    })
}
