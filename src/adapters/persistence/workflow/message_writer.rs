//! Shared canonical destination storage; each caller owns its transaction and authorization.
use super::*;
use crate::adapters::persistence::delivery::enqueue::insert_delivery_on;
use crate::adapters::persistence::thread::{MessageStampSource, insert_message_on};
use crate::application::transport::queue::DeliveryCreation;
use crate::entities::transport::DeliveryId;
use sqlx::{PgConnection, Postgres, Transaction};

pub(in crate::adapters::persistence::workflow) struct CanonicalDestinationReceipt {
    pub(in crate::adapters::persistence::workflow) message: CanonicalMessageId,
    pub(in crate::adapters::persistence::workflow) delivery: Option<DeliveryId>,
}

/// Writes canonical content and transport work without inventing an action/attempt owner.
pub(in crate::adapters::persistence::workflow) async fn write_destination_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActionScope,
    destination: &PlannedDestination,
) -> AppResult<CanonicalDestinationReceipt> {
    create_thread_on(tx, scope.company, destination).await?;
    let inserted = insert_message_on(
        tx,
        &destination.message,
        MessageStampSource::Workflow {
            company: scope.company,
            run: scope.run,
        },
    )
    .await?;
    if inserted.canonical_id != destination.message.id {
        return Err(AppError::Conflict(
            "Workflow message identity already belongs to another message".into(),
        ));
    }
    let delivery = match &destination.delivery {
        Some(delivery) => match insert_delivery_on(tx, delivery).await? {
            DeliveryCreation::Created(id) if id == delivery.id => Some(id),
            _ => {
                return Err(AppError::Conflict(
                    "Workflow message delivery key already belongs to another delivery".into(),
                ));
            }
        },
        None => None,
    };
    Ok(CanonicalDestinationReceipt {
        message: inserted.canonical_id,
        delivery,
    })
}

async fn create_thread_on(
    db: &mut PgConnection,
    company: CompanyId,
    destination: &PlannedDestination,
) -> AppResult<()> {
    let Some(thread) = &destination.new_thread else {
        return Ok(());
    };
    sqlx::query("INSERT INTO threads (id, company_id, channel_id, subject, principal_id, goal_unit_id) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(thread.id)
        .bind(company.as_uuid())
        .bind(thread.target.desk_id())
        .bind(&thread.subject)
        .bind(thread.target.principal_id().map(|id| id.as_uuid()))
        .bind(thread.goal_unit_id)
        .execute(&mut *db)
        .await?;
    let mut participants = Vec::new();
    if destination.kind == DestinationKind::User {
        participants.extend(destination.principal_id);
    }
    participants.extend(thread.target.principal_id().map(|id| id.as_uuid()));
    participants.sort_unstable();
    participants.dedup();
    for principal in participants {
        sqlx::query("INSERT INTO thread_principals (company_id,channel_id,thread_id,principal_id,role,target_principal_id) VALUES ($1,$2,$3,$4,'participant',$5)")
            .bind(company.as_uuid()).bind(thread.target.desk_id()).bind(thread.id).bind(principal)
            .bind(thread.target.principal_id().map(|id| id.as_uuid())).execute(&mut *db).await?;
    }
    Ok(())
}

pub(in crate::adapters::persistence::workflow) async fn recheck_routes_on(
    tx: &mut PgConnection,
    company: CompanyId,
    plan: &MessagePlan,
) -> AppResult<()> {
    let mut checks: Vec<&RouteCheck> = plan.destinations.iter().map(|d| &d.check).collect();
    checks.sort_by_key(|check| match check {
        RouteCheck::Channel { channel } => (0, *channel),
        RouteCheck::Member { user, .. } => (1, *user),
        RouteCheck::Source { source, .. } => (2, source.as_uuid()),
    });
    for check in checks {
        let current = match check {
            RouteCheck::Channel { channel } => channel_exists_on(tx, company, *channel).await?,
            RouteCheck::Member {
                user,
                principal,
                email,
                sender,
            } => {
                target_exists_on(tx, company, *sender).await?
                    && member_on(tx, company, *user, true)
                        .await?
                        .is_some_and(|member| {
                            member.principal == *principal && member.email == *email
                        })
            }
            RouteCheck::Source {
                source,
                thread,
                target,
            } => sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM thread_messages WHERE company_id=$1 AND message_id=$2 \
                   AND thread_id=$3 AND channel_id IS NOT DISTINCT FROM $4::uuid \
                   AND principal_id IS NOT DISTINCT FROM $5::uuid FOR KEY SHARE)",
            )
            .bind(company.as_uuid())
            .bind(source.as_uuid())
            .bind(thread)
            .bind(target.desk_id())
            .bind(target.principal_id().map(|id| id.as_uuid()))
            .fetch_one(&mut *tx)
            .await?,
        };
        if !current {
            return Err(AppError::BadRequest(
                "Workflow message destination changed before publication".into(),
            ));
        }
    }
    Ok(())
}

async fn channel_exists_on(
    tx: &mut PgConnection,
    company: CompanyId,
    channel: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM channels WHERE company_id=$1 AND id=$2 FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .fetch_optional(&mut *tx)
    .await?
    .is_some())
}

async fn target_exists_on(
    tx: &mut PgConnection,
    company: CompanyId,
    target: AddressTarget,
) -> AppResult<bool> {
    match target {
        AddressTarget::Desk(id) => channel_exists_on(tx, company, id).await,
        AddressTarget::Principal(id) => Ok(sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM principals WHERE company_id=$1 AND id=$2 AND kind='agent' FOR KEY SHARE",
        ).bind(company.as_uuid()).bind(id.as_uuid()).fetch_optional(tx).await?.is_some()),
    }
}
