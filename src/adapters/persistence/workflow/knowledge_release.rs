//! One evaluator for intent freeze and every actual new-effect boundary.
use super::*;
use crate::application::workflow::{
    actions::*,
    message::{
        DestinationKind, MessageArguments, MessageDestinationInput, MessageKind, MessagePlan,
    },
};
use crate::entities::access_level::{AccessLevel, may_release};
use crate::entities::value_objects::EmailAddress;
use serde_json::{Value, json};

/// Concrete transport addresses are part of the publication snapshot. Current routing by user
/// id cannot authorize an address retired between route validation and this MVCC statement.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MessageRecipient {
    kind: DestinationKind,
    id: Uuid,
    address: Option<EmailAddress>,
}
impl MessageRecipient {
    pub(super) fn reply(id: Uuid, address: Option<EmailAddress>) -> Self {
        Self {
            kind: DestinationKind::Reply,
            id,
            address,
        }
    }
}
pub(super) enum MessageRecipients<'a> {
    Current,
    Frozen(&'a [MessageRecipient]),
}
pub(super) fn publication_recipients(plan: &MessagePlan) -> AppResult<Vec<MessageRecipient>> {
    if plan.destinations.len() > crate::application::workflow::registry::MAX_MESSAGE_DESTINATIONS {
        return Err(AppError::BadRequest("Too many message destinations".into()));
    }
    Ok(plan
        .destinations
        .iter()
        .map(|d| MessageRecipient {
            kind: d.kind,
            id: d.destination_id,
            address: d.recipient.clone(),
        })
        .collect())
}
fn message_receivers(
    arguments: MessageArguments,
    recipients: MessageRecipients<'_>,
) -> AppResult<Value> {
    let destinations = match arguments {
        MessageArguments::Send { destinations, .. } => destinations,
        MessageArguments::Reply { source, .. } => vec![MessageDestinationInput {
            kind: DestinationKind::Reply,
            id: source.as_uuid(),
        }],
    };
    let frozen = match recipients {
        MessageRecipients::Current => None,
        MessageRecipients::Frozen(rows) => Some(rows),
    };
    if frozen.is_some_and(|rows| rows.len() != destinations.len()) {
        return Err(conflict());
    }
    let mut requested = Vec::with_capacity(destinations.len());
    for (ordinal, destination) in destinations.into_iter().enumerate() {
        let actual = frozen.map(|rows| &rows[ordinal]);
        if actual.is_some_and(|row| row.kind != destination.kind || row.id != destination.id) {
            return Err(conflict());
        }
        requested.push(json!({"kind":destination.kind.as_str(),"id":destination.id,"frozen":actual.is_some(),"recipient":actual.and_then(|row|row.address.as_ref()).map(|email|email.as_str())}));
    }
    Ok(json!(requested))
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ReleaseLevels {
    pub content: AccessLevel,
    pub destination: AccessLevel,
}
#[derive(sqlx::FromRow)]
struct DestinationFact {
    valid: bool,
    clearance: String,
    kind: Option<String>,
    membership: Option<String>,
    access_level: Option<i16>,
    is_owner: bool,
}
fn destination(
    company: CompanyId,
    content: AccessLevel,
    fact: DestinationFact,
) -> AppResult<AccessLevel> {
    if !fact.valid {
        return Err(missing());
    }
    match fact.clearance.as_str() {
        "internal" => Ok(content),
        "email" | "thread" => match fact.kind {
            None => Ok(AccessLevel::PUBLIC),
            Some(kind) => super::knowledge::reader(
                company,
                super::knowledge::ReaderFact {
                    kind,
                    membership: fact.membership,
                    access_level: fact.access_level,
                    is_owner: fact.is_owner,
                },
            )?
            .ok_or_else(invalid),
        },
        _ => Err(invalid()),
    }
}
async fn message_destination_on(
    db: &mut PgConnection,
    action: &FrozenAction,
    content: AccessLevel,
    recipients: MessageRecipients<'_>,
) -> AppResult<AccessLevel> {
    let scope = action.scope();
    let kind =
        MessageKind::parse(action.request().contract.contract.name.as_str()).ok_or_else(missing)?;
    let arguments = MessageArguments::parse(kind, &action.request().arguments)?;
    let requested = message_receivers(arguments, recipients)?;
    const MAX_FACTS: usize = crate::application::workflow::registry::MAX_MESSAGE_DESTINATIONS * 128;
    let facts = sqlx::query_as::<_, DestinationFact>(include_str!("knowledge_message.sql"))
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(json!(requested))
        .bind((MAX_FACTS + 1) as i64)
        .fetch_all(db)
        .await?;
    if facts.is_empty() {
        return Err(missing());
    }
    if facts.len() > MAX_FACTS {
        return Err(invalid());
    }
    let levels = facts
        .into_iter()
        .map(|fact| destination(scope.company, content, fact))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(crate::entities::access_level::audience_level(levels))
}

pub(super) async fn evaluate_on(
    db: &mut PgConnection,
    action: &FrozenAction,
    recipients: MessageRecipients<'_>,
) -> AppResult<ReleaseLevels> {
    let scope = action.scope();
    #[derive(sqlx::FromRow)]
    struct RunReleaseFact {
        content_level: i16,
        step_id: String,
    }
    let facts=sqlx::query_as::<_,RunReleaseFact>(
        "SELECT run.content_level::smallint AS content_level,execution.step_id FROM workflow_runs AS run \
         JOIN workflow_executions AS execution ON (execution.company_id,execution.run_id)=(run.company_id,run.id) \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3 FOR SHARE OF run")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_optional(&mut *db).await?.ok_or_else(missing)?;
    let content = AccessLevel::try_from(facts.content_level).map_err(|_| invalid())?;
    let destination = match &action.request().target {
        ActionTarget::Connection {
            resource,
            resource_kind,
        }
        | ActionTarget::Local {
            resource,
            resource_kind,
        } if matches!(resource_kind.as_str(), "http" | "mcp") => {
            let query = match resource_kind.as_str() {
                "http" => {
                    "SELECT access_level::smallint FROM workflow_http_connections WHERE company_id=$1 AND id=$2"
                }
                "mcp" => {
                    "SELECT access_level::smallint FROM company_mcp_connections WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL"
                }
                _ => return Err(missing()),
            };
            let level: i16 = sqlx::query_scalar(query)
                .bind(scope.company.as_uuid())
                .bind(resource)
                .fetch_optional(&mut *db)
                .await?
                .ok_or_else(missing)?;
            AccessLevel::try_from(level).map_err(|_| invalid())?
        }
        ActionTarget::Connection { .. } => return Err(missing()),
        ActionTarget::Local {
            resource,
            resource_kind,
        } if resource_kind.as_str() == "messaging" => {
            if *resource != scope.company.as_uuid() {
                return Err(missing());
            }
            message_destination_on(db, action, content, recipients).await?
        }
        ActionTarget::Local { .. } => AccessLevel::PUBLIC,
    };
    if !may_release(content, destination) {
        tracing::warn!(company_id=%scope.company.as_uuid(),run_id=%scope.run.as_uuid(),execution_id=%scope.execution.as_uuid(),step_id=%facts.step_id,content_level=content.as_i16(),destination_level=destination.as_i16(),code=crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT,"Workflow release refused");
        return Err(AppError::KnowledgeRelease {
            content,
            destination,
        });
    }
    Ok(ReleaseLevels {
        content,
        destination,
    })
}
