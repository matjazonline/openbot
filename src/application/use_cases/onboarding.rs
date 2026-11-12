//! One submission owns preparation and all durable onboarding effects.
//! A canceled provider call may already have incurred cost; only successful retries reuse it.
use std::{collections::HashSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    agent::{
        AgentPersistence, AgentWrite,
        model_preparation::{prepare_library_write, prompt_model, validate_model_selection},
    },
    company::{CompanyPersistence, owned_company},
};
use crate::{
    app_error::{AppError, AppResult},
    application::ports::department::DepartmentWrite,
    entities::{
        agent::Agent,
        company::CompanyModelConnection,
        creation::CreationProvenance,
        department::DepartmentId,
        value_objects::{ModelName, ModelProvider},
    },
};

pub struct OnboardingSubmission {
    pub submission_key: Uuid,
    pub library_agent_ids: Vec<Uuid>,
    pub name: String,
    pub instructions: String,
    /// The company's first department, created with the first agent in it, when named
    /// (plan 01-departments 03C item 0).
    pub department_name: Option<String>,
}

/// Fully resolved provider input. Deliberately neither Debug nor Serialize: it contains a key.
pub struct PromptRequest {
    pub provider: ModelProvider,
    pub model: ModelName,
    pub api_key: Option<String>,
    pub instructions: String,
}

#[async_trait]
pub trait OnboardingPromptGenerator: Send + Sync {
    async fn generate(&self, request: PromptRequest) -> AppResult<String>;
}

#[async_trait]
pub trait OnboardingPersistence: Send + Sync {
    async fn begin(&self, user_id: Uuid, company_id: Uuid)
    -> AppResult<Box<dyn OnboardingSession>>;
}

/// All reads and writes after begin use this retained authority and connection.
#[async_trait]
pub trait OnboardingSession: Send {
    async fn existing_agents(&mut self, ids: &[Uuid]) -> AppResult<Vec<Agent>>;
    async fn model_connections(&mut self) -> AppResult<Vec<CompanyModelConnection>>;
    async fn model_api_key(&mut self, provider: &ModelProvider) -> AppResult<Option<String>>;
    async fn complete(self: Box<Self>, command: OnboardingCommand) -> AppResult<OnboardingOutcome>;
}

pub struct PreparedAgent {
    pub id: Uuid,
    pub library_id: Option<Uuid>,
    pub write: AgentWrite,
}

/// The department onboarding creates, under an id the submission determines, so a replay finds
/// it instead of creating another.
pub struct OnboardingDepartment {
    pub id: DepartmentId,
    pub write: DepartmentWrite,
}

pub struct OnboardingCommand {
    pub selected_ids: Vec<Uuid>,
    pub missing: Vec<PreparedAgent>,
    /// Created when missing; the first selected agent, when it is created here, joins it.
    pub department: Option<OnboardingDepartment>,
}

#[derive(Debug)]
pub struct OnboardedAgent {
    pub agent: Agent,
    /// The agent's default desk, as its creation recorded it (`agent:{id}`).
    pub channel: serde_json::Value,
}

#[derive(Debug)]
pub enum OnboardingOutcome {
    Rejected(String),
    Completed(Vec<OnboardedAgent>),
}

#[derive(Clone)]
pub struct OnboardingUseCases {
    companies: Arc<dyn CompanyPersistence>,
    agents: Arc<dyn AgentPersistence>,
    persistence: Arc<dyn OnboardingPersistence>,
    prompts: Arc<dyn OnboardingPromptGenerator>,
    operation_timeout: Duration,
    prompt_timeout: Duration,
}

