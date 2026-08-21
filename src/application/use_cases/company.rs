use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess},
        company_member::CompanyMembership,
        value_objects::AvatarUrl,
    },
};

/// Everything one company write sets, so create and update cannot drift apart and so a caller
/// cannot transpose two same-typed arguments in a seven-parameter list.
///
/// Values reach persistence already normalized — see [`CompanyWrite::normalize`]. Mirrors
/// [`crate::use_cases::agent::AgentWrite`].
#[derive(Debug, Clone, Default)]
pub struct CompanyWrite {
    pub name: String,
    pub slug: String,
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enable_llm_spam_guardrail: Option<bool>,
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

        for field in [&mut self.api_key, &mut self.provider, &mut self.model] {
            if let Some(value) = field.as_mut() {
                *value = value.trim().to_string();
                if value.is_empty() {
                    *field = None;
                }
            }
        }

        Ok(())
    }
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

pub fn company_not_found() -> AppError {
    AppError::NotFound("Company not found, or you are not its owner.".into())
}

#[derive(Clone)]
pub struct CompanyUseCases {
    persistence: Arc<dyn CompanyPersistence>,
}

impl CompanyUseCases {
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
    /// What the mailbox scopes by, where an invited member belongs; the administration pages keep
    /// using [`CompanyUseCases::list_user_companies`], so being invited to a company does not
    /// become permission to reconfigure it.
    #[instrument(skip(self))]
    pub async fn list_accessible_companies(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        self.persistence.list_accessible_by_user_id(user_id).await
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
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chrono::Utc;

    use super::*;

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company> {
            let company = Company {
                id: Uuid::new_v4(),
                user_id,
                name: write.name,
                slug: write.slug.into(),
                api_key: write.api_key,
                provider: write.provider,
                model: write.model,
                enable_llm_spam_guardrail: write.enable_llm_spam_guardrail,
                avatar_url: write.avatar_url,
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

        async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company> {
            let mut list = self.companies.lock().unwrap();
            let company = list
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| AppError::Internal("Not found".into()))?;

            company.name = write.name;
            company.slug = write.slug.into();
            company.api_key = write.api_key;
            company.provider = write.provider;
            company.model = write.model;
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
    }

    #[tokio::test]
    async fn company_crud_flow_works() {
        let persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(Vec::new()),
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
                    api_key: Some("key123".to_string()),
                    provider: Some("google".to_string()),
                    model: Some("gemini-2.5-flash".to_string()),
                    enable_llm_spam_guardrail: Some(true),
                    avatar_url: Some(AvatarUrl::from("https://cdn.example.com/acme.png")),
                },
            )
            .await
            .unwrap();
        assert_eq!(company.name, "Acme Corp");
        assert_eq!(company.slug, "acme-corp");
        assert_eq!(company.api_key.as_deref(), Some("key123"));
        assert_eq!(company.provider.as_deref(), Some("google"));
        assert_eq!(company.model.as_deref(), Some("gemini-2.5-flash"));
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
            id: Uuid::new_v4(),
            user_id: owner,
            name: "Acme".into(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            created_at: Utc::now(),
        };
        let persistence = MockCompanyPersistence {
            companies: Mutex::new(vec![company.clone()]),
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
}
