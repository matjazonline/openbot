use super::*;

pub(super) enum ModuleAuthority<'a> {
    Business(HostAuthorityFacts),
    Configuration(&'a AccountOwner, ConfigurationAuthorityFacts),
}
impl PostgresVault {
    /// Only the persisted host configuration owner calls this while holding its native locks.
    /// This validates a metadata commit and can neither resolve a token nor dispatch a call.
    pub(crate) async fn validate_configuration_under_host(
        &self,
        c: &OwnerDiscoveryContext,
        facts: ConfigurationAuthorityFacts,
    ) -> IntegrationResult<()> {
        let gate = ConfigurationGate {
            vault: self,
            expected: c,
            generation: GenerationCheck::Current,
            passed: AtomicBool::new(false),
        };
        gate.check(c, facts).await
    }

    pub(super) async fn module_gate(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        expected: &AuthorizationSnapshot,
        generation: GenerationCheck,
        authority: ModuleAuthority<'_>,
    ) -> IntegrationResult<()> {
        let grant = grant_on(tx, expected.grant).await?;
        let account = read_account_on(tx, &grant.owner, grant.account).await?;
        if let GenerationCheck::Exact(generation) = generation {
            let current: bool = sqlx::query_scalar(
                "SELECT credential_generation=$2 AND renewal_attempt_id IS NULL FROM integration_accounts WHERE id=$1",
            ).bind(account.id.as_uuid()).bind(generation.get()).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if !current {
                return Err(IntegrationError::Conflict);
            }
        }
        let grant = grant_on(tx, grant.id).await?;
        pending(tx, &account.owner, &grant.host_scope).await?;
        match authority {
            ModuleAuthority::Business(host) => {
                authorize_dispatch(&account, &grant, expected, host)?
            }
            ModuleAuthority::Configuration(owner, facts) => {
                authorize_configuration(&account, &grant, expected, owner, facts)?
            }
        }
        if self.registry.policy(&grant.app)? != grant.app_policy {
            return Err(IntegrationError::Denied);
        }
        self.validate_binding_payload(tx, &account, &grant.app, &grant.destination, grant.purpose)
            .await
    }
    pub(crate) async fn configuration_guard(
        &self,
        c: &OwnerDiscoveryContext,
    ) -> IntegrationResult<()> {
        Box::pin(self.check_configuration_on_host(c, GenerationCheck::Current)).await
    }
    pub(crate) async fn configuration_generation_guard(
        &self,
        c: &OwnerDiscoveryContext,
        generation: CredentialGeneration,
    ) -> IntegrationResult<()> {
        Box::pin(self.check_configuration_on_host(c, GenerationCheck::Exact(generation))).await
    }
    async fn check_configuration_on_host(
        &self,
        c: &OwnerDiscoveryContext,
        generation: GenerationCheck,
    ) -> IntegrationResult<()> {
        let remaining = (c.deadline - chrono::Utc::now())
            .to_std()
            .map_err(|_| IntegrationError::Denied)?;
        let gate = ConfigurationGate {
            vault: self,
            expected: c,
            generation,
            passed: AtomicBool::new(false),
        };
        tokio::select! { biased;
            _ = c.cancellation.cancelled() => return Err(IntegrationError::Denied),
            _ = tokio::time::sleep(remaining) => return Err(IntegrationError::Denied),
            checked = c.authority.check(c, &gate) => checked?,
        }
        if !gate.passed.load(Ordering::Acquire) {
            return Err(IntegrationError::Denied);
        }
        Ok(())
    }
}
struct ConfigurationGate<'a> {
    vault: &'a PostgresVault,
    expected: &'a OwnerDiscoveryContext,
    generation: GenerationCheck,
    passed: AtomicBool,
}
#[async_trait]
impl ConfigurationCredentialGate for ConfigurationGate<'_> {
    async fn check(
        &self,
        c: &OwnerDiscoveryContext,
        facts: ConfigurationAuthorityFacts,
    ) -> IntegrationResult<()> {
        if c.authorization != self.expected.authorization
            || c.owner.owner() != self.expected.owner.owner()
            || c.deadline != self.expected.deadline
            || c.deadline <= chrono::Utc::now()
            || c.cancellation.is_cancelled()
        {
            return Err(IntegrationError::Denied);
        }
        let mut tx = self
            .vault
            .persistence
            .pool()
            .begin()
            .await
            .map_err(storage_error)?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        self.vault
            .module_gate(
                &mut tx,
                &c.authorization,
                self.generation,
                ModuleAuthority::Configuration(c.owner.owner(), facts),
            )
            .await?;
        tx.commit().await.map_err(storage_error)?;
        self.passed.store(true, Ordering::Release);
        Ok(())
    }
}
