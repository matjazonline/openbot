use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, instrument, warn};
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess, CompanyChannelDefaults, CompanyModelConnection},
        user::Caller,
        value_objects::{AvatarUrl, ModelName, ModelProvider},
    },
    model_providers::is_supported_model_provider,
    use_cases::caller::require_operator,
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
        if !is_supported_model_provider(&provider) {
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

        Ok(())
    }
}

#[async_trait]
pub trait CompanyPersistence:
    crate::application::ports::administration::AdministrationPersistence + Send + Sync
{
    async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>>;
    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>>;
    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>>;
    async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company>;
    async fn delete(&self, id: Uuid) -> AppResult<()>;
    /// Update a company only when `user_id` owns it; `NotFound` when the scoped statement matches
    /// nothing. The use case authorizes first, so this is defence in depth, never the decision.
    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        write: CompanyWrite,
    ) -> AppResult<Company>;
    /// Delete a company only when `user_id` owns it; `NotFound` when nothing matched.
    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()>;
    /// Every company the user may *read*, each with what they are to it: the switchers and the
    /// default selection of the pages. Capped by the implementation, so it never authorizes an
    /// explicitly requested company -- [`CompanyPersistence::company_access`] does.
    ///
    /// Required: a default answering "owned companies only" would grant nothing wrong, but would
    /// let a double hide the admin and member paths from every test that uses it.
    async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>>;
    /// What the user is to one company, or `None` if they are nothing to it: the one membership
    /// read every authorization decision starts from. One row, never a scan of a capped list.
    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>>;

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

    /// Replace the root's own model connections (the company settings form).
    async fn replace_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()>;

    /// Replace one unit's own model connections (Plan07 02A): the caller must manage the unit.
    async fn replace_unit_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()>;

    /// Set one period's spend cap of a unit, in micro-USD (Plan07 02B.1, decision c): whoever
    /// manages the unit; anyone else gets not-found.
    async fn set_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
        cap_micros: i64,
    ) -> AppResult<()>;

    /// Remove one period's spend cap of a unit, under the same authority.
    async fn clear_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
    ) -> AppResult<()>;

    /// The ids of the model connections in `agent`'s own reach (Plan07 02A with decision d).
    async fn model_connections_in_agent_reach(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Vec<Uuid>>;

    /// Move a model connection to another unit; the caller must manage both (Plan07 01B.4, 02A).
    async fn move_model_connection(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        target: Uuid,
    ) -> AppResult<crate::application::ports::unit_resources::MovedResource>;
}

/// The caller's access to a company they can see at all.
///
/// A company that does not exist and a company the caller has no membership in are reported
/// *identically*, on purpose: telling the two apart would let anyone probe ids to enumerate other
/// tenants' companies. A database error propagates; it is never read as "not a member".
pub async fn readable_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<CompanyAccess> {
    persistence
        .company_access(user_id, company_id)
        .await?
        .ok_or_else(company_not_found)
}

/// Resolve a company the caller owns.
///
/// Anyone else, member or stranger, gets the same not-found as for a missing company (Plan06
/// 02A.4: a management refusal tells nothing). Ownership is the `Owner` membership, which the
/// schema keeps in step with `companies.user_id`.
pub async fn owned_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<Company> {
    let access = readable_company(persistence, user_id, company_id).await?;
    if access.membership.is_owner() {
        return Ok(access.company);
    }
    warn!("User {user_id} is not the owner of company {company_id}");
    Err(company_not_found())
}

/// Resolve a company the caller administers at company level: its owner or a root admin
/// ([`CompanyMembership::administers_company`]). Everyone else gets the same not-found as for a
/// missing company.
pub async fn managed_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<Company> {
    Ok(managed_company_access(persistence, user_id, company_id)
        .await?
        .company)
}

