//! The one hand-written [`CompanyPersistence`] double, shared by every test that needs companies.
//!
//! It used to be declared six times, and every copy inherited the trait's defaults -- "every owned
//! company is `Owner`" -- so the admin and member paths were never exercised. Now each behaviour is
//! stated here once: memberships are explicit rows (the owner's row included, written the way
//! `CompanyPersistence::create` writes it), model connections are whatever the test configured, and
//! [`MockCompanyPersistence::failing`] answers every call with a database error.

use std::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        company::{Company, CompanyAccess, CompanyModelConnection},
        company_member::CompanyMembership,
        value_objects::ModelProvider,
    },
    use_cases::{
        company::{
            CompanyModelConnectionWrite, CompanyPersistence, CompanyWrite, company_not_found,
        },
        participant::test_support::TeamFixture,
    },
};

/// How the double answers "what is the account behind this address to the company" -- the
/// by-address half of [`TeamFixture`], which only the ingest-side tests use.
pub(crate) enum TeamByAddress {
    /// Not configured: a test that reaches it is wired wrong, so it panics rather than guess.
    NotConfigured,
    /// Every sender is a colleague (`Member`).
    EveryoneIsMember,
    /// `Member` for the listed `(company, address)` pairs, `None` for anyone else.
    Listed(Vec<(Uuid, String)>),
    /// The thread tests' outsiders-by-name rule: `None` for addresses containing "spammer",
    /// "unauthorized", "evil" or "notallowed", `Member` for everyone else.
    OutsidersByName,
}

pub(crate) struct MockCompanyPersistence {
    pub(crate) companies: Mutex<Vec<Company>>,
    /// `(user_id, company_id, membership)`; the owner of each company has an `Owner` row.
    pub(crate) memberships: Mutex<Vec<(Uuid, Uuid, CompanyMembership)>>,
    /// `None` until a test configures it: reaching the model-connection path unconfigured is a
    /// wiring mistake, so it panics rather than answer "no providers".
    model_connections: Mutex<Option<Vec<CompanyModelConnection>>>,
    model_api_key: Option<String>,
    team: TeamByAddress,
    failing: bool,
}

impl MockCompanyPersistence {
    /// These companies, each with its owner's `Owner` row, and nothing else configured.
    pub(crate) fn new(companies: Vec<Company>) -> Self {
        let memberships = companies
            .iter()
            .map(|company| (company.user_id, company.id, CompanyMembership::Owner))
            .collect();
        Self {
            companies: Mutex::new(companies),
            memberships: Mutex::new(memberships),
            model_connections: Mutex::new(None),
            model_api_key: None,
            team: TeamByAddress::NotConfigured,
            failing: false,
        }
    }

    /// A double whose every call fails as an unreachable database would.
    pub(crate) fn failing() -> Self {
        Self {
            failing: true,
            ..Self::new(Vec::new())
        }
    }

    /// Add accepted memberships, as `(user_id, company_id, membership)`.
    pub(crate) fn with_memberships(self, rows: Vec<(Uuid, Uuid, CompanyMembership)>) -> Self {
        self.memberships.lock().unwrap().extend(rows);
        self
    }

    /// The company's configured providers, and the key [`CompanyPersistence::model_api_key`]
    /// answers for any of them.
    pub(crate) fn with_model_connections(
        mut self,
        connections: Vec<CompanyModelConnection>,
        api_key: Option<&str>,
    ) -> Self {
        *self.model_connections.lock().unwrap() = Some(connections);
        self.model_api_key = api_key.map(str::to_string);
        self
    }

    pub(crate) fn with_team(mut self, team: TeamByAddress) -> Self {
        self.team = team;
        self
    }

    fn available(&self) -> AppResult<()> {
        if self.failing {
            Err(AppError::Database("mock database is unavailable".into()))
        } else {
            Ok(())
        }
    }

    fn membership(&self, user_id: Uuid, company_id: Uuid) -> Option<CompanyMembership> {
        self.memberships
            .lock()
            .unwrap()
            .iter()
            .find(|(member, company, _)| *member == user_id && *company == company_id)
            .map(|(_, _, membership)| *membership)
    }

    fn configured_model_connections(&self) -> Vec<CompanyModelConnection> {
        self.model_connections
            .lock()
            .unwrap()
            .clone()
            .expect("this double was not given model connections; configure them for this test")
    }

    fn owns(&self, user_id: Uuid, company_id: Uuid) -> bool {
        self.membership(user_id, company_id) == Some(CompanyMembership::Owner)
    }
}

