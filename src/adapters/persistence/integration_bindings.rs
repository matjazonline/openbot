//! Openbot's host consent adapter. User/company/connection locks precede module
//! locks. No credential column or token is read here.
use super::{PostgresPersistence, integrations::PostgresVault};
use crate::{
    adapters::integrations::owner::{client_namespace, company_binding},
    app_error::{AppError, AppResult},
    application::{
        integrations::{grants::*, *},
        use_cases::integration_binding::*,
        workflow::{CompanyId, WorkflowActor},
    },
    domain::integrations::{authority::*, *},
};
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct PostgresIntegrationBindings {
    pub persistence: PostgresPersistence,
    pub vault: PostgresVault,
}
impl PostgresIntegrationBindings {
    #[cfg(test)]
    pub fn new(persistence: PostgresPersistence) -> Self {
        Self {
            vault: PostgresVault::new(persistence.clone()),
            persistence,
        }
    }
}
pub(crate) fn binding(company: Uuid, connection: Uuid, kind: BindingKind) -> HostBinding {
    HostBinding::parse(format!("{}/{company}/{connection}", kind.as_str()))
        .expect("code-owned UUID binding")
}
fn denied() -> AppError {
    AppError::Forbidden("Connection consent is unavailable or changed".into())
}
fn map_error(error: AppError) -> IntegrationError {
    match error {
        AppError::NotFound(_) | AppError::Forbidden(_) | AppError::BadRequest(_) => {
            IntegrationError::Denied
        }
        AppError::Conflict(_) => IntegrationError::Conflict,
        _ => IntegrationError::Storage,
    }
}
fn owner_id(owner: &VerifiedOwner) -> AppResult<Uuid> {
    if owner.owner().namespace != client_namespace() {
        return Err(denied());
    }
    Uuid::parse_str(owner.owner().subject.as_str()).map_err(|_| denied())
}
#[derive(sqlx::FromRow)]
struct ProposalRow {
    id: Uuid,
    company_id: Uuid,
    company_name: String,
    connection_id: Uuid,
    kind: String,
    administrator_id: Uuid,
    connection_revision: i64,
    app_id: String,
    app_policy: String,
    destination: String,
}
impl ProposalRow {
    fn metadata(&self) -> AppResult<BindingProposal> {
        Ok(BindingProposal {
            id: self.id,
            company: self.company_id,
            company_name: self.company_name.clone(),
            connection: self.connection_id,
            kind: match self.kind.as_str() {
                "http" => BindingKind::Http,
                "mcp" => BindingKind::Mcp,
                _ => return Err(denied()),
            },
            destination: ApprovedDestination::parse(self.destination.clone())?,
            app: AppId::parse(self.app_id.clone())?,
            app_policy: AppPolicyDigest::parse(self.app_policy.clone())?,
            connection_revision: self.connection_revision,
        })
    }
}
async fn proposal_on(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> AppResult<ProposalRow> {
    sqlx::query_as::<_,ProposalRow>("SELECT proposal.id,proposal.company_id,company.name AS company_name,proposal.connection_id,proposal.kind,proposal.administrator_id,proposal.connection_revision,proposal.app_id,proposal.app_policy,proposal.destination FROM integration_binding_proposals AS proposal JOIN companies AS company ON company.id=proposal.company_id WHERE proposal.id=$1 AND NOT proposal.consumed AND proposal.expires_at>clock_timestamp()")
        .bind(id).fetch_optional(&mut **tx).await?.ok_or_else(denied)
}
async fn member_on(tx: &mut Transaction<'_, Postgres>, company: Uuid, user: Uuid) -> AppResult<()> {
    let person:Option<Uuid>=sqlx::query_scalar("SELECT member.user_id FROM company_members AS member JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id WHERE member.company_id=$1 AND member.user_id=$2 AND principal.kind='person' FOR SHARE OF member,principal")
        .bind(company).bind(user).fetch_optional(&mut **tx).await?;
    person.map(|_| ()).ok_or_else(denied)
}
async fn users_on(tx: &mut Transaction<'_, Postgres>, mut ids: Vec<Uuid>) -> AppResult<()> {
    ids.sort_unstable();
    ids.dedup();
    let found: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
            .bind(&ids)
            .fetch_all(&mut **tx)
            .await?;
    if found != ids {
        return Err(denied());
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
struct DestinationRow {
    destination: String,
    revision: i64,
    enabled: bool,
}
async fn destination_on(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    id: Uuid,
    kind: BindingKind,
) -> AppResult<DestinationRow> {
    let sql = match kind {
        BindingKind::Http => {
            "SELECT origin AS destination,revision,enabled FROM workflow_http_connections WHERE company_id=$1 AND id=$2 FOR UPDATE"
        }
        BindingKind::Mcp => {
            "SELECT endpoint_url AS destination,revision,enabled FROM company_mcp_connections WHERE company_id=$1 AND id=$2 AND deleted_at IS NULL FOR UPDATE"
        }
    };
    sqlx::query_as(sql)
        .bind(company)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(denied)
}
#[async_trait]
impl IntegrationBindings for PostgresIntegrationBindings {
    async fn propose(&self, actor: Uuid, c: ProposeBinding) -> AppResult<BindingProposal> {
        let app_policy = self.vault.registry.policy(&c.app)?;
        let mut tx = self.persistence.pool().begin().await?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        users_on(&mut tx, vec![actor]).await?;
        super::workflow::authority::authorize_company(
            &mut tx,
            CompanyId::new(c.company),
            WorkflowActor::authenticated(actor)?,
        )
        .await?;
        let current = destination_on(&mut tx, c.company, c.connection, c.kind).await?;
        if current.revision != c.expected_revision || !current.enabled {
            return Err(denied());
        }
        let destination = ApprovedDestination::parse(current.destination)?;
        sqlx::query("DELETE FROM integration_binding_proposals WHERE company_id=$1 AND connection_id=$2 AND kind=$3 AND (expires_at<=clock_timestamp() OR consumed)")
            .bind(c.company).bind(c.connection).bind(c.kind.as_str()).execute(&mut *tx).await?;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM integration_binding_proposals WHERE company_id=$1 AND connection_id=$2 AND kind=$3")
            .bind(c.company).bind(c.connection).bind(c.kind.as_str()).fetch_one(&mut *tx).await?;
        if count >= 8 {
            return Err(IntegrationError::Limit.into());
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO integration_binding_proposals(id,company_id,connection_id,kind,administrator_id,connection_revision,app_id,destination,app_policy,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,clock_timestamp()+interval '10 minutes')")
            .bind(id).bind(c.company).bind(c.connection).bind(c.kind.as_str()).bind(actor).bind(c.expected_revision).bind(c.app.as_str()).bind(destination.as_str()).bind(app_policy.as_str()).execute(&mut *tx).await?;
        let result = proposal_on(&mut tx, id).await?.metadata()?;
        tx.commit().await?;
        Ok(result)
    }
    async fn proposal(&self, owner: &VerifiedOwner, id: Uuid) -> AppResult<BindingProposal> {
        let user = owner_id(owner)?;
        let mut tx = self.persistence.pool().begin().await?;
        let proposal = proposal_on(&mut tx, id).await?;
        member_on(&mut tx, proposal.company_id, user).await?;
        let result = proposal.metadata()?;
        tx.commit().await?;
        Ok(result)
    }
    async fn confirm(
        &self,
        owner: VerifiedOwner,
        id: Uuid,
        account: AccountId,
        authorization: AuthorizationRevision,
    ) -> AppResult<UseGrant> {
        let proposal = self.proposal(&owner, id).await?;
        let command = OwnerConfirmation {
            owner,
            account,
            binding: binding(proposal.company, proposal.connection, proposal.kind),
            host_scope: company_binding(proposal.company),
            purpose: proposal.kind.purpose(),
            expected_authorization: authorization,
            app: proposal.app,
            destination: proposal.destination,
            host_authority: HostAuthorityReference::parse(id.to_string())?,
        };
        let service = GrantService {
            writer: Arc::new(self.vault.clone()),
            authority: Arc::new(self.clone()),
        };
        Ok(service.create_use_grant(command).await?)
    }
}
#[async_trait]
impl BindingAuthority for PostgresIntegrationBindings {
    async fn confirm(
        &self,
        c: OwnerConfirmation,
        writer: &dyn GrantWriter,
    ) -> IntegrationResult<UseGrant> {
        let mut tx = self
            .persistence
            .pool()
            .begin()
            .await
            .map_err(|_| IntegrationError::Storage)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(|_| IntegrationError::Storage)?;
        let proposal = self.verify_consent_on(&mut tx, &c).await?;
        // Lock/consume the exact proposal after all its authoritative host owners.
        let claimed=sqlx::query("UPDATE integration_binding_proposals SET consumed=true WHERE id=$1 AND NOT consumed AND expires_at>clock_timestamp()")
            .bind(proposal.id).execute(&mut *tx).await.map_err(|_|IntegrationError::Storage)?;
        if claimed.rows_affected() != 1 {
            return Err(IntegrationError::Conflict);
        }
        let grant = writer
            .create_confirmed(
                &c,
                GrantAuthorityFacts {
                    owner_is_member: true,
                    administrator_can_edit: true,
                    owner_confirmed: true,
                },
            )
            .await?;
        let (clear, install) = match proposal.kind {
            BindingKind::Http => (
                "DELETE FROM workflow_http_credentials WHERE company_id=$1 AND connection_id=$2",
                "UPDATE workflow_http_connections SET bearer=false,integration_account_id=$3,integration_grant_id=$4,integration_authorization=$5,integration_grant_revision=$6,revision=revision+1 WHERE company_id=$1 AND id=$2",
            ),
            BindingKind::Mcp => (
                "DELETE FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
                "UPDATE company_mcp_connections SET auth_type='integration',integration_account_id=$3,integration_grant_id=$4,integration_authorization=$5,integration_grant_revision=$6,revision=revision+1,credential_revision=credential_revision+1 WHERE company_id=$1 AND id=$2",
            ),
        };
        sqlx::query(clear)
            .bind(proposal.company)
            .bind(proposal.connection)
            .execute(&mut *tx)
            .await
            .map_err(|_| IntegrationError::Storage)?;
        sqlx::query(install)
            .bind(proposal.company)
            .bind(proposal.connection)
            .bind(grant.account.as_uuid())
            .bind(grant.id.as_uuid())
            .bind(grant.account_authorization.get())
            .bind(grant.revision.get())
            .execute(&mut *tx)
            .await
            .map_err(|_| IntegrationError::Storage)?;
        tx.commit().await.map_err(|_| IntegrationError::Storage)?;
        Ok(grant)
    }
}

impl PostgresIntegrationBindings {
    async fn verify_consent_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        c: &OwnerConfirmation,
    ) -> IntegrationResult<BindingProposal> {
        let user = owner_id(&c.owner).map_err(map_error)?;
        let id =
            Uuid::parse_str(c.host_authority.as_str()).map_err(|_| IntegrationError::Denied)?;
        let row = proposal_on(tx, id).await.map_err(map_error)?;
        let proposal = row.metadata().map_err(map_error)?;
        users_on(tx, vec![user, row.administrator_id])
            .await
            .map_err(map_error)?;
        super::workflow::authority::authorize_company(
            tx,
            CompanyId::new(proposal.company),
            WorkflowActor::authenticated(row.administrator_id).map_err(map_error)?,
        )
        .await
        .map_err(map_error)?;
        member_on(tx, proposal.company, user)
            .await
            .map_err(map_error)?;
        let current = destination_on(tx, proposal.company, proposal.connection, proposal.kind)
            .await
            .map_err(map_error)?;
        let destination = ApprovedDestination::parse(current.destination)?;
        if self.vault.registry.policy(&proposal.app)? != proposal.app_policy {
            return Err(IntegrationError::Denied);
        }
        if !current.enabled
            || current.revision != proposal.connection_revision
            || destination != proposal.destination
            || c.app != proposal.app
            || c.destination != destination
            || c.host_scope != company_binding(proposal.company)
            || c.binding != binding(proposal.company, proposal.connection, proposal.kind)
            || c.purpose != proposal.kind.purpose()
        {
            return Err(IntegrationError::Denied);
        }
        Ok(proposal)
    }
}
