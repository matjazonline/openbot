use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company::{
            Company, CompanyAccess, CompanyChannelDefaults, CompanyModelConnection,
            CompanyTeamAccount,
        },
        company_member::CompanyMembership,
        memory::MemoryProviderKind,
        value_objects::{AvatarUrl, ModelName, ModelProvider},
    },
};

pub const MAX_COMPANY_MODEL_CONNECTIONS: usize = 8;
pub const MAX_MODELS_PER_CONNECTION: usize = 32;
const MAX_MODEL_IDENTIFIER_BYTES: usize = 200;
const MAX_PROVIDER_IDENTIFIER_BYTES: usize = 64;
const MAX_MODEL_API_KEY_BYTES: usize = 8 * 1024;

/// One provider credential and the exact models a company permits its agents to select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyModelConnectionWrite {
    pub provider: ModelProvider,
    /// `None` preserves the stored key for an existing provider. A new provider requires a key.
    pub api_key: Option<String>,
    pub models: Vec<ModelName>,
    pub is_default: bool,
}

impl CompanyModelConnectionWrite {
    pub fn new(
        provider: impl Into<String>,
        api_key: Option<String>,
        models: Vec<String>,
        is_default: bool,
    ) -> AppResult<Self> {
        let provider = provider.into().trim().to_ascii_lowercase();
        if provider.is_empty() || provider.len() > MAX_PROVIDER_IDENTIFIER_BYTES {
            return Err(AppError::BadRequest(
                "A model provider is missing or too long.".into(),
            ));
        }
        if !matches!(
            provider.as_str(),
            "google" | "openai" | "anthropic" | "groq"
        ) {
            return Err(AppError::BadRequest(format!(
                "Unsupported model provider '{provider}'."
            )));
        }

        let mut normalized_models = Vec::with_capacity(models.len());
        for model in models {
            let model = model.trim();
            if model.is_empty() || model.len() > MAX_MODEL_IDENTIFIER_BYTES {
                return Err(AppError::BadRequest(
                    "A model name is missing or too long.".into(),
                ));
            }
            if !normalized_models
                .iter()
                .any(|existing: &ModelName| existing.as_str() == model)
            {
                normalized_models.push(ModelName::from(model));
            }
        }
        if normalized_models.is_empty() || normalized_models.len() > MAX_MODELS_PER_CONNECTION {
            return Err(AppError::BadRequest(format!(
                "Each provider must enable between 1 and {MAX_MODELS_PER_CONNECTION} models."
            )));
        }

        let api_key = api_key
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());
        if api_key
            .as_ref()
            .is_some_and(|key| key.len() > MAX_MODEL_API_KEY_BYTES)
        {
            return Err(AppError::BadRequest(
                "The model API key is too long.".into(),
            ));
        }

        Ok(Self {
            provider: provider.into(),
            api_key,
            models: normalized_models,
            is_default,
        })
    }
}

/// Everything one company write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a seven-parameter list.
///
/// Values reach persistence already normalized — see [`CompanyWrite::normalize`]. Mirrors
/// [`crate::use_cases::agent::AgentWrite`].
#[derive(Debug, Clone, Default)]
pub struct CompanyWrite {
    pub name: String,
    pub slug: String,
    pub enable_llm_spam_guardrail: Option<bool>,
    pub memory_provider: Option<MemoryProviderKind>,
    pub channel_defaults: CompanyChannelDefaults,
    /// The company's picture, already parsed as a URL a page may render.
    pub avatar_url: Option<AvatarUrl>,
}

