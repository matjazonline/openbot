//! Company-first TypeSafe ownership, bounded management and current encrypted credentials.
use super::*;
use crate::adapters::persistence::credentials::CredentialContext;
use crate::application::workflow::{
    binding::{ResourceReadiness, ResourceStatus},
    jev::*,
};
use secrecy::SecretString;
use std::collections::BTreeSet;

#[path = "jev_authorization.rs"]
pub(super) mod runtime;

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    company_id: Uuid,
    id: Uuid,
    enabled: bool,
    revision: i64,
    models: Vec<String>,
}
impl ConnectionRow {
    fn decode(self) -> AppResult<JevConnection> {
        let models = self
            .models
            .iter()
            .map(|s| JevModel::parse(s))
            .collect::<AppResult<BTreeSet<_>>>()?;
        validate_models(&models)?;
        Ok(JevConnection {
            company: CompanyId::new(self.company_id),
            id: self.id,
            enabled: self.enabled,
            revision: self.revision,
            models,
            ready: self.enabled,
        })
    }
}
pub(super) async fn connection_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: Uuid,
) -> AppResult<Option<JevConnection>> {
    sqlx::query_as::<_, ConnectionRow>("SELECT company_id,id,enabled,revision,models FROM company_typesafe_connections WHERE company_id=$1 AND id=$2 FOR SHARE")
        .bind(company.as_uuid()).bind(id).fetch_optional(db).await?.map(ConnectionRow::decode).transpose()
}
pub(super) async fn status_on(
    db: &mut PgConnection,
    company: CompanyId,
    id: Uuid,
) -> AppResult<Option<ResourceStatus>> {
    let Some(connection) = connection_on(db, company, id).await? else {
        return Ok(None);
    };
    Ok(Some(ResourceStatus {
        id: RuntimeResourceId::new(id),
        company_id: company,
        kind: TypeName::parse("typesafe").map_err(|_| invalid())?,
        supported_contracts: connection
            .models
            .iter()
            .map(JevModel::resource_contract)
            .collect(),
        authorized: true,
        readiness: if connection.enabled {
            ResourceReadiness::Ready
        } else {
            ResourceReadiness::Revoked
        },
    }))
}
async fn edit_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    id: Uuid,
    expected: i64,
) -> AppResult<()> {
    authority::authorize_company(db, company, actor).await?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM company_typesafe_connections WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(company.as_uuid()).bind(id).fetch_optional(db).await?.ok_or_else(missing)?;
    if revision != expected || revision == i64::MAX {
        return Err(conflict());
    }
    Ok(())
}
fn model_strings(models: &BTreeSet<JevModel>) -> Vec<&str> {
    models.iter().map(JevModel::as_str).collect()
}

#[async_trait]
impl JevConnections for PostgresPersistence {
    async fn list_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
    ) -> AppResult<Vec<JevConnection>> {
        let mut tx = http::begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let rows = sqlx::query_as::<_, ConnectionRow>("SELECT company_id,id,enabled,revision,models FROM company_typesafe_connections WHERE company_id=$1 ORDER BY id LIMIT 257")
            .bind(company.as_uuid()).fetch_all(&mut *tx).await?;
        if rows.len() > MAX_CONNECTIONS as usize {
            return Err(invalid());
        }
        let result = rows
            .into_iter()
            .map(ConnectionRow::decode)
            .collect::<AppResult<_>>()?;
        tx.commit().await?;
        Ok(result)
    }
    async fn read_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
    ) -> AppResult<JevConnection> {
        let mut tx = http::begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let result = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(result)
    }
    async fn create_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: JevConnectionWrite,
    ) -> AppResult<JevConnection> {
        validate_models(&write.models)?;
        validate_secret(&write.secret)?;
        let mut tx = http::begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM company_typesafe_connections WHERE company_id=$1",
        )
        .bind(company.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if count >= MAX_CONNECTIONS {
            return Err(invalid());
        }
        let id = Uuid::new_v4();
        let envelope = self.credential_cipher()?.seal_envelope(
            &CredentialContext::typesafe_credential(company.as_uuid(), id),
            &write.secret,
        )?;
        sqlx::query("INSERT INTO company_typesafe_connections(company_id,id,enabled,models,envelope) VALUES($1,$2,$3,$4,$5)")
            .bind(company.as_uuid()).bind(id).bind(write.enabled).bind(model_strings(&write.models)).bind(envelope).execute(&mut *tx).await?;
        let result = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(result)
    }
    async fn update_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        update: JevConnectionUpdate,
    ) -> AppResult<JevConnection> {
        validate_models(&update.models)?;
        let mut tx = http::begin_http(self).await?;
        edit_on(&mut tx, company, actor, id, update.expected_revision).await?;
        sqlx::query("UPDATE company_typesafe_connections SET enabled=$3,models=$4,revision=revision+1 WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid()).bind(id).bind(update.enabled).bind(model_strings(&update.models)).execute(&mut *tx).await?;
        let result = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(result)
    }
    async fn rotate_jev(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        expected_revision: i64,
        secret: SecretString,
    ) -> AppResult<JevConnection> {
        validate_secret(&secret)?;
        let mut tx = http::begin_http(self).await?;
        edit_on(&mut tx, company, actor, id, expected_revision).await?;
        let envelope = self.credential_cipher()?.seal_envelope(
            &CredentialContext::typesafe_credential(company.as_uuid(), id),
            &secret,
        )?;
        sqlx::query("UPDATE company_typesafe_connections SET envelope=$3,revision=revision+1 WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid()).bind(id).bind(envelope).execute(&mut *tx).await?;
        let result = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(result)
    }
}
/// Publication has slots, not IDs: the selected ID is checked later by binding/admission.
pub(super) async fn publication_on(
    db: &mut PgConnection,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    let requirements = required_models(bundle)?;
    for model in requirements.values().flatten().collect::<BTreeSet<_>>() {
        let available: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM company_typesafe_connections WHERE company_id=$1 AND enabled AND $2=ANY(models))")
            .bind(bundle.company_id().as_uuid()).bind(model.as_str()).fetch_one(&mut *db).await?;
        if !available {
            return Err(revoked());
        }
    }
    Ok(())
}
pub(super) fn revoked() -> AppError {
    AppError::BadRequest("Selected TypeSafe connection or model is unavailable".into())
}
