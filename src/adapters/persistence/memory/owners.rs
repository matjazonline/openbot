//! Resolve live application identities to the principal that owns memory.
use super::*;
use crate::entities::transport::PrincipalId;
use sqlx::PgConnection;

pub(crate) async fn agent_principal_on(
    db: &mut PgConnection,
    company: Uuid,
    agent: Uuid,
) -> AppResult<PrincipalId> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2 AND kind='agent'",
    )
    .bind(company)
    .bind(agent)
    .fetch_optional(db)
    .await?
    .map(PrincipalId::new)
    .ok_or_else(|| AppError::NotFound("Agent memory principal is unavailable".into()))
}

pub(crate) async fn person_principal_on(
    db: &mut PgConnection,
    company: Uuid,
    user: Uuid,
) -> AppResult<PrincipalId> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(company)
    .bind(user)
    .fetch_optional(db)
    .await?
    .map(PrincipalId::new)
    .ok_or_else(|| AppError::NotFound("Personal memory principal is unavailable".into()))
}
