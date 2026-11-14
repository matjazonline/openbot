use super::*;
use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gates_on,
};
use crate::adapters::persistence::workflow::{association, authority};
use crate::application::workflow::{RelatedAssociation, RelatedChannelId, RelatedThreadId};
use crate::entities::value_objects::EmailAddress;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Route {
    pub agent_id: Uuid,
    pub desk_id: Uuid,
    pub binding_id: Uuid,
    pub binding_revision: i64,
    pub run_as_user_id: Uuid,
    pub settings_revision: i64,
    pub transport_binding_id: Uuid,
    #[sqlx(try_from = "String")]
    pub address: EmailAddress,
}

pub(super) async fn read_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    principal: Uuid,
) -> AppResult<Option<Route>> {
    Ok(sqlx::query_as(
        "SELECT agent_id,desk_id,binding_id,binding_revision,run_as_user_id, \
        settings_revision,transport_binding_id,address FROM workflow_escalation_agent_route($1,$2)",
    )
    .bind(event.company_id)
    .bind(principal)
    .fetch_optional(db)
    .await?)
}

pub(super) enum Ownership {
    Owned,
    Busy,
    Stale,
}

pub(super) async fn lock_on(
    db: &mut PgConnection,
    company: Uuid,
    hint: Option<&Route>,
) -> AppResult<Ownership> {
    if let Some(route) = hint {
        match acquire_channel_gates_on(
            db,
            [ChannelGateKey::new(company, route.desk_id)],
            ChannelGateAccess::Admit,
        )
        .await
        {
            Ok(()) => {}
            Err(AppError::DatabaseTimeout(_)) => return Ok(Ownership::Busy),
            Err(error) => return Err(error),
        }
    }
    match lock_rows_on(db, company, hint).await {
        Ok(owned) => Ok(if owned {
            Ownership::Owned
        } else {
            Ownership::Stale
        }),
        Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03") => {
            Ok(Ownership::Busy)
        }
        Err(error) => Err(error.into()),
    }
}

async fn lock_rows_on(
    db: &mut PgConnection,
    company: Uuid,
    hint: Option<&Route>,
) -> Result<bool, sqlx::Error> {
    let accounts: Vec<_> = hint.iter().map(|route| route.run_as_user_id).collect();
    let locked: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE NOWAIT")
            .bind(&accounts)
            .fetch_all(&mut *db)
            .await?;
    let company: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(company)
            .fetch_optional(db)
            .await?;
    Ok(locked == accounts && company.is_some())
}

pub(super) async fn eligible_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    source: &NoticeSource,
    route: &Route,
) -> AppResult<bool> {
    let actor = WorkflowActor::authenticated(route.run_as_user_id)?;
    let company = CompanyId::new(event.company_id);
    let Some(scope) = case_scope_on(db, company, EscalationId::new(source.case_id)).await? else {
        return Ok(false);
    };
    let authorized = async {
        authority::authorize_company(db, company, actor).await?;
        association::authorize_association(
            db,
            company,
            actor,
            RelatedAssociation::Channel(RelatedChannelId::new(route.desk_id)),
        )
        .await?;
        if let Some(channel) = scope.channel_id {
            let association = match scope.thread_id {
                Some(thread) => RelatedAssociation::Thread {
                    channel_id: RelatedChannelId::new(channel),
                    thread_id: RelatedThreadId::new(thread),
                },
                None => RelatedAssociation::Channel(RelatedChannelId::new(channel)),
            };
            association::authorize_association(db, company, actor, association).await?;
        }
        payload_access_on(db, &scope, actor).await
    }
    .await;
    match authorized {
        Ok(access) => Ok(access == CasePayloadAccess::Visible),
        Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(false),
        Err(error) => Err(error),
    }
}