impl OnboardingUseCases {
    pub fn new(
        companies: Arc<dyn CompanyPersistence>,
        agents: Arc<dyn AgentPersistence>,
        persistence: Arc<dyn OnboardingPersistence>,
        prompts: Arc<dyn OnboardingPromptGenerator>,
    ) -> Self {
        Self {
            companies,
            agents,
            persistence,
            prompts,
            operation_timeout: Duration::from_secs(90),
            prompt_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts(mut self, operation: Duration, prompt: Duration) -> Self {
        self.operation_timeout = operation;
        self.prompt_timeout = prompt;
        self
    }

    pub async fn onboard(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        submission: OnboardingSubmission,
    ) -> AppResult<OnboardingOutcome> {
        owned_company(self.companies.as_ref(), user_id, company_id).await?;
        let library = self.agents.list_library().await?;
        let selected = match selection(company_id, &submission, &library) {
            Ok(selected) => selected,
            Err(AppError::BadRequest(message)) => return Ok(OnboardingOutcome::Rejected(message)),
            Err(error) => return Err(error),
        };
        let department = match department(user_id, company_id, &submission) {
            Ok(department) => department,
            Err(AppError::BadRequest(message)) => return Ok(OnboardingOutcome::Rejected(message)),
            Err(error) => return Err(error),
        };
        tokio::time::timeout(
            self.operation_timeout,
            self.run(user_id, company_id, &submission, selected, department),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    async fn run(
        &self,
        user_id: Uuid,
        company_id: Uuid,
        submission: &OnboardingSubmission,
        selected: Vec<SelectedAgent>,
        department: Option<OnboardingDepartment>,
    ) -> AppResult<OnboardingOutcome> {
        // Begin errors remain authorization/infrastructure errors, never draft feedback.
        let mut session = self.persistence.begin(user_id, company_id).await?;
        let ids: Vec<_> = selected.iter().map(|item| item.id).collect();
        let existing = session.existing_agents(&ids).await?;
        let missing = selected
            .into_iter()
            .filter(|item| !existing.iter().any(|agent| agent.id == item.id))
            .collect::<Vec<_>>();
        let prepared = match self
            .prepare(user_id, submission, session.as_mut(), missing)
            .await
        {
            Ok(prepared) => prepared,
            Err(AppError::BadRequest(message)) => return Ok(OnboardingOutcome::Rejected(message)),
            Err(error) => return Err(error),
        };
        session
            .complete(OnboardingCommand {
                selected_ids: ids,
                missing: prepared,
                department,
            })
            .await
    }

    async fn prepare(
        &self,
        user_id: Uuid,
        submission: &OnboardingSubmission,
        session: &mut dyn OnboardingSession,
        missing: Vec<SelectedAgent>,
    ) -> AppResult<Vec<PreparedAgent>> {
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        let connections = session.model_connections().await?;
        let mut prepared = Vec::with_capacity(missing.len());
        for selected in missing {
            let (library_id, write) = match selected.library {
                Some(definition) => (
                    Some(definition.id),
                    prepare_library_write(&definition, user_id, &connections)?.0,
                ),
                None => (
                    None,
                    self.custom_write(user_id, submission, session, &connections)
                        .await?,
                ),
            };
            prepared.push(PreparedAgent {
                id: selected.id,
                library_id,
                write,
            });
        }
        Ok(prepared)
    }

    async fn custom_write(
        &self,
        user_id: Uuid,
        submission: &OnboardingSubmission,
        session: &mut dyn OnboardingSession,
        connections: &[CompanyModelConnection],
    ) -> AppResult<AgentWrite> {
        let (provider, model) = prompt_model(connections, None, None)?;
        let mut write = AgentWrite {
            name: submission.name.clone(),
            slug: custom_slug(&submission.name),
            provider: Some(provider.to_string()),
            model: Some(model.to_string()),
            created_by: Some(CreationProvenance::user(user_id)),
            ..AgentWrite::default()
        };
        write.normalize()?;
        validate_model_selection(connections, &write)?;
        let request = PromptRequest {
            api_key: session.model_api_key(&provider).await?,
            provider,
            model,
            instructions: submission.instructions.trim().into(),
        };
        write.system_prompt = Some(
            tokio::time::timeout(self.prompt_timeout, self.prompts.generate(request))
                .await
                .map_err(|_| timeout_error())??,
        );
        write.normalize()?;
        Ok(write)
    }
}

struct SelectedAgent {
    id: Uuid,
    library: Option<Agent>,
}

fn selection(
    company_id: Uuid,
    submission: &OnboardingSubmission,
    library: &[Agent],
) -> AppResult<Vec<SelectedAgent>> {
    let custom = !submission.name.trim().is_empty();
    if custom == submission.instructions.trim().is_empty() {
        return Err(AppError::BadRequest(
            "A custom agent needs both a channel name and instructions.".into(),
        ));
    }
    let ids: HashSet<_> = submission.library_agent_ids.iter().copied().collect();
    if ids.len() > 10 {
        return Err(AppError::BadRequest(
            "Select at most 10 library agents and one custom agent.".into(),
        ));
    }
    if ids.is_empty() && !custom {
        return Err(AppError::BadRequest(
            "Select at least one library agent or fill in the custom agent form.".into(),
        ));
    }
    let mut selected = Vec::new();
    if custom {
        selected.push(SelectedAgent {
            id: agent_id(
                company_id,
                submission.submission_key,
                &format!("custom:{}", custom_slug(&submission.name)),
            ),
            library: None,
        });
    }
    let mut seen = HashSet::new();
    for id in &submission.library_agent_ids {
        if !seen.insert(*id) {
            continue;
        }
        let definition = library
            .iter()
            .find(|agent| agent.id == *id)
            .ok_or_else(|| {
                AppError::BadRequest(
                    "One or more selected library agents are no longer available.".into(),
                )
            })?;
        selected.push(SelectedAgent {
            id: agent_id(
                company_id,
                submission.submission_key,
                &format!("library:{id}"),
            ),
            library: Some(definition.clone()),
        });
    }
    Ok(selected)
}

/// The named department as a normalized write under the submission's department id.
fn department(
    user_id: Uuid,
    company_id: Uuid,
    submission: &OnboardingSubmission,
) -> AppResult<Option<OnboardingDepartment>> {
    let Some(name) = submission
        .department_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return Ok(None);
    };
    let mut write = DepartmentWrite {
        name: name.to_owned(),
        slug: custom_slug(name),
        description: None,
        charter: None,
        parent_id: None,
        created_by: Some(CreationProvenance::user(user_id)),
    };
    write.normalize()?;
    Ok(Some(OnboardingDepartment {
        id: DepartmentId::new(agent_id(
            company_id,
            submission.submission_key,
            "department",
        )),
        write,
    }))
}

fn custom_slug(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn agent_id(company: Uuid, key: Uuid, role: &str) -> Uuid {
    let mut hash = Sha256::new();
    hash.update(b"p04.onboarding");
    hash.update(company.as_bytes());
    hash.update(key.as_bytes());
    hash.update(role.as_bytes());
    let bytes: [u8; 16] = hash.finalize()[..16]
        .try_into()
        .expect("SHA256 has 16 bytes");
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

fn timeout_error() -> AppError {
    AppError::ServiceUnavailable("Onboarding timed out. Please retry this submission.".into())
}
