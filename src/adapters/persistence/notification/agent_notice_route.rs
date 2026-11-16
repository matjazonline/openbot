use super::*;
use crate::adapters::persistence::workflow::{association, authority};
use crate::application::workflow::RelatedAssociation;
use crate::entities::transport::PrincipalId;
use crate::entities::value_objects::ChannelSlug;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct Route {
    pub agent_id: Uuid,
    pub principal_id: Uuid,
    pub binding_id: Uuid,
    pub binding_revision: i64,
    pub run_as_user_id: Uuid,
    pub settings_revision: i64,
    pub transport_binding_id: Uuid,
    #[sqlx(try_from = "String")]
    pub address_slug: ChannelSlug,
}

pub(super) async fn read_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
    principal: Uuid,
) -> AppResult<Option<Route>> {
    Ok(sqlx::query_as(
        "SELECT agent_id,principal_id,binding_id,binding_revision,run_as_user_id, \
        settings_revision,transport_binding_id,address_slug FROM workflow_escalation_agent_route($1,$2)",
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
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *db)
        .await?;
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
    if let Some(route) = hint {
        let locked: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR SHARE")
                .bind(company)
                .bind(route.principal_id)
                .fetch_optional(&mut *db)
                .await?;
        if locked.is_none() {
            return Ok(false);
        }
    }
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
            RelatedAssociation::Principal(PrincipalId::new(route.principal_id)),
        )
        .await?;
        let source_association =
            association::association_from_columns(association::AssociationColumns {
                channel_id: scope.channel_id,
                principal_id: scope.principal_id,
                thread_id: scope.thread_id,
            })?;
        association::authorize_association(db, company, actor, source_association).await?;
        payload_access_on(db, &scope, actor).await
    }
    .await;
    match authorized {
        Ok(access) => Ok(access == CasePayloadAccess::Visible),
        Err(AppError::NotFound(_) | AppError::Forbidden(_)) => Ok(false),
        Err(error) => Err(error),
    }
}