impl CompanyWrite {
    /// Trim the fields that have canonical forms and drop the blanks. Runs once, in the use case,
    /// so create and update store the same shape.
    fn normalize(&mut self) -> AppResult<()> {
        self.name = self.name.trim().to_string();
        self.slug = self.slug.trim().to_lowercase().replace(' ', "-");

        if self.name.is_empty() || self.slug.is_empty() {
            return Err(AppError::Internal(
                "Company name and slug cannot be empty.".into(),
            ));
        }

        let submitted_participants = self
            .channel_defaults
            .participant_emails
            .take()
            .unwrap_or_default();
        if submitted_participants.len() > 64 {
            return Err(AppError::BadRequest(
                "A company may configure at most 64 default channel participants.".into(),
            ));
        }

        let mut seen = std::collections::HashSet::new();
        let mut participants = Vec::new();
        for participant in submitted_participants {
            let normalized = participant.trim().to_ascii_lowercase();
            if normalized.is_empty() || !seen.insert(normalized.clone()) {
                continue;
            }
            if normalized != crate::entities::channel::PUBLIC_PARTICIPANT
                && !email_shaped(&normalized)
            {
                return Err(AppError::BadRequest(format!(
                    "Invalid default channel participant '{normalized}'."
                )));
            }
            participants.push(normalized.into());
        }
        self.channel_defaults.participant_emails =
            (!participants.is_empty()).then_some(participants);

        Ok(())
    }
}

fn email_shaped(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty()
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '.' | '!'
                        | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '\''
                        | '*'
                        | '+'
                        | '-'
                        | '/'
                        | '='
                        | '?'
                        | '^'
                        | '_'
                        | '`'
                        | '{'
                        | '|'
                        | '}'
                        | '~'
                )
        })
    {
        return false;
    }
    let labels = domain.split('.').collect::<Vec<_>>();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

