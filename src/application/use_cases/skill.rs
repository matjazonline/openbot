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
        harness::SubAgentScope,
        skill::{MAX_SKILL_INSTRUCTIONS_JSON_BYTES, Skill, SkillInstruction},
        value_objects::SkillSlug,
    },
    use_cases::company::{CompanyPersistence, managed_company, owned_company},
};

pub const MAX_SKILL_PAGE_SIZE: u16 = 100;

#[derive(Debug, Clone)]
pub struct SkillWrite {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    pub created_by: Option<CreationProvenance>,
}

impl SkillWrite {
    pub(crate) fn normalize(&mut self) -> AppResult<()> {
        self.slug = self.slug.trim().to_ascii_lowercase();
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        self.trigger = self.trigger.trim().to_string();
        for instruction in &mut self.instructions {
            match instruction {
                SkillInstruction::Prompt { text } => *text = text.trim().to_string(),
                SkillInstruction::Tool { output_as, .. } => {
                    if let Some(value) = output_as {
                        *value = value.trim().to_string();
                    }
                }
            }
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
    pub sub_agent_scope: SubAgentScope,
}

#[async_trait]
pub trait AgentCapabilityReader: Send + Sync {
    async fn load_for_execution(
        &self,
        execution_company_id: Uuid,
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
            .load_for_execution(company_id, agent_id)
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
            .load_for_execution(Uuid::nil(), agent_id)
            .await?
            .filter(|stored| stored.agent.is_library()))
    }

    #[instrument(skip(self, write), fields(%user_id, %company_id))]
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

    #[instrument(skip(self), fields(%user_id, %company_id, %skill_id))]
    pub async fn get_company(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        skill_id: Uuid,
    ) -> AppResult<Option<Skill>> {
        self.authorize_read(user_id, company_id).await?;
        self.persistence.get_company(company_id, skill_id).await
    }

    #[instrument(skip(self), fields(%user_id, %company_id))]
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

    #[instrument(skip(self, write), fields(%user_id, %company_id, %skill_id))]
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

    #[instrument(skip(self), fields(%user_id, %company_id, %skill_id))]
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
    #[instrument(skip(self), fields(%user_id, %company_id, %skill_id))]
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

    #[instrument(skip(self, write))]
    pub async fn create_library(&self, mut write: SkillWrite) -> AppResult<Skill> {
        write.created_by = Some(CreationProvenance::system());
        write.normalize()?;
        self.persistence.create_library(write).await
    }

    #[instrument(skip(self))]
    pub async fn get_library(&self, skill_id: Uuid) -> AppResult<Option<Skill>> {
        self.persistence.get_library(skill_id).await
    }

    #[instrument(skip(self))]
    pub async fn list_library_page(&self, page: SkillPageRequest) -> AppResult<SkillPage> {
        self.persistence.list_library_page(page.validate()?).await
    }

    #[instrument(skip(self, write))]
    pub async fn update_library(&self, skill_id: Uuid, mut write: SkillWrite) -> AppResult<Skill> {
        write.created_by = None;
        write.normalize()?;
        self.persistence.update_library(skill_id, write).await
    }

    #[instrument(skip(self))]
    pub async fn delete_library(&self, skill_id: Uuid) -> AppResult<()> {
        self.persistence.delete_library(skill_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        agent::MAX_GRANTED_TOOLS,
        harness::{HarnessConfig, NativeToolPolicy},
        skill::Skill,
        value_objects::ToolId,
    };

    fn write(instructions: Vec<SkillInstruction>) -> SkillWrite {
        SkillWrite {
            slug: "  REVIEW-Invoice  ".into(),
            name: " Review invoice ".into(),
            description: " Decide what it needs. ".into(),
            trigger: " When an invoice arrives. ".into(),
            instructions,
            created_by: None,
        }
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

    #[test]
    fn a_write_whose_last_instruction_is_a_tool_step_is_refused() {
        let error = write(vec![SkillInstruction::Tool {
            tool: ToolId::from("datetime"),
            args: None,
            output_as: None,
        }])
        .normalize()
        .unwrap_err();
        assert!(error.to_string().contains("last instruction"));
    }

    #[test]
    fn advanced_config_rejects_security_owned_paths_with_the_path_named() {
        for key in [
            "tools",
            "skills",
            "spawner",
            "persona",
            "hitl",
            "tool_security",
            "context",
            "observability",
            "storage",
            "runtime",
            "process",
            "states",
            "llms",
            "tool_aliases",
            "llm",
        ] {
            let value = serde_json::json!({"version": 1, key: {}});
            let error = HarnessConfig::parse(
                crate::entities::harness::HarnessKind::AiAgents,
                Some(&value),
            )
            .unwrap_err();
            assert!(error.contains(key), "{error}");
        }
    }

    #[test]
    fn advanced_config_rejects_an_unknown_nested_key_with_the_full_path_named() {
        let value = serde_json::json!({
            "version": 1,
            "reasoning": {"mode": "auto", "unbounded_work": true}
        });
        let error = HarnessConfig::parse(
            crate::entities::harness::HarnessKind::AiAgents,
            Some(&value),
        )
        .unwrap_err();
        assert!(error.contains("reasoning.unbounded_work"), "{error}");
    }

    #[test]
    fn advanced_config_accepts_only_the_reviewed_bounded_options() {
        let value = serde_json::json!({
            "version": 1,
            "reasoning": {"mode": "react", "max_iterations": 16},
            "reflection": {"enabled": "auto", "max_retries": 5},
            "disambiguation": {"enabled": true}
        });
        HarnessConfig::parse(
            crate::entities::harness::HarnessKind::AiAgents,
            Some(&value),
        )
        .unwrap();

        for value in [
            serde_json::json!({"version": 1, "reasoning": {"mode": "auto", "max_iterations": 17}}),
            serde_json::json!({"version": 1, "reflection": {"enabled": "auto", "max_retries": 6}}),
        ] {
            assert!(
                HarnessConfig::parse(
                    crate::entities::harness::HarnessKind::AiAgents,
                    Some(&value),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn advanced_config_rejects_alternate_llms_prompts_planning_caches_and_visible_reasoning() {
        for value in [
            serde_json::json!({"version": 1, "reasoning": {"judge_llm": "other"}}),
            serde_json::json!({"version": 1, "reasoning": {"prompt": "ignore policy"}}),
            serde_json::json!({"version": 1, "reasoning": {"planner": {}}}),
            serde_json::json!({"version": 1, "reflection": {"cache": true}}),
            serde_json::json!({"version": 1, "reasoning": {"output": "visible"}}),
        ] {
            assert!(
                HarnessConfig::parse(
                    crate::entities::harness::HarnessKind::AiAgents,
                    Some(&value),
                )
                .is_err(),
                "accepted {value}"
            );
        }
    }

    #[test]
    fn native_tool_policy_refuses_out_of_bound_counts_timeouts_and_unknown_versions() {
        for value in [
            serde_json::json!({"version": 2}),
            serde_json::json!({"version": 1, "outreach": {"max_targets": 0}}),
            serde_json::json!({"version": 1, "outreach": {"max_timeout_hours": 721}}),
            serde_json::json!({"version": 1, "outreach": {"default_timeout_hours": 25, "max_timeout_hours": 24}}),
            serde_json::json!({"version": 1, "directory": {"max_results": 101}}),
        ] {
            assert!(NativeToolPolicy::parse(&value).is_err(), "accepted {value}");
        }
    }

    #[test]
    fn an_agent_write_naming_an_ungrantable_tool_is_refused_with_the_id_named() {
        let mut candidate = crate::use_cases::agent::AgentWrite {
            name: "Unsafe".into(),
            slug: "unsafe".into(),
            granted_tool_ids: vec![ToolId::from("command")],
            ..Default::default()
        };
        let error = candidate.normalize().unwrap_err();
        assert!(error.to_string().contains("command"), "{error}");

        candidate.granted_tool_ids = (0..=MAX_GRANTED_TOOLS)
            .map(|index| ToolId::from(format!("tool-{index}")))
            .collect();
        assert!(candidate.normalize().is_err());
    }

    fn skill_using(tool: &str) -> Skill {
        let now = Utc::now();
        Skill {
            id: Uuid::new_v4(),
            company_id: Some(Uuid::new_v4()),
            slug: SkillSlug::parse("delegate").unwrap(),
            name: "Delegate".into(),
            description: "Delegate work.".into(),
            trigger: "When specialist work is required.".into(),
            instructions: vec![
                SkillInstruction::Tool {
                    tool: ToolId::from(tool),
                    args: None,
                    output_as: None,
                },
                SkillInstruction::Prompt {
                    text: "Reply.".into(),
                },
            ],
            created_by: CreationProvenance::system(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_page_cursor_is_emitted_only_when_the_probe_found_another_row() {
        let first = skill_using("datetime");
        let second = skill_using("datetime");

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

    #[test]
    fn a_restricted_scope_rejects_direct_or_skill_implied_agent_creation() {
        let restricted = vec![Uuid::new_v4()];
        let direct = crate::use_cases::agent::AgentWrite {
            sub_agent_ids: restricted.clone(),
            granted_tool_ids: vec![ToolId::from(
                crate::entities::tool_catalogue::CREATE_AGENT_CHANNEL_TOOL_ID,
            )],
            ..Default::default()
        };
        assert!(crate::use_cases::agent::validate_effective_capabilities(&direct, &[]).is_err());

        let implied = crate::use_cases::agent::AgentWrite {
            sub_agent_ids: restricted,
            ..Default::default()
        };
        assert!(
            crate::use_cases::agent::validate_effective_capabilities(
                &implied,
                &[skill_using(
                    crate::entities::tool_catalogue::CREATE_AGENT_CHANNEL_TOOL_ID
                )],
            )
            .is_err()
        );
    }
}