#[async_trait]
impl CompanyPersistence for MockCompanyPersistence {
    async fn create(&self, user_id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        self.available()?;
        let company = Company {
            channel_defaults: write.channel_defaults,
            id: Uuid::new_v4(),
            user_id,
            name: write.name,
            slug: write.slug.into(),
            avatar_url: write.avatar_url,
            created_at: Utc::now(),
        };
        self.companies.lock().unwrap().push(company.clone());
        self.memberships
            .lock()
            .unwrap()
            .push((user_id, company.id, CompanyMembership::Owner));
        Ok(company)
    }

    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<Company>> {
        self.available()?;
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|company| company.id == id)
            .cloned())
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
        self.available()?;
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|company| company.slug.eq_ignore_ascii_case(slug))
            .cloned())
    }

    async fn list_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<Company>> {
        self.available()?;
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .filter(|company| self.owns(user_id, company.id))
            .cloned()
            .collect())
    }

    async fn update(&self, id: Uuid, write: CompanyWrite) -> AppResult<Company> {
        self.available()?;
        let mut companies = self.companies.lock().unwrap();
        let company = companies
            .iter_mut()
            .find(|company| company.id == id)
            .ok_or_else(company_not_found)?;
        company.name = write.name;
        company.slug = write.slug.into();
        company.avatar_url = write.avatar_url;
        company.channel_defaults = write.channel_defaults;
        Ok(company.clone())
    }

    async fn delete(&self, id: Uuid) -> AppResult<()> {
        self.available()?;
        self.companies
            .lock()
            .unwrap()
            .retain(|company| company.id != id);
        self.memberships
            .lock()
            .unwrap()
            .retain(|(_, company, _)| *company != id);
        Ok(())
    }

    async fn update_for_user(
        &self,
        user_id: Uuid,
        id: Uuid,
        write: CompanyWrite,
    ) -> AppResult<Company> {
        self.available()?;
        if !self.owns(user_id, id) {
            return Err(company_not_found());
        }
        self.update(id, write).await
    }

    async fn delete_for_user(&self, user_id: Uuid, id: Uuid) -> AppResult<()> {
        self.available()?;
        if !self.owns(user_id, id) {
            return Err(company_not_found());
        }
        self.delete(id).await
    }

    async fn list_accessible_by_user_id(&self, user_id: Uuid) -> AppResult<Vec<CompanyAccess>> {
        self.available()?;
        let companies = self.companies.lock().unwrap();
        Ok(companies
            .iter()
            .filter_map(|company| {
                self.membership(user_id, company.id)
                    .map(|membership| CompanyAccess {
                        access_level: crate::entities::access_level::fixture_level(membership),
                        company: company.clone(),
                        membership,
                    })
            })
            .collect())
    }

    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        self.available()?;
        let Some(membership) = self.membership(user_id, company_id) else {
            return Ok(None);
        };
        Ok(self
            .companies
            .lock()
            .unwrap()
            .iter()
            .find(|company| company.id == company_id)
            .map(|company| CompanyAccess {
                access_level: crate::entities::access_level::fixture_level(membership),
                company: company.clone(),
                membership,
            }))
    }

    async fn list_model_connections(
        &self,
        _company_id: Uuid,
    ) -> AppResult<Vec<CompanyModelConnection>> {
        self.available()?;
        Ok(self.configured_model_connections())
    }

    async fn model_api_key(
        &self,
        _company_id: Uuid,
        _provider: &ModelProvider,
    ) -> AppResult<Option<String>> {
        self.available()?;
        self.configured_model_connections();
        Ok(self.model_api_key.clone())
    }

    async fn replace_model_connections_for_user(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        connections: Vec<CompanyModelConnectionWrite>,
    ) -> AppResult<()> {
        self.available()?;
        if !self.owns(user_id, company_id) {
            return Err(company_not_found());
        }
        *self.model_connections.lock().unwrap() = Some(
            connections
                .into_iter()
                .map(|connection| CompanyModelConnection {
                    has_api_key: connection.api_key.is_some(),
                    provider: connection.provider,
                    models: connection.models,
                    is_default: connection.is_default,
                })
                .collect(),
        );
        Ok(())
    }
}

#[async_trait]
impl TeamFixture for MockCompanyPersistence {
    async fn membership_for_email(
        &self,
        company_id: Uuid,
        email: &str,
    ) -> AppResult<CompanyMembership> {
        self.available()?;
        let address = email.trim().to_lowercase();
        let on_the_team = match &self.team {
            TeamByAddress::NotConfigured => {
                panic!("this double was not told who belongs to a company by address")
            }
            TeamByAddress::EveryoneIsMember => true,
            TeamByAddress::Listed(members) => members.iter().any(|(company, member)| {
                *company == company_id && member.eq_ignore_ascii_case(&address)
            }),
            TeamByAddress::OutsidersByName => !["spammer", "unauthorized", "evil", "notallowed"]
                .iter()
                .any(|name| address.contains(name)),
        };
        Ok(if on_the_team {
            CompanyMembership::Member
        } else {
            CompanyMembership::None
        })
    }

    async fn company_access(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Option<CompanyAccess>> {
        CompanyPersistence::company_access(self, user_id, company_id).await
    }
}
