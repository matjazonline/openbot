//! Retains the company lock and the sole connection through bounded prompt preparation.
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    agent::{AGENT_COLUMNS, AgentDb},
    channel::prepare_library_agent_on,
    company::{model_api_key_on, model_connections_on},
    company_agent::{CompanyAgentCreation, create_company_agent_on, default_desk_key},
    department::{create_department_with_id_on, place_in_department_on},
    workflow::provisioning::ProvisioningDeployment,
};
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::WorkflowActor,
    domain::workflow::CompanyId,
    entities::{agent::Agent, company::CompanyModelConnection, value_objects::ModelProvider},
    use_cases::{company::company_not_found, onboarding::*},
};

#[derive(Clone)]
pub struct PostgresOnboarding {
    persistence: PostgresPersistence,
    templates: ProvisioningDeployment,
}
impl PostgresOnboarding {
    pub fn new(persistence: PostgresPersistence, templates: ProvisioningDeployment) -> Self {
        Self {
            persistence,
            templates,
        }
    }
}

struct Session {
    tx: Transaction<'static, Postgres>,
    persistence: PostgresPersistence,
    templates: ProvisioningDeployment,
    company_id: Uuid,
    user_id: Uuid,
}

#[async_trait]
impl OnboardingPersistence for PostgresOnboarding {
    async fn begin(
        &self,
        user_id: Uuid,
        company_id: Uuid,
    ) -> AppResult<Box<dyn OnboardingSession>> {
        let mut tx = self.persistence.pool().begin().await?;
        sqlx::query("SELECT set_config('lock_timeout', '45s', true), set_config('statement_timeout', '45s', true), set_config('idle_in_transaction_session_timeout', '45s', true)")
            .execute(&mut *tx).await?;
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE id=$1 FOR SHARE")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(company_not_found)?;
        let owner: Uuid =
            sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
                .bind(company_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(company_not_found)?;
        let role: String = sqlx::query_scalar(
            "SELECT role FROM company_members WHERE company_id = $1 AND user_id = $2 FOR SHARE",
        )
        .bind(company_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(company_not_found)?;
        if owner != user_id || role != "owner" {
            return Err(AppError::Forbidden(
                "Only the company owner can onboard agents.".into(),
            ));
        }
        Ok(Box::new(Session {
            tx,
            persistence: self.persistence.clone(),
            templates: self.templates.clone(),
            company_id,
            user_id,
        }))
    }
}

#[async_trait]
impl OnboardingSession for Session {
    async fn existing_agents(&mut self, ids: &[Uuid]) -> AppResult<Vec<Agent>> {
        sqlx::query_as::<_, AgentDb>(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents AS agent \
            WHERE agent.company_id = $1 AND agent.id = ANY($2)"
        ))
        .bind(self.company_id)
        .bind(ids)
        .fetch_all(&mut *self.tx)
        .await?
        .into_iter()
        .map(TryInto::try_into)
        .collect()
    }
    async fn model_connections(&mut self) -> AppResult<Vec<CompanyModelConnection>> {
        model_connections_on(&mut self.tx, self.company_id).await
    }
    async fn model_api_key(&mut self, provider: &ModelProvider) -> AppResult<Option<String>> {
        model_api_key_on(&self.persistence, &mut self.tx, self.company_id, provider).await
    }
    async fn complete(
        mut self: Box<Self>,
        command: OnboardingCommand,
    ) -> AppResult<OnboardingOutcome> {
        let placed = match self.department(command.department).await {
            Ok(placed) => placed,
            Err(error) => {
                self.tx.rollback().await?;
                return rejected(error);
            }
        };
        for missing in command.missing {
            if let Err(error) = self.insert(missing).await {
                self.tx.rollback().await?;
                return rejected(error);
            }
        }
        // The first agent joins the department last, once every desk exists, so the
        // department's desk gates are the last locks taken. Joining again changes nothing, and
        // a resubmission that names the department for the first time still places it
        // (01DEP-P03-B1-F6).
        if let (Some(department), Some(first)) = (placed, command.selected_ids.first().copied())
            && let Err(error) = self.place(first, department).await
        {
            self.tx.rollback().await?;
            return rejected(error);
        }
        let agents = self.existing_agents(&command.selected_ids).await?;
        let mut results = Vec::with_capacity(agents.len());
        for id in command.selected_ids {
            let agent = agents
                .iter()
                .find(|agent| agent.id == id)
                .cloned()
                .ok_or_else(|| {
                    AppError::Internal("Prepared onboarding agent was not found.".into())
                })?;
            let channel = self.channel(&agent).await?;
            results.push(OnboardedAgent { agent, channel });
        }
        self.tx.commit().await?;
        Ok(OnboardingOutcome::Completed(results))
    }
}

/// A refusal of the submission is its outcome, with nothing committed; anything else is an error.
fn rejected(error: AppError) -> AppResult<OnboardingOutcome> {
    match error {
        AppError::BadRequest(message)
        | AppError::Conflict(message)
        | AppError::NotFound(message) => Ok(OnboardingOutcome::Rejected(message)),
        error => Err(error),
    }
}

impl Session {
    /// The submission's department: found by its id on a replay, created otherwise.
    async fn department(
        &mut self,
        department: Option<OnboardingDepartment>,
    ) -> AppResult<Option<crate::entities::department::DepartmentId>> {
        let Some(department) = department else {
            return Ok(None);
        };
        let company = CompanyId::new(self.company_id);
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM departments WHERE company_id = $1 AND id = $2)",
        )
        .bind(self.company_id)
        .bind(department.id.as_uuid())
        .fetch_one(&mut *self.tx)
        .await?;
        if !exists {
            create_department_with_id_on(
                &mut self.tx,
                &self.templates,
                company,
                WorkflowActor::authenticated(self.user_id)?,
                department.id,
                &department.write,
                None,
            )
            .await?;
        }
        Ok(Some(department.id))
    }

    /// One agent through company-agent creation, with its default desk (01DEP-P03-D4).
    async fn insert(&mut self, mut prepared: PreparedAgent) -> AppResult<()> {
        if let Some(library_id) = prepared.library_id {
            prepare_library_agent_on(
                &mut self.tx,
                self.company_id,
                library_id,
                &mut prepared.write,
            )
            .await?;
        }
        Box::pin(create_company_agent_on(
            &mut self.tx,
            &self.templates,
            CompanyId::new(self.company_id),
            WorkflowActor::authenticated(self.user_id)?,
            CompanyAgentCreation {
                id: prepared.id,
                write: prepared.write,
                placement: None,
            },
        ))
        .await?;
        Ok(())
    }

    /// `agent` as a member of `department`.
    async fn place(
        &mut self,
        agent: Uuid,
        department: crate::entities::department::DepartmentId,
    ) -> AppResult<()> {
        let principal: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2")
                .bind(self.company_id)
                .bind(agent)
                .fetch_optional(&mut *self.tx)
                .await?
                .ok_or_else(|| AppError::Internal("Onboarded agent has no principal".into()))?;
        place_in_department_on(
            &mut self.tx,
            &self.templates,
            CompanyId::new(self.company_id),
            WorkflowActor::authenticated(self.user_id)?,
            department,
            crate::entities::transport::PrincipalId::new(principal),
            None,
        )
        .await
    }

    /// The agent's default desk, as its creation recorded it.
    async fn channel(&mut self, agent: &Agent) -> AppResult<serde_json::Value> {
        sqlx::query_scalar(
            "SELECT result FROM workflow_provisioning_commands \
             WHERE company_id = $1 AND command_key = $2",
        )
        .bind(self.company_id)
        .bind(default_desk_key(agent.id))
        .fetch_optional(&mut *self.tx)
        .await?
        .ok_or_else(|| AppError::Internal(format!("Agent '{}' has no default desk", agent.slug)))
    }
}

#[cfg(test)]
mod tests;
