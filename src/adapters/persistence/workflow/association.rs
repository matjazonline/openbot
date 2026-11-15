use super::*;
use crate::entities::transport::PrincipalId;

pub(in crate::adapters::persistence) async fn authorize_thread(
    tx: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    thread: Uuid,
) -> AppResult<()> {
    let columns = sqlx::query_as::<_, association::AssociationColumns>(
        "SELECT channel_id, principal_id, id AS thread_id FROM threads WHERE company_id=$1 AND id=$2",
    ).bind(company.as_uuid()).bind(thread).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    authorize_association(
        tx,
        company,
        actor,
        association::association_from_columns(columns)?,
    )
    .await
}

/// Company/member/principal locks are acquired by authorize_company first.
/// Channel settings writers update the channel before replacing grants; direct
/// grant revocation conflicts with the granted row's SHARE lock below.
pub(in crate::adapters::persistence) async fn authorize_association(
    tx: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> AppResult<()> {
    let Some(target) = association.target() else {
        return Ok(());
    };
    match target {
        AddressTarget::Desk(channel) => authorize_channel(tx, company, actor, channel).await?,
        AddressTarget::Principal(principal) => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR SHARE",
            )
            .bind(company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
            let visible: bool = sqlx::query_scalar(
                "SELECT principal_direct_can_view($1,$2,actor.id) FROM principals AS actor \
                 WHERE actor.company_id=$1 AND actor.user_id=$3 AND actor.kind='person'",
            )
            .bind(company.as_uuid())
            .bind(principal.as_uuid())
            .bind(actor.user_id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
            if !visible {
                return Err(missing());
            }
        }
    }
    if let Some(thread) = association.thread_id() {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM threads WHERE company_id=$1 AND id=$2 \
             AND channel_id IS NOT DISTINCT FROM $3::uuid \
             AND principal_id IS NOT DISTINCT FROM $4::uuid FOR SHARE",
        )
        .bind(company.as_uuid())
        .bind(thread.as_uuid())
        .bind(target.desk_id())
        .bind(target.principal_id().map(|id| id.as_uuid()))
        .fetch_optional(tx)
        .await?
        .ok_or_else(missing)?;
    }
    Ok(())
}

async fn authorize_channel(
    tx: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    channel: Uuid,
) -> AppResult<()> {
    let mode: String = sqlx::query_scalar(
        "SELECT access_mode FROM channels WHERE company_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(missing)?;
    let owner: bool = sqlx::query_scalar("SELECT user_id = $2 FROM companies WHERE id = $1")
        .bind(company.as_uuid())
        .bind(actor.user_id())
        .fetch_one(&mut *tx)
        .await?;
    match mode.as_str() {
        "team" | "public" => {}
        "allowlist" if owner => {}
        "allowlist" => {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT access_grant.principal_id FROM channel_principal_grants AS access_grant \
                 JOIN principals AS principal ON principal.company_id = access_grant.company_id \
                   AND principal.id = access_grant.principal_id \
                 JOIN users AS account ON account.id = principal.user_id \
                 WHERE access_grant.company_id = $1 AND access_grant.channel_id = $2 \
                   AND principal.user_id = $3 AND account.email_verified_at IS NOT NULL \
                   AND access_grant.capability = 'view' \
                 FOR SHARE OF access_grant, account",
            )
            .bind(company.as_uuid())
            .bind(channel)
            .bind(actor.user_id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, sqlx::FromRow)]
pub(in crate::adapters::persistence) struct AssociationColumns {
    pub channel_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
}

pub(in crate::adapters::persistence) fn association_columns(
    association: RelatedAssociation,
) -> AssociationColumns {
    let target = association.target();
    AssociationColumns {
        channel_id: target.and_then(AddressTarget::desk_id),
        principal_id: target
            .and_then(AddressTarget::principal_id)
            .map(PrincipalId::as_uuid),
        thread_id: association.thread_id().map(RelatedThreadId::as_uuid),
    }
}

/// A thread always has exactly one tenant-scoped target; a NULL desk alone is not company scope.
pub(in crate::adapters::persistence) fn association_from_columns(
    columns: AssociationColumns,
) -> AppResult<RelatedAssociation> {
    if columns.channel_id.is_none() && columns.principal_id.is_none() {
        return if columns.thread_id.is_none() {
            Ok(RelatedAssociation::Company)
        } else {
            Err(invalid())
        };
    }
    let target = AddressTarget::from_parts(
        columns.channel_id,
        columns.principal_id.map(PrincipalId::new),
    )
    .ok_or_else(invalid)?;
    Ok(match (target, columns.thread_id) {
        (target, Some(thread)) => RelatedAssociation::Thread {
            target,
            thread_id: RelatedThreadId::new(thread),
        },
        (AddressTarget::Desk(channel), None) => {
            RelatedAssociation::Channel(RelatedChannelId::new(channel))
        }
        (AddressTarget::Principal(principal), None) => RelatedAssociation::Principal(principal),
    })
}