async fn managed_company_access(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<CompanyAccess> {
    let access = readable_company(persistence, user_id, company_id).await?;
    if access.membership.administers_company() {
        return Ok(access);
    }
    warn!("User {user_id} may not manage operations for company {company_id}");
    Err(company_not_found())
}

/// Resolve a company in which the caller manages at least one unit: the entry to unit-owned
/// configuration workspaces. It authorizes nothing by itself; every object is checked against
/// its own unit. Everyone else gets the same not-found as for a missing company.
pub async fn administered_company(
    persistence: &dyn CompanyPersistence,
    user_id: Uuid,
    company_id: Uuid,
) -> AppResult<CompanyAccess> {
    use crate::application::ports::administration::ManagementTarget;
    let access = readable_company(persistence, user_id, company_id).await?;
    if persistence
        .management_role(
            user_id,
            crate::domain::workflow::CompanyId::new(company_id),
            ManagementTarget::AnyUnit,
        )
        .await?
        .is_some()
    {
        return Ok(access);
    }
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
    /// The root's own connections: the company default lives there, so the set is never empty
    /// and holds exactly one default.
    pub fn validate_model_connections(
        connections: &[CompanyModelConnectionWrite],
    ) -> AppResult<()> {
        Self::validate_model_connection_bounds(connections)?;
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
        Ok(())
    }

    /// Bounds every unit's replace shares: the count limit and one row per provider.
    fn validate_model_connection_bounds(
        connections: &[CompanyModelConnectionWrite],
    ) -> AppResult<()> {
        if connections.len() > MAX_COMPANY_MODEL_CONNECTIONS {
            return Err(AppError::BadRequest(format!(
                "A company may configure at most {MAX_COMPANY_MODEL_CONNECTIONS} model providers."
            )));
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

    /// What the caller is to a company they can see; not-found for anyone else. See
    /// [`readable_company`].
    pub async fn readable_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyAccess> {
        readable_company(self.persistence.as_ref(), user_id, company_id).await
    }

    /// The company, only when the caller owns it. See [`owned_company`] for the 404/403 split.
    pub async fn owned_company(&self, user_id: Uuid, company_id: Uuid) -> AppResult<Company> {
        owned_company(self.persistence.as_ref(), user_id, company_id).await
    }

    /// The caller's access, only when they own the company or are one of its admins.
    pub async fn managed_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyAccess> {
        managed_company_access(self.persistence.as_ref(), user_id, company_id).await
    }

    /// Any company, for an operator watching the deployment-wide dashboard. Operators are usually
    /// not members of the companies they watch; anyone else is told there is nothing here.
    pub async fn operator_company(
        &self,
        caller: &Caller,
        company_id: Uuid,
    ) -> AppResult<Option<Company>> {
        require_operator(caller)?;
        self.persistence.get_by_id(company_id).await
    }

    #[instrument(skip_all)]
    pub async fn create_company(
        &self,
        user_id: Uuid,
        mut write: CompanyWrite,
    ) -> AppResult<Company> {
        write.normalize()?;

        info!(%user_id, "Creating company");
        self.persistence.create(user_id, write).await
    }

    #[instrument(skip_all)]
    pub async fn list_user_companies(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        self.persistence.list_by_user_id(user_id).await
    }

    /// The companies a user may read, each with what they are to it.
    ///
    /// What the mailbox scopes by, where an invited member belongs. Administration pages apply a
    /// narrower owner-or-admin filter with [`CompanyUseCases::list_managed_companies`].
    #[instrument(skip_all)]
    pub async fn list_accessible_companies(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        self.persistence.list_accessible_by_user_id(user_id).await
    }

    /// The caller's role over one management target, or not found: for pages that show a fragment
    /// of one object to whoever manages it.
    pub async fn require_management(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        target: crate::application::ports::administration::ManagementTarget,
    ) -> AppResult<crate::application::ports::administration::ManagementRole> {
        crate::application::ports::administration::require_management(
            self.persistence.as_ref(),
            user_id,
            crate::domain::workflow::CompanyId::new(company_id),
            target,
        )
        .await
    }

    /// The caller's access when they manage at least one of its units. See [`administered_company`].
    pub async fn administered_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<CompanyAccess> {
        administered_company(self.persistence.as_ref(), user_id, company_id).await
    }

    /// Companies in which the caller manages at least one unit, for unit workspace switchers.
    #[instrument(skip_all)]
    pub async fn list_administered_companies(
        &self,
        user_id: Uuid,
    ) -> AppResult<Vec<CompanyAccess>> {
        Ok(self
            .persistence
            .list_accessible_by_user_id(user_id)
            .await?
            .into_iter()
            .filter(|access| access.administers_units)
            .collect())
    }

    /// Companies the caller administers at company level.
    #[instrument(skip_all)]
    pub async fn list_managed_companies(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        Ok(self
            .persistence
            .list_accessible_by_user_id(user_id)
            .await?
            .into_iter()
            .filter(|access| access.membership.administers_company())
            .collect())
    }

    pub async fn update_company_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        mut write: CompanyWrite,
    ) -> AppResult<Company> {
        owned_company(self.persistence.as_ref(), user_id, id).await?;
        write.normalize()?;
        info!(company_id = %id, "Updating company");
        self.persistence.update_for_user(user_id, id, write).await
    }

    pub async fn delete_company_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        owned_company(self.persistence.as_ref(), user_id, id).await?;
        info!(company_id = %id, "Deleting company");
        self.persistence.delete_for_user(user_id, id).await
    }

    /// The model connections of the units this person manages (Plan07 decision a): root
    /// managers see all; anyone managing no unit gets the usual not-found.
    pub async fn list_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        let company = crate::domain::workflow::CompanyId::new(company_id);
        crate::application::ports::administration::require_management(
            self.persistence.as_ref(),
            user_id,
            company,
            crate::application::ports::administration::ManagementTarget::AnyUnit,
        )
        .await?;
        let units: std::collections::HashSet<Uuid> = self
            .persistence
            .managed_units(user_id, company)
            .await?
            .into_iter()
            .map(|unit| unit.as_uuid())
            .collect();
        Ok(self
            .persistence
            .list_model_connections(company_id)
            .await?
            .into_iter()
            .filter(|connection| units.contains(&connection.unit_id))
            .collect())
    }

    /// One unit's own model connections, the root's when `unit` is `None`, for that unit's form:
    /// whoever manages the unit (Plan07 decision a); anyone else gets the usual not-found.
    pub async fn list_unit_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit: Option<Uuid>,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        let choices = self
            .persistence
            .managed_unit_choices(user_id, crate::domain::workflow::CompanyId::new(company_id))
            .await?;
        let unit = choices
            .iter()
            .find(|choice| match unit {
                Some(unit) => choice.id.as_uuid() == unit,
                None => choice.is_root,
            })
            .ok_or_else(company_not_found)?
            .id
            .as_uuid();
        Ok(self
            .persistence
            .list_model_connections(company_id)
            .await?
            .into_iter()
            .filter(|connection| connection.unit_id == unit)
            .collect())
    }

    /// Set a unit's spend cap for `period` (`day` or `month`, UTC), in micro-USD (Plan07 02B.1).
    pub async fn set_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
        cap_micros: i64,
    ) -> AppResult<()> {
        self.persistence
            .set_unit_budget(user_id, company_id, unit_id, period, cap_micros)
            .await
    }

    /// Remove a unit's spend cap for `period`.
    pub async fn clear_unit_budget(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        period: &str,
    ) -> AppResult<()> {
        self.persistence
            .clear_unit_budget(user_id, company_id, unit_id, period)
            .await
    }

    /// Move a model connection between units the caller manages (Plan07 02A).
    pub async fn move_model_connection(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        id: Uuid,
        target: Uuid,
    ) -> AppResult<crate::application::ports::unit_resources::MovedResource> {
        self.persistence
            .move_model_connection(user_id, company_id, id, target)
            .await
    }

    /// Replace the root's own connections: whoever manages the root (Plan07 decision a), which
    /// persistence checks under its locks.
    pub async fn replace_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        Self::validate_model_connections(&connections)?;
        self.persistence
            .replace_model_connections_for_user(user_id, company_id, connections)
            .await
    }

    /// Replace one unit's own connections: whoever manages that unit. A unit may hold none and
    /// marks at most one of its own as its default (Plan07 02A, decision g).
    pub async fn replace_unit_model_connections(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        unit_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        // The root's own connections hold the company default whichever form saves them, so the
        // root's department form is held to the settings form's rules (never empty, exactly one
        // default); the company therefore always keeps a connection (G1 r2 D1).
        let root = self
            .persistence
            .managed_unit_choices(user_id, crate::domain::workflow::CompanyId::new(company_id))
            .await?
            .iter()
            .any(|choice| choice.is_root && choice.id.as_uuid() == unit_id);
        if root {
            Self::validate_model_connections(&connections)?;
        } else {
            Self::validate_model_connection_bounds(&connections)?;
            if connections.iter().filter(|item| item.is_default).count() > 1 {
                return Err(AppError::BadRequest(
                    "A department may mark at most one of its model providers as its default."
                        .into(),
                ));
            }
        }
        self.persistence
            .replace_unit_model_connections_for_user(user_id, company_id, unit_id, connections)
            .await
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        entities::company_member::CompanyMembership,
        use_cases::company_test_support::MockCompanyPersistence,
    };

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

        let xai = CompanyModelConnectionWrite::new(
            "xai",
            Some("xai-key".into()),
            vec!["grok-4.6".into()],
            true,
        )
        .expect("xAI is a supported company provider");
        CompanyUseCases::validate_model_connections(std::slice::from_ref(&xai))
            .expect("xAI can be the company's default provider");

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

    #[tokio::test]
    async fn company_crud_flow_works() {
        let persistence = Arc::new(MockCompanyPersistence::new(Vec::new()));
        let use_cases = CompanyUseCases::new(persistence);
        let user_id = Uuid::new_v4();

        // Create
        let company = use_cases
            .create_company(
                user_id,
                CompanyWrite {
                    name: "Acme Corp".to_string(),
                    slug: "acme-corp".to_string(),
                    channel_defaults: crate::entities::company::CompanyChannelDefaults::default(),
                    avatar_url: Some(AvatarUrl::from("https://cdn.example.com/acme.png")),
                },
            )
            .await
            .unwrap();
        assert_eq!(company.name, "Acme Corp");
        assert_eq!(company.slug, "acme-corp");
        assert_eq!(
            company.avatar_url,
            Some(AvatarUrl::from("https://cdn.example.com/acme.png"))
        );

        // List
        let list = use_cases.list_user_companies(user_id).await.unwrap();
        assert_eq!(list.len(), 1);

        // Update
        let updated = use_cases
            .update_company_for_user(
                user_id,
                company.id,
                CompanyWrite {
                    name: "Acme Inc".to_string(),
                    slug: "acme-inc".to_string(),
                    ..CompanyWrite::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Acme Inc");
        // A write sets every column, so a save that carries no picture is a save that clears it.
        assert_eq!(updated.avatar_url, None);

        // Delete
        use_cases
            .delete_company_for_user(user_id, company.id)
            .await
            .unwrap();
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
            avatar_url: None,
            created_at: Utc::now(),
        };
        let persistence = MockCompanyPersistence::new(vec![company.clone()]);

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
            avatar_url: None,
            created_at: Utc::now(),
        };
        let persistence = Arc::new(
            MockCompanyPersistence::new(vec![company.clone()]).with_memberships(vec![
                (admin, company.id, CompanyMembership::Admin),
                (member, company.id, CompanyMembership::Member),
            ]),
        );
        let use_cases = CompanyUseCases::new(persistence);

        assert_eq!(
            use_cases
                .list_managed_companies(owner)
                .await
                .expect("owner management list")
                .into_iter()
                .map(|access| access.company)
                .collect::<Vec<_>>(),
            vec![company.clone()]
        );
        assert_eq!(
            use_cases
                .list_managed_companies(admin)
                .await
                .expect("admin management list")
                .into_iter()
                .map(|access| access.company)
                .collect::<Vec<_>>(),
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
                .expect("admin management access")
                .company,
            company
        );
        // A member is told nothing, the same as a stranger (Plan06 02A.4).
        assert!(matches!(
            use_cases.managed_company(member, company.id).await,
            Err(AppError::NotFound(_))
        ));
    }

    fn acme(owner: Uuid) -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: owner,
            name: "Acme".into(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }
    }

    /// Owner-only actions: an admin, a member, a stranger and a missing id all get the same 404
    /// (Plan06 02A.4), and the write never runs for any of them.
    #[tokio::test]
    async fn only_the_owner_may_update_or_delete_and_everyone_else_is_told_nothing() {
        let owner = Uuid::new_v4();
        let admin = Uuid::new_v4();
        let member = Uuid::new_v4();
        let stranger = Uuid::new_v4();
        let company = acme(owner);
        let persistence = Arc::new(
            MockCompanyPersistence::new(vec![company.clone()]).with_memberships(vec![
                (admin, company.id, CompanyMembership::Admin),
                (member, company.id, CompanyMembership::Member),
            ]),
        );
        let use_cases = CompanyUseCases::new(persistence.clone());
        let rename = || CompanyWrite {
            name: "Renamed".into(),
            slug: "renamed".into(),
            ..CompanyWrite::default()
        };

        for refused in [admin, member] {
            assert!(matches!(
                use_cases
                    .update_company_for_user(refused, company.id, rename())
                    .await,
                Err(AppError::NotFound(_))
            ));
            assert!(matches!(
                use_cases.delete_company_for_user(refused, company.id).await,
                Err(AppError::NotFound(_))
            ));
        }
        let foreign = use_cases
            .update_company_for_user(stranger, company.id, rename())
            .await
            .unwrap_err();
        let missing = use_cases
            .delete_company_for_user(owner, Uuid::new_v4())
            .await
            .unwrap_err();
        assert!(matches!(foreign, AppError::NotFound(_)), "{foreign:?}");
        assert_eq!(foreign.to_string(), missing.to_string());
        assert_eq!(
            persistence.get_by_id(company.id).await.unwrap(),
            Some(company.clone()),
            "no refused caller changed anything"
        );

        let renamed = use_cases
            .update_company_for_user(owner, company.id, rename())
            .await
            .unwrap();
        assert_eq!(renamed.name, "Renamed");
        use_cases
            .delete_company_for_user(owner, company.id)
            .await
            .unwrap();
    }

    /// A database outage is an error, never a decision: not "not found", not "forbidden".
    #[tokio::test]
    async fn an_unreachable_database_is_an_error_not_a_refusal() {
        let use_cases = CompanyUseCases::new(Arc::new(MockCompanyPersistence::failing()));
        let user = Uuid::new_v4();
        let company = Uuid::new_v4();

        assert!(matches!(
            use_cases.readable_company(user, company).await,
            Err(AppError::Database(_))
        ));
        assert!(matches!(
            use_cases.owned_company(user, company).await,
            Err(AppError::Database(_))
        ));
        assert!(matches!(
            use_cases.managed_company(user, company).await,
            Err(AppError::Database(_))
        ));
    }

    #[tokio::test]
    async fn only_an_operator_reads_a_company_it_does_not_belong_to() {
        let company = acme(Uuid::new_v4());
        let use_cases =
            CompanyUseCases::new(Arc::new(MockCompanyPersistence::new(vec![company.clone()])));
        let watcher = Uuid::new_v4();

        assert_eq!(
            use_cases
                .operator_company(&Caller::operator(watcher), company.id)
                .await
                .unwrap(),
            Some(company.clone())
        );
        assert!(matches!(
            use_cases
                .operator_company(&Caller::user(watcher), company.id)
                .await,
            Err(AppError::NotFound(_))
        ));
    }
}
