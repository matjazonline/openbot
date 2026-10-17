//! Current company HTTP ownership, encrypted secrets and trusted operation registrations.
use super::invalid;
use super::*;
use crate::adapters::persistence::credentials::envelope::CredentialContext;
use crate::application::workflow::{
    actions::*,
    binding::{ResourceReadiness, ResourceStatus},
    http::*,
};
use secrecy::SecretString;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) async fn begin_http(
    persistence: &PostgresPersistence,
) -> AppResult<Transaction<'_, Postgres>> {
    let mut tx = persistence.pool.begin().await?;
    sqlx::query(
        "SELECT set_config('lock_timeout','5s',true),set_config('statement_timeout','5s',true)",
    )
    .execute(&mut *tx)
    .await?;
    Ok(tx)
}
#[path = "http_publication.rs"]
mod publication_owner;
pub use publication_owner::HttpPublicationDirectory;
pub use publication_owner::HttpPublicationSelection;
#[path = "http_runtime.rs"]
mod runtime;

pub(super) async fn registered_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    wanted: &publication::ToolSnapshot,
) -> AppResult<Uuid> {
    let mut normalized = wanted.clone();
    normalized.contract.connection = None;
    HttpOperation {
        tool: normalized.clone(),
        approval_required: false,
    }
    .validate(company)?;
    let snapshot = serde_json::to_value(normalized).map_err(|_| invalid())?;
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT operation.connection_id FROM workflow_http_operations AS operation JOIN workflow_http_connections AS connection ON connection.company_id=operation.company_id AND connection.id=operation.connection_id WHERE operation.company_id=$1 AND operation.name=$2 AND operation.snapshot=$3 AND connection.enabled AND (NOT connection.bearer OR EXISTS(SELECT 1 FROM workflow_http_credentials AS credential WHERE credential.company_id=connection.company_id AND credential.connection_id=connection.id)) ORDER BY operation.connection_id LIMIT 2")
        .bind(company.as_uuid()).bind(wanted.contract.name.as_str()).bind(snapshot).fetch_all(&mut **tx).await?;
    // A tool without a binding must have exactly one owner; ambiguous routing fails closed.
    if ids.is_empty() || (wanted.contract.connection.is_none() && ids.len() != 1) {
        return Err(missing());
    }
    Ok(ids[0])
}

