use super::{
    CapabilitySnapshot, ClaimVerifier, CredentialVerifier, PairingClaimSecret, PairingCode,
    PairingCodeHash,
};
use crate::{
    app_error::{AppError, AppResult},
    domain::local_agents::*,
    use_cases::{
        company::{CompanyPersistence, managed_company},
        user::{AuthAttemptKey, AuthAttemptSecret, auth_attempt_key},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, sync::Arc};
use uuid::Uuid;

use super::ports::LocalAgentRead;

#[derive(Debug, Clone, Serialize)]
pub struct PairingAttempt {
    pub id: PairingId,
    pub company_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub machine_id: Option<MachineId>,
}

pub struct CreatedPairing {
    pub code: PairingCode,
    pub attempt: PairingAttempt,
}

pub struct PairingCreate {
    pub id: PairingId,
    pub company_id: Uuid,
    pub actor_id: Uuid,
    pub code_hash: PairingCodeHash,
    pub replace_machine: Option<MachineId>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingClaim {
    pub code: PairingCode,
    pub installation_id: InstallationId,
    pub name: String,
    pub claim_verifier: ClaimVerifier,
    pub credential_verifier: CredentialVerifier,
}

impl PairingClaim {
    pub fn validate(&self) -> AppResult<()> {
        validate_name(&self.name)
    }
}

pub fn validate_name(name: &str) -> AppResult<()> {
    if name.is_empty()
        || name.len() > 128
        || name.trim() != name
        || name.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(
            "name must be 1–128 UTF-8 bytes without surrounding whitespace or controls".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingReceipt {
    pub company_id: Uuid,
    pub company_name: String,
    pub machine: LocalMachine,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineDetails {
    pub machine: LocalMachine,
    pub capabilities: Option<CapabilitySnapshot>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineChange {
    Enable,
    Disable,
    Revoke,
}

#[derive(Debug, Clone, Copy)]
pub struct MachineAuthentication {
    pub company_id: Uuid,
    pub machine_id: MachineId,
    pub installation_id: InstallationId,
    pub credential_generation: CredentialGeneration,
}

#[derive(Debug, Clone, Copy)]
pub struct ConnectionFence {
    pub machine: MachineAuthentication,
    pub generation: ConnectionGeneration,
    pub owner_id: Uuid,
}

#[async_trait]
pub trait LocalMachineStore: LocalAgentRead {
    async fn create_pairing(&self, write: PairingCreate) -> AppResult<PairingAttempt>;
    async fn list_pairings(
        &self,
        company: Uuid,
        after: Option<PairingId>,
    ) -> AppResult<Vec<PairingAttempt>>;
    async fn cancel_pairing(&self, actor: Uuid, company: Uuid, id: PairingId) -> AppResult<()>;
    async fn reserve_pairing_budget(&self, keys: &[AuthAttemptKey]) -> AppResult<bool>;
    async fn claim_pairing(&self, claim: &PairingClaim) -> AppResult<Option<PairingReceipt>>;
    async fn pairing_status(
        &self,
        installation: InstallationId,
        claim: ClaimVerifier,
    ) -> AppResult<Option<PairingReceipt>>;
    async fn list_machines(
        &self,
        company: Uuid,
        after: Option<MachineId>,
    ) -> AppResult<Vec<LocalMachine>>;
    async fn change_machine(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        change: MachineChange,
    ) -> AppResult<LocalMachine>;
    async fn set_projects(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        projects: &[CloudroomProjectId],
    ) -> AppResult<()>;
    async fn save_profile(&self, actor: Uuid, profile: &LocalAgentProfile) -> AppResult<()>;
    async fn acquire_connection(
        &self,
        verifier: CredentialVerifier,
        installation: InstallationId,
        owner: Uuid,
    ) -> AppResult<Option<ConnectionFence>>;
    async fn renew_connection(&self, fence: ConnectionFence) -> AppResult<bool>;
    async fn release_connection(&self, fence: ConnectionFence) -> AppResult<()>;
}

#[derive(Clone)]
pub struct LocalMachineUseCases {
    companies: Arc<dyn CompanyPersistence>,
    store: Arc<dyn LocalMachineStore>,
    attempt_secret: AuthAttemptSecret,
}

impl LocalMachineUseCases {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        store: Arc<dyn LocalMachineStore>,
        attempt_secret: AuthAttemptSecret,
    ) -> Self {
        Self {
            companies,
            store,
            attempt_secret,
        }
    }

    pub async fn create_pairing(
        &self,
        actor: Uuid,
        company: Uuid,
        replace_machine: Option<MachineId>,
    ) -> AppResult<CreatedPairing> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        let code = PairingCode::generate()?;
        let attempt = self
            .store
            .create_pairing(PairingCreate {
                id: PairingId::new(Uuid::new_v4()).map_err(|e| AppError::Internal(e.into()))?,
                company_id: company,
                actor_id: actor,
                code_hash: code.verifier(),
                replace_machine,
            })
            .await?;
        Ok(CreatedPairing { code, attempt })
    }

    pub async fn list_pairings(
        &self,
        actor: Uuid,
        company: Uuid,
        after: Option<PairingId>,
    ) -> AppResult<Vec<PairingAttempt>> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.list_pairings(company, after).await
    }

    pub async fn cancel_pairing(&self, actor: Uuid, company: Uuid, id: PairingId) -> AppResult<()> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.cancel_pairing(actor, company, id).await
    }

    fn budget(&self, domain: &str, value: &[u8]) -> AuthAttemptKey {
        auth_attempt_key(self.attempt_secret.expose_secret(), domain, &[value])
    }

    pub async fn pair(&self, source: IpAddr, claim: PairingClaim) -> AppResult<PairingReceipt> {
        claim.validate()?;
        let code_hash = claim.code.verifier();
        let keys = [
            self.budget("local-pair-source:v1", source.to_string().as_bytes()),
            self.budget("local-pair-code:v1", code_hash.as_bytes()),
            self.budget("local-pair-claim:v1", claim.claim_verifier.as_bytes()),
        ];
        if !self.store.reserve_pairing_budget(&keys).await? {
            return Err(AppError::InvalidCredentials);
        }
        self.store
            .claim_pairing(&claim)
            .await?
            .ok_or(AppError::InvalidCredentials)
    }

    pub async fn pair_status(
        &self,
        source: IpAddr,
        installation: InstallationId,
        secret: PairingClaimSecret,
    ) -> AppResult<PairingReceipt> {
        let claim = secret.verifier();
        let keys = [
            self.budget("local-pair-source:v1", source.to_string().as_bytes()),
            self.budget("local-pair-claim:v1", claim.as_bytes()),
        ];
        if !self.store.reserve_pairing_budget(&keys).await? {
            return Err(AppError::InvalidCredentials);
        }
        self.store
            .pairing_status(installation, claim)
            .await?
            .ok_or(AppError::InvalidCredentials)
    }

    pub async fn machines(
        &self,
        actor: Uuid,
        company: Uuid,
        after: Option<MachineId>,
    ) -> AppResult<Vec<LocalMachine>> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.list_machines(company, after).await
    }

    pub async fn machine(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
    ) -> AppResult<LocalMachine> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store
            .machine(company, id)
            .await?
            .ok_or_else(|| AppError::NotFound("local machine".into()))
    }

    pub async fn change(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        change: MachineChange,
    ) -> AppResult<LocalMachine> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        self.store.change_machine(actor, company, id, change).await
    }

    pub async fn set_projects(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        projects: &[CloudroomProjectId],
    ) -> AppResult<()> {
        managed_company(self.companies.as_ref(), actor, company).await?;
        if projects.len() > super::bounds::MAX_PROJECTS {
            return Err(AppError::BadRequest("too many projects".into()));
        }
        self.store.set_projects(actor, company, id, projects).await
    }

    pub async fn save_profile(&self, actor: Uuid, profile: &LocalAgentProfile) -> AppResult<()> {
        managed_company(self.companies.as_ref(), actor, profile.company_id).await?;
        validate_name(&profile.name)?;
        self.store.save_profile(actor, profile).await
    }

    pub async fn details(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
    ) -> AppResult<MachineDetails> {
        let machine = self.machine(actor, company, id).await?;
        let capabilities = match machine.capabilities_sequence {
            Some(sequence) => self.store.capabilities(company, id, sequence).await,
            None => Ok(None),
        }?;
        Ok(MachineDetails {
            machine,
            capabilities,
        })
    }
}