#[async_trait]
pub trait CompanyPersistence: Send + Sync {
    async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>>;
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>>;
    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>>;
    async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company>;
    async fn delete(&self, id: Uuid) -> AppResult<()>;
    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        write: CompanyWrite,
    ) -> AppResult<Company> {
        match self.get_by_id(id).await? {
            Some(company) if company.user_id == user_id => self.update(id, write).await,
            _ => Err(company_not_found()),
        }
    }
    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        match self.get_by_id(id).await? {
            Some(company) if company.user_id == user_id => self.delete(id).await,
            _ => Err(company_not_found()),
        }
    }
    /// Every company the user may *read*: the ones they own, plus the ones they were invited to.
    ///
    /// Separate from [`CompanyPersistence::list_by_user_id`], which stays ownership-only because
    /// it scopes the pages that administer a company. The default answers with owned companies
    /// alone -- the conservative half of the truth -- so a persistence that has not implemented
    /// this grants nobody anything they would not already have had.
    async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        Ok(self
            .list_by_user_id(user_id)
            .await?
            .into_iter()
            .map(|company| CompanyAccess {
                company,
                membership: CompanyMembership::Owner,
            })
            .collect())
    }
    /// What the user is to one company, or `None` if they are nothing to it.
    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        Ok(self
            .list_accessible_by_user_id(user_id)
            .await?
            .into_iter()
            .find(|access| access.company.id == company_id))
    }
    /// What the account behind an *address* is to this company.
    ///
    /// The by-email counterpart of [`CompanyPersistence::company_access`], for the inbound path,
    /// which knows a sender only by the address they wrote from. `Channel::participant_access`
    /// asks it about every sender, and needs the owner told apart from the rest of the team --
    /// a restricted channel takes its own owner's mail whether or not they are on its list.
    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership>;
    async fn list_company_team_emails(&self, company_id: Uuid) -> AppResult<Vec<String>>;

    /// The same team as one identity per account, owner first.
    ///
    /// [`CompanyPersistence::list_company_team_emails`] answers only "which addresses belong to
    /// this team", which is enough to decide delivery but not to *choose* a person: attributing a
    /// scheduled run to a colleague needs their account id and what they are to the company as
    /// well. Required rather than defaulted for that reason -- an empty list would read as "this
    /// company has no team", and the choice it feeds is an authorization decision.
    async fn list_company_team_accounts(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyTeamAccount>>;

    /// Required rather than defaulted, like the two below it. A default returning an empty list
    /// reads as "this company has configured no providers", which is a real state with real
    /// consequences -- every agent stops resolving -- and no implementation should be able to
    /// assert it by saying nothing.
    async fn list_model_connections(
        &self,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>>;

    /// Credential-only read used immediately before a provider call. Never defaulted: `Ok(None)`
    /// is a claim that the company holds no key for this provider, and a double that made that
    /// claim by omission would send an unauthenticated call at a provider.
    async fn model_api_key(
        &self,
        company_id: Uuid,
        provider: &ModelProvider,
    ) -> AppResult<Option<String>>;

    async fn replace_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()>;
}

/// Resolve a company the caller owns.
///
/// A company that does not exist and a company owned by somebody else are reported *identically*,
/// on purpose: telling the two apart would let anyone probe ids to enumerate other tenants'
/// companies. Nothing in the product shows a non-owner a company — `list_by_user_id` returns only
/// owned rows — so there is no "visible but forbidden" case that would deserve a 403 instead.
///
/// The denied attempt is logged, so the signal an operator needs survives even though the caller
/// is told nothing.
pub async fn owned_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<Company> {
    match persistence.get_by_id(company_id).await? {
        Some(company) if company.user_id == user_id => Ok(company),
        Some(_) => {
            warn!("User {user_id} is not the owner of company {company_id}");
            Err(company_not_found())
        }
        None => Err(company_not_found()),
    }
}

/// Resolve a company whose operational workspaces and automation the caller may manage.
///
/// Ownership still outranks the stored membership row. Otherwise the caller must be an accepted
/// company admin; an ordinary member and a stranger receive the same not-found response so this
/// guard does not become a tenant-id probe.
pub async fn managed_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<Company> {
    let company = persistence
        .get_by_id(company_id)
        .await?
        .ok_or_else(company_not_found)?;

    if company.user_id == user_id {
        return Ok(company);
    }

    let may_manage = persistence
        .company_access(user_id, company_id)
        .await?
        .is_some_and(|access| access.membership.manages_company_operations());
    if may_manage {
        return Ok(company);
    }

    warn!("User {user_id} may not manage operations for company {company_id}");
    Err(company_not_found())
}

pub fn company_not_found() -> AppError {
    AppError::NotFound("Company not found, or you do not have permission.".into())
}

#[derive(Clone)]
pub struct CompanyUseCases {
    persistence: Arc<dyn CompanyPersistence>,
}

impl CompanyUseCases {
    pub fn validate_model_connections(
        connections: &[CompanyModelConnectionWrite],
    ) -> AppResult<()> {
        if connections.len() > MAX_COMPANY_MODEL_CONNECTIONS {
            return Err(AppError::BadRequest(format!(
                "A company may configure at most {MAX_COMPANY_MODEL_CONNECTIONS} model providers."
            )));
        }
        // A replace is wholesale, so an empty set is a request to delete every stored credential
        // -- which reads to the caller as an ordinary save and leaves every agent unable to run.
        // Clearing the last provider is not something a save is allowed to mean.
        if connections.is_empty() {
            return Err(AppError::BadRequest(
                "A company must keep at least one model provider configured.".into(),
            ));
        }
        if connections.iter().filter(|item| item.is_default).count() != 1 {
            return Err(AppError::BadRequest(
                "Exactly one configured model provider must be the company default.".into(),
            ));
        }
        let mut providers = std::collections::HashSet::new();
        if connections
            .iter()
            .any(|item| !providers.insert(item.provider.as_str()))
        {
            return Err(AppError::BadRequest(
                "Each model provider may be configured only once.".into(),
            ));
        }
        Ok(())
    }

    pub fn new(persistence: Arc<dyn CompanyPersistence>) -> Self {
        Self { persistence }
    }

    /// The company, only when the caller owns it.
    ///
    /// The method exists because route handlers hold an `Arc<CompanyUseCases>` and cannot reach
    /// the persistence behind it. See [`owned_company`] for why a company owned by somebody else
    /// is reported exactly like one that does not exist.
    pub async fn owned_company(&self, user_id: Uuid, company_id: Uuid) -> AppResult<Company> {
        owned_company(self.persistence.as_ref(), user_id, company_id).await
    }

    /// The company, only when the caller owns it or is one of its admins.
    pub async fn managed_company(&self, user_id: Uuid, company_id: Uuid) -> AppResult<Company> {
        managed_company(self.persistence.as_ref(), user_id, company_id).await
    }

    #[instrument(skip(self))]
    pub async fn create_company(
        &self,
        user_id: Uuid,
        mut write: CompanyWrite,
    ) -> AppResult<Company> {
        write.normalize()?;

        info!(
            "Creating company: {} ({}) for user {}",
            write.name, write.slug, user_id
        );
        self.persistence.create(user_id, write).await
    }

    #[instrument(skip(self))]
    pub async fn list_user_companies(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        self.persistence.list_by_user_id(user_id).await
    }

    /// The companies a user may read, each with what they are to it.
    ///
    /// What the mailbox scopes by, where an invited member belongs. Administration pages apply a
    /// narrower owner-or-admin filter with [`CompanyUseCases::list_managed_companies`].
    #[instrument(skip(self))]
    pub async fn list_accessible_companies(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        self.persistence.list_accessible_by_user_id(user_id).await
    }

    /// Companies whose operational workspaces and automation the caller may manage.
    #[instrument(skip(self))]
    pub async fn list_managed_companies(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        Ok(self
            .persistence
            .list_accessible_by_user_id(user_id)
            .await?
            .into_iter()
            .filter(|access| access.membership.manages_company_operations())
            .map(|access| access.company)
            .collect())
    }

    /// What a user is to one company: its owner, an invited member, or nothing.
    #[instrument(skip(self))]
    pub async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        self.persistence.company_access(user_id, company_id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company(&self, id: Uuid) -> AppResult<Option<Company>> {
        self.persistence.get_by_id(id).await
    }

    #[instrument(skip(self))]
    pub async fn get_company_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        self.persistence.get_by_slug(slug.trim()).await
    }

    #[instrument(skip(self))]
    pub async fn update_company(&self, id: Uuid, mut write: CompanyWrite) -> AppResult<Company> {
        write.normalize()?;

        info!("Updating company {}: {} ({})", id, write.name, write.slug);
        self.persistence.update(id, write).await
    }

    #[instrument(skip(self))]
    pub async fn delete_company(&self, id: Uuid) -> AppResult<()> {
        info!("Deleting company {}", id);
        self.persistence.delete(id).await
    }

    pub async fn update_company_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        mut write: CompanyWrite,
    ) -> AppResult<Company> {
        write.normalize()?;
        self.persistence.update_for_user(user_id, id, write).await
    }

    pub async fn delete_company_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        self.persistence.delete_for_user(user_id, id).await
    }

    pub async fn list_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        owned_company(self.persistence.as_ref(), user_id, company_id).await?;
        self.persistence.list_model_connections(company_id).await
    }

    pub async fn replace_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        owned_company(self.persistence.as_ref(), user_id, company_id).await?;
        Self::validate_model_connections(&connections)?;
        self.persistence
            .replace_model_connections_for_user(user_id, company_id, connections)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;

    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
        memberships: Mutex<Vec<(Uuid, Uuid, CompanyMembership)>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company> {
            let company = Company {
                channel_defaults: Default::default(),
                id: Uuid::new_v4(),
                user_id,
                name: write.name,
                slug: write.slug.into(),
                enable_llm_spam_guardrail: write.enable_llm_spam_guardrail,
                avatar_url: write.avatar_url,
                memory_provider: None,
                created_at: Utc::now(),
            };
            self.companies.lock().unwrap().push(company.clone());
            Ok(company)
        }

        async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.id == id)
                .cloned())
        }

        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug.eq_ignore_ascii_case(slug))
                .cloned())
        }

        async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.user_id == user_id)
                .cloned()
                .collect())
        }

        async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
            let companies = self.companies.lock().unwrap();
            let memberships = self.memberships.lock().unwrap();
            Ok(companies
                .iter()
                .filter_map(|company| {
                    let membership = if company.user_id == user_id {
                        CompanyMembership::Owner
                    } else if let Some((_, _, membership)) =
                        memberships.iter().find(|(member_id, company_id, _)| {
                            *member_id == user_id && *company_id == company.id
                        })
                    {
                        *membership
                    } else {
                        return None;
                    };
                    Some(CompanyAccess {
                        company: company.clone(),
                        membership,
                    })
                })
                .collect())
        }

        async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company> {
            let mut list = self.companies.lock().unwrap();
            let company = list
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            company.name = write.name;
            company.slug = write.slug.into();
            company.enable_llm_spam_guardrail = write.enable_llm_spam_guardrail;
            company.avatar_url = write.avatar_url;
            Ok(company.clone())
        }

        async fn delete(&self, id: Uuid) -> AppResult<()> {
            self.companies.lock().unwrap().retain(|c| c.id != id);
            Ok(())
        }

        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
        }

        /// Model connections are not part of what these tests drive; a call here is a wiring mistake
        /// rather than a state worth simulating.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            unimplemented!("this double is not exercised on the model-connection path")
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection path")
        }
    }

    #[test]
    fn a_wholesale_replace_never_means_delete_every_credential() {
        // An empty set reaches the validator as an ordinary save -- a form whose provider rows
        // were all left unset -- and would delete every stored key, leaving every agent in the
        // company unable to resolve a provider. That is not something a save may mean.
        let empty = CompanyUseCases::validate_model_connections(&[])
            .expect_err("an empty replace is refused");
        assert!(
            empty
                .to_string()
                .contains("must keep at least one model provider"),
            "unexpected error: {empty}"
        );

        let one = CompanyModelConnectionWrite::new(
            "openai",
            Some("key".into()),
            vec!["gpt-4o".into()],
            true,
        )
        .unwrap();
        CompanyUseCases::validate_model_connections(std::slice::from_ref(&one))
            .expect("a single default provider is a valid set");

        let no_default = CompanyModelConnectionWrite::new(
            "anthropic",
            Some("key".into()),
            vec!["claude-a".into()],
            false,
        )
        .unwrap();
        assert!(
            CompanyUseCases::validate_model_connections(std::slice::from_ref(&no_default)).is_err(),
            "a set with no default has no answer for what an agent inherits"
        );
        assert!(
            CompanyUseCases::validate_model_connections(&[one.clone(), one]).is_err(),
            "a provider may only be configured once"
        );
        assert!(
            CompanyUseCases::validate_model_connections(&[no_default]).is_err(),
            "and still needs exactly one default"
        );
    }

    #[test]
    fn channel_defaults_normalize_and_validate_participants() {
        let mut write = CompanyWrite {
            name: "Acme".into(),
            slug: "acme".into(),
            channel_defaults: CompanyChannelDefaults {
                participant_emails: Some(vec![
                    "  Partner@Example.COM  ".into(),
                    "partner@example.com".into(),
                    " ".into(),
                    "@PUBLIC".into(),
                ]),
                ..CompanyChannelDefaults::default()
            },
            ..CompanyWrite::default()
        };
        write.normalize().unwrap();
        assert_eq!(
            write.channel_defaults.participant_emails,
            Some(vec!["partner@example.com".into(), "@public".into()])
        );

        let mut invalid = CompanyWrite {
            name: "Acme".into(),
            slug: "acme".into(),
            channel_defaults: CompanyChannelDefaults {
                participant_emails: Some(vec!["not-an-email".into()]),
                ..CompanyChannelDefaults::default()
            },
            ..CompanyWrite::default()
        };
        assert!(matches!(invalid.normalize(), Err(AppError::BadRequest(_))));

        let mut too_many = CompanyWrite {
            name: "Acme".into(),
            slug: "acme".into(),
            channel_defaults: CompanyChannelDefaults {
                participant_emails: Some(vec!["same@example.com".into(); 65]),
                ..CompanyChannelDefaults::default()
            },
            ..CompanyWrite::default()
        };
        assert!(matches!(too_many.normalize(), Err(AppError::BadRequest(_))));
    }

    #[tokio::test]
    async fn company_crud_flow_works() {
        let persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(Vec::new()),
            memberships: Mutex::new(Vec::new()),
        });
        let use_cases = CompanyUseCases::new(persistence);
        let user_id = Uuid::new_v4();

        // Create
        let company = use_cases
            .create_company(
                user_id,
                CompanyWrite {
                    name: "Acme Corp".to_string(),
                    slug: "acme-corp".to_string(),
                    enable_llm_spam_guardrail: Some(true),
                    memory_provider: None,
                    channel_defaults: crate::entities::company::CompanyChannelDefaults::default(),
                    avatar_url: Some(AvatarUrl::from("https://cdn.example.com/acme.png")),
                },
            )
            .await
            .unwrap();
        assert_eq!(company.name, "Acme Corp");
        assert_eq!(company.slug, "acme-corp");
        assert_eq!(company.enable_llm_spam_guardrail, Some(true));
        assert_eq!(
            company.avatar_url,
            Some(AvatarUrl::from("https://cdn.example.com/acme.png"))
        );

        // List
        let list = use_cases.list_user_companies(user_id).await.unwrap();
        assert_eq!(list.len(), 1);

        // Update
        let updated = use_cases
            .update_company(
                company.id,
                CompanyWrite {
                    name: "Acme Inc".to_string(),
                    slug: "acme-inc".to_string(),
                    enable_llm_spam_guardrail: Some(false),
                    ..CompanyWrite::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Acme Inc");
        assert_eq!(updated.enable_llm_spam_guardrail, Some(false));
        // A write sets every column, so a save that carries no picture is a save that clears it.
        assert_eq!(updated.avatar_url, None);

        // Delete
        use_cases.delete_company(company.id).await.unwrap();
        let list_after = use_cases.list_user_companies(user_id).await.unwrap();
        assert_eq!(list_after.len(), 0);
    }

    /// The security property, not just the status code: a stranger must not be able to tell a
    /// company that exists from one that does not.
    #[tokio::test]
    async fn a_foreign_company_is_indistinguishable_from_a_missing_one() {
        let owner = Uuid::new_v4();
        let stranger = Uuid::new_v4();
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: owner,
            name: "Acme".into(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };
        let persistence = MockCompanyPersistence {
            companies: Mutex::new(vec![company.clone()]),
            memberships: Mutex::new(Vec::new()),
        };

        owned_company(&persistence, owner, company.id)
            .await
            .expect("the owner gets their company");

        let foreign = owned_company(&persistence, stranger, company.id)
            .await
            .unwrap_err();
        let missing = owned_company(&persistence, stranger, Uuid::new_v4())
            .await
            .unwrap_err();

        assert!(matches!(foreign, AppError::NotFound(_)), "{foreign:?}");
        assert_eq!(
            foreign.to_string(),
            missing.to_string(),
            "a differing message would let an id probe enumerate other tenants"
        );
    }

    #[tokio::test]
    async fn only_owners_and_admins_receive_company_management_access() {
        let owner = Uuid::new_v4();
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: owner,
            name: "Acme".into(),
            slug: "acme".into(),
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
            created_at: Utc::now(),
        };
        let persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![company.clone()]),
            memberships: Mutex::new(vec![
                (admin, company.id, CompanyMembership::Admin),
                (member, company.id, CompanyMembership::Member),
            ]),
        });
        let use_cases = CompanyUseCases::new(persistence);

        assert_eq!(
            use_cases
                .list_managed_companies(owner)
                .await
                .expect("owner management list"),
            vec![company.clone()]
        );
        assert_eq!(
            use_cases
                .list_managed_companies(admin)
                .await
                .expect("admin management list"),
            vec![company.clone()]
        );
        assert!(
            use_cases
                .list_managed_companies(member)
                .await
                .expect("member management list")
                .is_empty()
        );
        assert_eq!(
            use_cases
                .managed_company(admin, company.id)
                .await
                .expect("admin management access"),
            company
        );
        assert!(use_cases.managed_company(member, company.id).await.is_err());
    }
}
