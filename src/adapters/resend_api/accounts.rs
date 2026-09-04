//! Acting as one company at Resend.
//!
//! Every credential this module hands out is read at the moment it is used rather than held from
//! startup. That is what makes connecting, rotating or disconnecting an account take effect on the
//! next mail instead of on the next deploy -- and it is why nothing here caches a key: the row is
//! the source of truth, and a cache of credentials is a cache that can outlive the authority to
//! use them.
//!
//! What *is* shared is the HTTP client, which holds the connection pool and no authority at all.

use std::sync::Arc;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    adapters::{
        protocols::email::{CompanyMailTransports, MailTransport},
        resend_api::{
            client::{ReqwestResendApiClient, ResendApi},
            transport::ResendApiMailTransport,
        },
    },
    app_error::{AppError, AppResult},
    entities::value_objects::AuthservId,
    infra::config::ResendApiConfig,
    use_cases::company_resend_api::CompanyResendApiAccounts,
};

/// One company's Resend account, ready to use: the client its calls are authorized by, and the
/// `authserv-id` whose verdicts may be believed on the mail those calls return.
///
/// The two travel together because they are one account. Reading a mail through company A's key
/// and then trusting an `Authentication-Results` header written for company B's receiving domain
/// is exactly the mix-up this struct exists to make unrepresentable.
pub struct CompanyResendApiAccount {
    pub api: Arc<dyn ResendApi>,
    pub authserv_id: AuthservId,
}

/// Builds a client for whichever company is being acted for.
pub struct CompanyResendApiClients {
    accounts: Arc<dyn CompanyResendApiAccounts>,
    config: ResendApiConfig,
    /// Built once and cloned per company: a clone shares the connection pool, so a deployment with
    /// a hundred tenants still keeps one set of connections to the API rather than a hundred.
    http: reqwest::Client,
}

impl CompanyResendApiClients {
    pub fn new(
        accounts: Arc<dyn CompanyResendApiAccounts>,
        config: ResendApiConfig,
    ) -> AppResult<Self> {
        let http = ReqwestResendApiClient::shared_http(&config).map_err(|error| {
            AppError::Internal(format!("Could not build a Resend client: {error}"))
        })?;
        Ok(Self {
            accounts,
            config,
            http,
        })
    }

    /// The account to act as for this company, or `None` when it has not connected Resend or has
    /// switched the integration off.
    ///
    /// A read failure propagates. "Could not load the credential" is not "this company has no
    /// Resend", and collapsing the two would send a tenant's mail out through the deployment's own
    /// relay the moment a query failed.
    pub async fn account_for(
        &self,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyResendApiAccount>> {
        let Some(credentials) = self.accounts.account_credentials(company_id).await? else {
            return Ok(None);
        };
        Ok(Some(CompanyResendApiAccount {
            api: Arc::new(ReqwestResendApiClient::new(
                &self.http,
                &self.config,
                credentials.api_key,
            )),
            authserv_id: credentials.authserv_id,
        }))
    }
}

/// Which transport a company's mail goes out through: its own Resend account when it has one, and
/// the deployment's relay otherwise.
///
/// The fallback is deliberate and is not a credential fallback: a company without a Resend account
/// sends over the deployment's SMTP relay from the deployment's own domain, which is what it did
/// before it connected anything. No company ever sends through another company's key, because the
/// only key reachable here is the one the company id resolves to.
pub struct ResendApiCompanyTransports {
    clients: Arc<CompanyResendApiClients>,
    deployment: Arc<dyn MailTransport>,
}

impl ResendApiCompanyTransports {
    pub fn new(clients: Arc<CompanyResendApiClients>, deployment: Arc<dyn MailTransport>) -> Self {
        Self {
            clients,
            deployment,
        }
    }
}

#[async_trait]
impl CompanyMailTransports for ResendApiCompanyTransports {
    async fn transport_for(&self, company_id: Option<Uuid>) -> AppResult<Arc<dyn MailTransport>> {
        // A platform notice belongs to the deployment rather than to a tenant, so there is no
        // account to look up: it goes out the way it always has.
        let Some(company_id) = company_id else {
            return Ok(self.deployment.clone());
        };
        match self.clients.account_for(company_id).await? {
            Some(account) => Ok(Arc::new(ResendApiMailTransport::new(account.api))),
            None => Ok(self.deployment.clone()),
        }
    }
}
