//! Authorized skill-library operations and the capability snapshot used by agent execution.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{
        agent::Agent,
        creation::CreationProvenance,
        skill::{MAX_SKILL_INSTRUCTIONS_JSON_BYTES, Skill, SkillInstruction},
        user::Caller,
        value_objects::SkillSlug,
    },
    use_cases::{
        caller::require_operator,
        company::{CompanyPersistence, managed_company, owned_company},
    },
};

pub const MAX_SKILL_PAGE_SIZE: u16 = 100;

#[derive(Debug, Clone)]
pub struct SkillWrite {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    /// Workflow tools an agent must grant for this skill (06-D5); `None` keeps them.
    pub required_tools: Option<Vec<crate::domain::workflow::TypeName>>,
    pub created_by: Option<CreationProvenance>,
}

impl SkillWrite {
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        super::agent::validate_workflow_names(self.required_tools.as_deref(), "required tools")?;
        self.slug = self.slug.trim().to_ascii_lowercase();
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        self.trigger = self.trigger.trim().to_string();
        for SkillInstruction::Prompt { text } in &mut self.instructions {
            *text = text.trim().to_string();
        }

        let slug = SkillSlug::parse(&self.slug).map_err(AppError::BadRequest)?;
        let now = Utc::now();
        let candidate = Skill {
            id: Uuid::nil(),
            company_id: None,
            slug,
            name: self.name.clone(),
            description: self.description.clone(),
            trigger: self.trigger.clone(),
            instructions: self.instructions.clone(),
            created_by: CreationProvenance::system(),
            created_at: now,
            updated_at: now,
        };
        candidate.validate().map_err(AppError::BadRequest)?;
        let encoded = serde_json::to_vec(&self.instructions)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        if encoded.len() > MAX_SKILL_INSTRUCTIONS_JSON_BYTES {
            return Err(AppError::BadRequest(format!(
                "Skill instructions may be at most {MAX_SKILL_INSTRUCTIONS_JSON_BYTES} bytes."
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillCursor {
    pub updated_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Copy)]
pub struct SkillPageRequest {
    pub before: Option<SkillCursor>,
    pub limit: u16,
}

impl SkillPageRequest {
    pub fn validate(self) -> AppResult<Self> {
        if self.limit == 0 || self.limit > MAX_SKILL_PAGE_SIZE {
            return Err(AppError::BadRequest(format!(
                "Skill page size must be between 1 and {MAX_SKILL_PAGE_SIZE}."
            )));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillPage {
    pub items: Vec<Skill>,
    pub next: Option<SkillCursor>,
}

impl SkillPage {
    pub fn from_probe(mut items: Vec<Skill>, limit: u16) -> Self {
        let has_more = items.len() > usize::from(limit);
        items.truncate(usize::from(limit));
        let next = has_more.then(|| {
            let last = items
                .last()
                .expect("a non-empty probed page has a last item");
            SkillCursor {
                updated_at: last.updated_at,
                id: last.id,
            }
        });
        Self { items, next }
    }
}

#[async_trait]
pub trait SkillManagementPersistence: Send + Sync {
    async fn create_company(&self, company_id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn create_library(&self, write: SkillWrite) -> AppResult<Skill>;
    async fn copy_library_to_company(
        &self,
        company_id: Uuid,
        skill_id: Uuid,
        created_by: CreationProvenance,
    ) -> AppResult<Skill>;
    async fn get_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<Option<Skill>>;
    async fn get_library(&self, skill_id: Uuid) -> AppResult<Option<Skill>>;
    async fn list_company_page(
        &self,
        company_id: Uuid,
        page: SkillPageRequest,
    ) -> AppResult<SkillPage>;
    async fn count_company(&self, company_id: Uuid) -> AppResult<u64>;
    async fn list_library_page(&self, page: SkillPageRequest) -> AppResult<SkillPage>;
    async fn update_company(
        &self,
        company_id: Uuid,
        skill_id: Uuid,
        write: SkillWrite,
    ) -> AppResult<Skill>;
    async fn update_library(&self, skill_id: Uuid, write: SkillWrite) -> AppResult<Skill>;
    async fn delete_company(&self, company_id: Uuid, skill_id: Uuid) -> AppResult<()>;
    async fn delete_library(&self, skill_id: Uuid) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub struct StoredAgentCapabilities {
    pub agent: Agent,
    pub skills: Vec<Skill>,
}

#[async_trait]
pub trait AgentCapabilityReader: Send + Sync {
    async fn load_capabilities(
        &self,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>>;
}

pub struct SkillUseCases {
    persistence: Arc<dyn SkillManagementPersistence>,
    companies: Arc<dyn CompanyPersistence>,
    capabilities: Arc<dyn AgentCapabilityReader>,
}

impl SkillUseCases {
    pub fn new(
        persistence: Arc<dyn SkillManagementPersistence>,
        companies: Arc<dyn CompanyPersistence>,
        capabilities: Arc<dyn AgentCapabilityReader>,
    ) -> Self {
        Self {
            persistence,
            companies,
            capabilities,
        }
    }

    async fn authorize_read(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        managed_company(self.companies.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    async fn authorize_write(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        Ok(())
    }

    pub async fn verify_company_owner(&self, user_id: Uuid, company_id: Uuid) -> AppResult<()> {
        self.authorize_write(user_id, company_id).await
    }

    /// The ordered capability relationships stored for one company agent, for an owner-facing
    /// editor. The capability reader scopes the agent in the same query; the second check keeps a
    /// global library agent from being mistaken for a company-owned one.
    pub async fn company_agent_capabilities(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>> {
        self.authorize_read(user_id, company_id).await?;
        Ok(self
            .capabilities
            .load_capabilities(company_id, agent_id)
            .await?
            .filter(|stored| stored.agent.company_id == Some(company_id)))
    }

    /// The ordered skills attached to an operator-managed library agent. Passing a nil execution
    /// company is deliberate: global agents are selectable for every company and the persistence
    /// predicate admits only the global row when no company has that id.
    pub async fn library_agent_capabilities(
        &self,
        agent_id: Uuid,
    ) -> AppResult<Option<StoredAgentCapabilities>> {
        Ok(self
            .capabilities
            .load_capabilities(Uuid::nil(), agent_id)
            .await?
            .filter(|stored| stored.agent.is_library()))
    }

    #[instrument(skip_all, fields(%user_id, %company_id))]
    pub async fn create_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        mut write: SkillWrite,
    ) -> AppResult<Skill> {
        self.authorize_write(user_id, company_id).await?;
        write.created_by = Some(CreationProvenance::user(user_id));
        write.normalize()?;
        self.persistence.create_company(company_id, write).await
    }

    #[instrument(skip_all, fields(%user_id, %company_id, %skill_id))]
    pub async fn get_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        skill_id: Uuid,
    ) -> AppResult<Option<Skill>> {
        self.authorize_read(user_id, company_id).await?;
        self.persistence.get_company(company_id, skill_id).await
    }

    #[instrument(skip_all, fields(%user_id, %company_id))]
    pub async fn list_company_page(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        page: SkillPageRequest,
    ) -> AppResult<SkillPage> {
        self.authorize_read(user_id, company_id).await?;
        self.persistence
            .list_company_page(company_id, page.validate()?)
            .await
    }

    pub async fn count_company(&self, user_id: Uuid, company_id: Uuid) -> AppResult<u64> {
        self.authorize_read(user_id, company_id).await?;
        self.persistence.count_company(company_id).await
    }

    #[instrument(skip_all, fields(%user_id, %company_id, %skill_id))]
    pub async fn update_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        skill_id: Uuid,
        mut write: SkillWrite,
    ) -> AppResult<Skill> {
        self.authorize_write(user_id, company_id).await?;
        write.created_by = None;
        write.normalize()?;
        self.persistence
            .update_company(company_id, skill_id, write)
            .await
    }

    #[instrument(skip_all, fields(%user_id, %company_id, %skill_id))]
    pub async fn delete_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        skill_id: Uuid,
    ) -> AppResult<()> {
        self.authorize_write(user_id, company_id).await?;
        self.persistence.delete_company(company_id, skill_id).await
    }

    /// Insert a company-owned copy of a global library skill.
    #[instrument(skip_all, fields(%user_id, %company_id, %skill_id))]
    pub async fn create_skill_from_library(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        skill_id: Uuid,
    ) -> AppResult<Skill> {
        self.authorize_write(user_id, company_id).await?;
        self.persistence
            .copy_library_to_company(company_id, skill_id, CreationProvenance::user(user_id))
            .await
    }

    /// Operator only: a library skill is offered to every company.
    #[instrument(skip_all)]
    pub async fn create_library(&self, caller: &Caller, mut write: SkillWrite) -> AppResult<Skill> {
        require_operator(caller)?;
        write.created_by = Some(CreationProvenance::system());
        write.normalize()?;
        self.persistence.create_library(write).await
    }

    #[instrument(skip_all)]
    pub async fn get_library(&self, skill_id: Uuid) -> AppResult<Option<Skill>> {
        self.persistence.get_library(skill_id).await
    }

    #[instrument(skip_all)]
    pub async fn list_library_page(&self, page: SkillPageRequest) -> AppResult<SkillPage> {
        self.persistence.list_library_page(page.validate()?).await
    }

    /// The library as its manager reads it, for the operator workspace and its API. Companies
    /// browse the same rows through [`Self::list_library_page`] to copy them.
    #[instrument(skip_all)]
    pub async fn library_page_for(
        &self,
        caller: &Caller,
        page: SkillPageRequest,
    ) -> AppResult<SkillPage> {
        require_operator(caller)?;
        self.list_library_page(page).await
    }

    /// One library skill, for the operator workspace and its API.
    #[instrument(skip_all)]
    pub async fn library_skill_for(
        &self,
        caller: &Caller,
        skill_id: Uuid,
    ) -> AppResult<Option<Skill>> {
        require_operator(caller)?;
        self.get_library(skill_id).await
    }

    /// Operator only.
    #[instrument(skip_all)]
    pub async fn update_library(
        &self,
        caller: &Caller,
        skill_id: Uuid,
        mut write: SkillWrite,
    ) -> AppResult<Skill> {
        require_operator(caller)?;
        write.created_by = None;
        write.normalize()?;
        self.persistence.update_library(skill_id, write).await
    }

    /// Operator only.
    #[instrument(skip_all)]
    pub async fn delete_library(&self, caller: &Caller, skill_id: Uuid) -> AppResult<()> {
        require_operator(caller)?;
        self.persistence.delete_library(skill_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::skill::Skill;

    fn write(instructions: Vec<SkillInstruction>) -> SkillWrite {
        SkillWrite {
            required_tools: None,
            slug: "  REVIEW-Invoice  ".into(),
            name: " Review invoice ".into(),
            description: " Decide what it needs. ".into(),
            trigger: " When an invoice arrives. ".into(),
            instructions,
            created_by: None,
        }
    }

    #[tokio::test]
    async fn nonoperators_cannot_read_or_write_the_skill_library_management_surface() {
        use crate::adapters::persistence::{PostgresPersistence, test_support::own_database};
        use crate::use_cases::company_test_support::MockCompanyPersistence;
        let Some(database) = own_database().await else {
            return;
        };
        let persistence = Arc::new(PostgresPersistence::new(database.pool.clone()));
        let use_cases = SkillUseCases::new(
            persistence.clone(),
            Arc::new(MockCompanyPersistence::failing()),
            persistence.clone(),
        );
        let caller = Caller::user(Uuid::new_v4());
        let draft = || {
            write(vec![SkillInstruction::Prompt {
                text: "Review it.".into(),
            }])
        };
        assert!(matches!(
            use_cases.create_library(&caller, draft()).await,
            Err(AppError::NotFound(_))
        ));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM skills")
            .fetch_one(&database.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "denied create writes nothing");
        let stored = use_cases
            .create_library(&Caller::operator(caller.user_id), draft())
            .await
            .unwrap();
        assert!(matches!(
            use_cases.library_skill_for(&caller, stored.id).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            use_cases
                .library_page_for(
                    &caller,
                    SkillPageRequest {
                        before: None,
                        limit: 20
                    }
                )
                .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            use_cases.update_library(&caller, stored.id, draft()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            use_cases.delete_library(&caller, stored.id).await,
            Err(AppError::NotFound(_))
        ));
        assert_eq!(
            use_cases
                .get_library(stored.id)
                .await
                .unwrap()
                .unwrap()
                .name,
            stored.name
        );
    }

    #[test]
    fn normalize_lowercases_the_slug_and_trims_every_field() {
        let mut candidate = write(vec![SkillInstruction::Prompt {
            text: " Reply clearly. ".into(),
        }]);
        candidate.normalize().unwrap();
        assert_eq!(candidate.slug, "review-invoice");
        assert_eq!(candidate.name, "Review invoice");
        assert_eq!(candidate.description, "Decide what it needs.");
        assert_eq!(candidate.trigger, "When an invoice arrives.");
        assert_eq!(
            candidate.instructions,
            [SkillInstruction::Prompt {
                text: "Reply clearly.".into()
            }]
        );
    }

    fn skill() -> Skill {
        let now = Utc::now();
        Skill {
            id: Uuid::new_v4(),
            company_id: Some(Uuid::new_v4()),
            slug: SkillSlug::parse("delegate").unwrap(),
            name: "Delegate".into(),
            description: "Delegate work.".into(),
            trigger: "When specialist work is required.".into(),
            instructions: vec![SkillInstruction::Prompt {
                text: "Reply.".into(),
            }],
            created_by: CreationProvenance::system(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_page_cursor_is_emitted_only_when_the_probe_found_another_row() {
        let first = skill();
        let second = skill();

        let final_page = SkillPage::from_probe(vec![first.clone()], 1);
        assert_eq!(final_page.items.len(), 1);
        assert!(final_page.next.is_none());

        let page_with_more = SkillPage::from_probe(vec![first.clone(), second], 1);
        assert_eq!(
            page_with_more.items.as_slice(),
            std::slice::from_ref(&first)
        );
        assert_eq!(
            page_with_more.next,
            Some(SkillCursor {
                updated_at: first.updated_at,
                id: first.id,
            })
        );
    }
}
