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
#[path = "message_receipts.rs"]
pub(crate) mod receipts;

const MESSAGING: &str = "messaging";

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
        super::authority::authorize_company(tx, request.scope.company, authority.actor).await?;
        let kind =
            MessageKind::parse(request.contract.contract.name.as_str()).ok_or_else(missing)?;
        Ok(action_dispatch::LockedActionAuthority {
            resource: messaging_status(request.scope.company)?,
            policy: CurrentActionPolicy {
                tool: builtin_message_snapshot(request.scope.company, kind)?,
                approval_required: false,
            },
        })
    }
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
        supported_contracts: [MessageKind::Send, MessageKind::Reply]
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
        inputs
            .as_object_mut()
            .ok_or_else(invalid)?
            .remove("approval");
        let request = message_request(scope, &facts, message_kind, inputs)?;
        tx.commit().await?;
        Ok(request)
    }

    async fn message_tool_request(
        &self,
        scope: ActionScope,
        kind: MessageKind,
        arguments: Value,
    ) -> AppResult<ActionRequest> {
        let mut tx = begin_http(self).await?;
        let facts = facts(&mut tx, scope).await?;
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
        })
    }
}

/// The run's explicit channel/thread scope and the company's slug.
pub(super) struct RunRoute {
    company_slug: CompanySlug,
    correlation_id: CorrelationId,
    channel: Option<Uuid>,
    thread: Option<Uuid>,
}

async fn run_route_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
) -> AppResult<RunRoute> {
    let (slug, correlation, channel, thread): (String, Uuid, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT company.slug, run.correlation_id, run.channel_id, run.thread_id FROM workflow_runs AS run \
         JOIN companies AS company ON company.id=run.company_id WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    Ok(RunRoute {
        company_slug: CompanySlug::from(slug),
        correlation_id: CorrelationId::from(correlation),
        channel,
        thread,
    })
}

pub(super) async fn channel_route_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    channel: Uuid,
) -> AppResult<Option<ChannelRoute>> {
    let row: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT channel.id, channel.name, (SELECT alias.slug::text FROM channel_slugs AS alias \
           WHERE alias.channel_id=channel.id AND alias.is_primary) \
         FROM channels AS channel WHERE channel.company_id=$1 AND channel.id=$2",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.and_then(|(id, name, slug)| {
        Some(ChannelRoute {
            id,
            slug: ChannelSlug::from(slug?),
            name,
        })
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
        "SELECT principal.id, account.email FROM company_members AS member \
         JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id \
         JOIN users AS account ON account.id=member.user_id \
         WHERE member.company_id=$1 AND member.user_id=$2{lock}"
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
            let channel = run
                .channel
                .ok_or_else(|| refused("emailing a user requires the run's channel"))?;
            let sender = channel_route_on(tx, company, channel)
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
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    source: CanonicalMessageId,
    run_channel: Option<Uuid>,
    run_thread: Option<Uuid>,
) -> AppResult<Option<(Uuid, Uuid)>> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT association.thread_id, association.channel_id FROM thread_messages AS association \
         WHERE association.company_id=$1 AND association.message_id=$2 \
           AND ($3::uuid IS NULL OR association.thread_id=$3) \
           AND ($4::uuid IS NULL OR association.channel_id=$4) \
         ORDER BY association.id LIMIT 2",
    )
    .bind(company.as_uuid())
    .bind(source.as_uuid())
    .bind(run_thread)
    .bind(run_channel)
    .fetch_all(&mut **tx)
    .await?;
    Ok(match rows.as_slice() {
        [only] => Some(*only),
        _ => None,
    })
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
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    run: &RunRoute,
    source: CanonicalMessageId,
) -> AppResult<RouteTarget> {
    let (thread, channel) = source_association_on(tx, company, source, run.channel, run.thread)
        .await?
        .ok_or_else(|| {
            refused("the source message has no unambiguous association in this run's scope")
        })?;
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
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| refused("the source message does not exist"))?;
    if facts.audience == "internal_only" {
        return Ok(RouteTarget::ReplyInternal {
            source,
            thread,
            channel,
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
    let sender_channel = channel_route_on(tx, company, channel)
        .await?
        .ok_or_else(|| refused("the source channel cannot send email"))?;
    let relay = source_relay_on(tx, company, source, channel).await?;
    Ok(RouteTarget::ReplyEmail {
        source,
        thread,
        sender: sender_channel,
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

/// The relay trace the source arrived with, as its inbound task recorded it: the only place it
/// is kept (`InboundTaskPayloadV1`). A payload that does not decode is refused, never guessed.
async fn source_relay_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    source: CanonicalMessageId,
    channel: Uuid,
) -> AppResult<Option<SourceRelay>> {
    use crate::application::transport::task_payload::InboundTaskPayload;
    let payload: Option<Value> = sqlx::query_scalar(
        "SELECT payload FROM background_tasks WHERE company_id=$1 AND source_message_uuid=$2 \
           AND channel_id=$3 AND queue_kind='legacy'",
    )
    .bind(company.as_uuid())
    .bind(source.as_uuid())
    .bind(channel)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let payload = InboundTaskPayload::decode(&payload)
        .map_err(|_| refused("the source's recorded relay trace cannot be read"))?;
    let recorded = payload.identifiers();
    Ok(Some(SourceRelay {
        hop_count: recorded.hop_count,
        trace_channels: recorded.trace_channels.to_vec(),
    }))
}

/// Authoring selection of the trusted built-ins. Requires current owner/admin authority.
pub struct MessagePublicationDirectory {
    pub persistence: PostgresPersistence,
}
#[async_trait]
impl PublicationDirectory for MessagePublicationDirectory {
    async fn capture(
        &self,
        actor: WorkflowActor,
        draft: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        let mut tx = begin_http(&self.persistence).await?;
        super::authority::authorize_company(&mut tx, draft.company(), actor).await?;
        tx.commit().await?;
        Ok(PublicationDependencies {
            snapshots: publication::DependencySnapshots {
                tools: vec![
                    builtin_message_snapshot(draft.company(), MessageKind::Send)?,
                    builtin_message_snapshot(draft.company(), MessageKind::Reply)?,
                ],
                ..Default::default()
            },
            children: vec![],
        })
    }
}
