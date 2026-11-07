//! Current transport authority under the same transaction as final effect admission.
use super::*;
use crate::entities::transport::TransportKind;
use sqlx::PgConnection;

pub(super) struct BindingScope {
    pub company: Uuid,
    pub channel: Uuid,
    pub binding: Uuid,
    pub transport: TransportKind,
}

pub(super) async fn active_on(db: &mut PgConnection, scope: BindingScope) -> AppResult<bool> {
    let installation: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT installation_id FROM channel_bindings \
        WHERE company_id=$1 AND channel_id=$2 AND id=$3 AND transport=$4 FOR SHARE",
    )
    .bind(scope.company)
    .bind(scope.channel)
    .bind(scope.binding)
    .bind(scope.transport.as_str())
    .fetch_optional(&mut *db)
    .await?;
    let Some(installation) = installation else {
        return Ok(false);
    };
    if let Some(installation) = installation {
        sqlx::query(
            "SELECT id FROM integration_installations WHERE company_id=$1 AND id=$2 FOR SHARE",
        )
        .bind(scope.company)
        .bind(installation)
        .fetch_optional(&mut *db)
        .await?;
    }
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM ({}) AS active \
        WHERE active.company_id=$1 AND active.channel_id=$2 AND active.id=$3 AND active.transport=$4)",
        super::super::integration::active_binding_select()
    );
    Ok(sqlx::query_scalar(&query)
        .bind(scope.company)
        .bind(scope.channel)
        .bind(scope.binding)
        .bind(scope.transport.as_str())
        .fetch_one(db)
        .await?)
}