#[derive(sqlx::FromRow)]
struct ConnectionRow {
    company_id: Uuid,
    id: Uuid,
    origin: String,
    enabled: bool,
    bearer: bool,
    secret_set: bool,
    revision: i64,
}
impl ConnectionRow {
    fn decode(self) -> AppResult<HttpConnection> {
        Ok(HttpConnection {
            company: CompanyId::new(self.company_id),
            id: self.id,
            origin: HttpOrigin::parse(&self.origin)?,
            enabled: self.enabled,
            bearer: self.bearer,
            secret_set: self.secret_set,
            revision: self.revision,
        })
    }
}
pub(super) async fn connection_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: Uuid,
) -> AppResult<Option<HttpConnection>> {
    sqlx::query_as::<_, ConnectionRow>("SELECT connection.company_id, connection.id, connection.origin, connection.enabled, connection.bearer, connection.revision, EXISTS(SELECT 1 FROM workflow_http_credentials AS credential WHERE credential.company_id=connection.company_id AND credential.connection_id=connection.id) AS secret_set FROM workflow_http_connections AS connection WHERE connection.company_id=$1 AND connection.id=$2 FOR SHARE OF connection")
        .bind(company.as_uuid()).bind(id).fetch_optional(&mut **tx).await?.map(ConnectionRow::decode).transpose()
}
async fn edit_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    id: Uuid,
    revision: i64,
) -> AppResult<HttpConnection> {
    authority::authorize_company(tx, company, actor).await?;
    sqlx::query(
        "SELECT id FROM workflow_http_connections WHERE company_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(company.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    let connection = connection_on(tx, company, id).await?.ok_or_else(missing)?;
    if connection.revision != revision {
        return Err(conflict());
    }
    Ok(connection)
}
async fn revised(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: Uuid,
) -> AppResult<HttpConnection> {
    sqlx::query("UPDATE workflow_http_connections SET revision=revision+1 WHERE company_id=$1 AND id=$2 AND revision<9223372036854775807").bind(company.as_uuid()).bind(id).execute(&mut **tx).await?.rows_affected().eq(&1).then_some(()).ok_or_else(conflict)?;
    connection_on(tx, company, id).await?.ok_or_else(missing)
}

#[async_trait]
impl HttpConnections for PostgresPersistence {
    async fn create_http(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: HttpConnectionWrite,
    ) -> AppResult<HttpConnection> {
        let mut tx = begin_http(self).await?;
        authority::authorize_company(&mut tx, company, actor).await?;
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_http_connections WHERE company_id=$1",
        )
        .bind(company.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        if count >= 256 {
            return Err(invalid());
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO workflow_http_connections(company_id,id,origin,bearer) VALUES($1,$2,$3,$4)").bind(company.as_uuid()).bind(id).bind(write.origin.as_str()).bind(write.bearer).execute(&mut *tx).await?;
        let connection = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        tx.commit().await?;
        Ok(connection)
    }
    async fn set_http_enabled(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        enabled: bool,
    ) -> AppResult<HttpConnection> {
        let mut tx = begin_http(self).await?;
        edit_on(&mut tx, company, actor, id, revision).await?;
        sqlx::query(
            "UPDATE workflow_http_connections SET enabled=$3 WHERE company_id=$1 AND id=$2",
        )
        .bind(company.as_uuid())
        .bind(id)
        .bind(enabled)
        .execute(&mut *tx)
        .await?;
        let result = revised(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn replace_http_secret(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        secret: Option<SecretString>,
    ) -> AppResult<HttpConnection> {
        if let Some(secret) = &secret {
            validate_secret(secret)?;
        }
        let mut tx = begin_http(self).await?;
        let connection = edit_on(&mut tx, company, actor, id, revision).await?;
        if secret.is_some() && !connection.bearer {
            return Err(invalid());
        }
        if let Some(secret) = secret {
            let envelope = self.credential_cipher()?.seal_envelope(
                &CredentialContext::workflow_http_credential(company.as_uuid(), id),
                &secret,
            )?;
            sqlx::query("INSERT INTO workflow_http_credentials(company_id,connection_id,envelope) VALUES($1,$2,$3) ON CONFLICT(company_id,connection_id) DO UPDATE SET envelope=EXCLUDED.envelope").bind(company.as_uuid()).bind(id).bind(envelope).execute(&mut *tx).await?;
        } else {
            sqlx::query(
                "DELETE FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2",
            )
            .bind(company.as_uuid())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        let result = revised(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn register_http_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        operation: HttpOperation,
    ) -> AppResult<HttpConnection> {
        operation.validate(company)?;
        let snapshot = serde_json::to_value(&operation.tool).map_err(|_| invalid())?;
        let mut tx = begin_http(self).await?;
        edit_on(&mut tx, company, actor, id, revision).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 AND name<>$3").bind(company.as_uuid()).bind(id).bind(operation.tool.contract.name.as_str()).fetch_one(&mut *tx).await?;
        if count >= 256 {
            return Err(invalid());
        }
        sqlx::query("INSERT INTO workflow_http_operations(company_id,connection_id,name,snapshot,approval_required) VALUES($1,$2,$3,$4,$5) ON CONFLICT(company_id,connection_id,name) DO UPDATE SET snapshot=EXCLUDED.snapshot,approval_required=EXCLUDED.approval_required")
            .bind(company.as_uuid()).bind(id).bind(operation.tool.contract.name.as_str()).bind(snapshot).bind(operation.approval_required).execute(&mut *tx).await?;
        let result = revised(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(result)
    }
    async fn revoke_http_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        name: TypeName,
    ) -> AppResult<HttpConnection> {
        let mut tx = begin_http(self).await?;
        edit_on(&mut tx, company, actor, id, revision).await?;
        sqlx::query("DELETE FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3").bind(company.as_uuid()).bind(id).bind(name.as_str()).execute(&mut *tx).await?;
        let result = revised(&mut tx, company, id).await?;
        tx.commit().await?;
        Ok(result)
    }
}

pub(super) async fn status_on(
    tx: &mut Transaction<'_, Postgres>,
    connection: &HttpConnection,
) -> AppResult<ResourceStatus> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 ORDER BY name LIMIT 257").bind(connection.company.as_uuid()).bind(connection.id).fetch_all(&mut **tx).await?;
    if names.len() > 256 {
        return Err(invalid());
    }
    let supported_contracts = names
        .into_iter()
        .map(|name| TypeName::parse(name).map_err(|_| invalid()))
        .collect::<AppResult<BTreeSet<_>>>()?;
    Ok(ResourceStatus {
        id: RuntimeResourceId::new(connection.id),
        company_id: connection.company,
        kind: TypeName::parse("http").map_err(|_| invalid())?,
        authorized: true,
        readiness: if !connection.enabled {
            ResourceReadiness::Revoked
        } else if connection.bearer && !connection.secret_set {
            ResourceReadiness::Unavailable
        } else {
            ResourceReadiness::Ready
        },
        supported_contracts,
    })
}

pub(super) async fn policy_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    id: Uuid,
    wanted: &publication::ToolSnapshot,
) -> AppResult<CurrentActionPolicy> {
    let (snapshot, approval_required): (Value, bool) = sqlx::query_as("SELECT snapshot,approval_required FROM workflow_http_operations WHERE company_id=$1 AND connection_id=$2 AND name=$3 FOR SHARE")
        .bind(company.as_uuid()).bind(id).bind(wanted.contract.name.as_str()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let mut tool: publication::ToolSnapshot =
        serde_json::from_value(snapshot).map_err(|_| invalid())?;
    HttpOperation {
        tool: tool.clone(),
        approval_required,
    }
    .validate(company)?;
    tool.contract.connection = wanted.contract.connection.clone();
    if serde_json::to_value(&tool).map_err(|_| invalid())?
        != serde_json::to_value(wanted).map_err(|_| invalid())?
    {
        return Err(missing());
    }
    Ok(CurrentActionPolicy {
        tool,
        approval_required,
    })
}

pub struct HttpActionAuthority;
#[async_trait]
impl action_dispatch::SqlActionAuthority for HttpActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<action_dispatch::LockedActionAuthority> {
        let request = authority.action.request();
        let id = http_target(&request.target)?;
        let connection = connection_on(tx, request.scope.company, id)
            .await?
            .ok_or_else(missing)?;
        Ok(action_dispatch::LockedActionAuthority {
            resource: status_on(tx, &connection).await?,
            policy: policy_on(tx, request.scope.company, id, &request.contract).await?,
        })
    }
}
pub(super) fn http_target(target: &ActionTarget) -> AppResult<Uuid> {
    match target {
        ActionTarget::Connection {
            resource,
            resource_kind,
        }
        | ActionTarget::Local {
            resource,
            resource_kind,
        } if resource_kind.as_str() == "http" => Ok(*resource),
        _ => Err(missing()),
    }
}

impl PostgresPersistence {
    pub(crate) async fn http_for_action(
        &self,
        action: &FrozenAction,
    ) -> AppResult<(HttpConnection, Option<SecretString>)> {
        let request = action.request();
        let company = request.scope.company;
        let id = http_target(&request.target)?;
        let mut tx = begin_http(self).await?;
        authority::authorize_company(
            &mut tx,
            company,
            WorkflowActor::authenticated(request.context.actor)?,
        )
        .await?;
        let live: Option<Uuid> = sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 AND actor_id=$3 AND state IN ('queued','running') AND deadline>clock_timestamp() FOR SHARE")
            .bind(company.as_uuid()).bind(request.scope.run.as_uuid()).bind(request.context.actor).fetch_optional(&mut *tx).await?;
        live.ok_or_else(missing)?;
        let connection = connection_on(&mut tx, company, id)
            .await?
            .ok_or_else(missing)?;
        validate_resource(request, &status_on(&mut tx, &connection).await?)?;
        if validate_current_authority(
            request,
            &policy_on(&mut tx, company, id, &request.contract).await?,
        )? != ActionAccessDecision::CurrentAccessConfirmed
        {
            return Err(missing());
        }
        let envelope: Option<String> = sqlx::query_scalar("SELECT envelope FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2").bind(company.as_uuid()).bind(id).fetch_optional(&mut *tx).await?;
        let secret = envelope
            .map(|envelope| {
                self.credential_cipher()?.open_envelope(
                    &CredentialContext::workflow_http_credential(company.as_uuid(), id),
                    &envelope,
                )
            })
            .transpose()?;
        if connection.bearer != secret.is_some() {
            return Err(missing());
        }
        tx.commit().await?;
        Ok((connection, secret))
    }
}
