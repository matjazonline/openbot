//! Retains the company lock and the sole connection through bounded prompt preparation.
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::{
    PostgresPersistence,
    agent::{AGENT_COLUMNS, AgentDb, insert_agent_on},
    channel::prepare_library_agent_on,
    company::{model_api_key_on, model_connections_on},
    participant::create_agent_principal_on,
    workflow::provisioning::{ProvisioningDeployment, assistant_channel_on},
};
use crate::{
    app_error::{AppError, AppResult},
    application::workflow::{
        WorkflowActor,
        provisioning::{AssistantChannelRequest, ProvisioningRefusal, ReplyMode},
    },
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
        for missing in command.missing {
            if let Err(error) = self.insert(missing).await {
                self.tx.rollback().await?;
                return match error {
                    AppError::BadRequest(message)
                    | AppError::Conflict(message)
                    | AppError::NotFound(message) => Ok(OnboardingOutcome::Rejected(message)),
                    error => Err(error),
                };
            }
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

impl Session {
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
        insert_agent_on(
            &mut self.tx,
            prepared.id,
            Some(self.company_id),
            &prepared.write,
        )
        .await?;
        create_agent_principal_on(
            &mut self.tx,
            self.company_id,
            prepared.id,
            &prepared.write.name,
        )
        .await?;
        Ok(())
    }

    async fn channel(
        &mut self,
        agent: &Agent,
    ) -> AppResult<Option<crate::application::workflow::provisioning::Provisioned<serde_json::Value>>>
    {
        if agent.model.is_none() {
            return Ok(None);
        }
        let outcome = assistant_channel_on(
            &mut self.tx,
            &self.templates,
            CompanyId::new(self.company_id),
            WorkflowActor::authenticated(self.user_id)?,
            AssistantChannelRequest {
                agent_slug: agent.slug.clone(),
                channel_slug: agent.slug.clone(),
                channel_name: agent.name.clone(),
                reply_mode: ReplyMode::Autonomous,
                command_key: format!("onboarding:{}", agent.id),
            },
        )
        .await;
        match outcome {
            Ok(outcome) => Ok(Some(outcome)),
            Err(AppError::Conflict(message)) => {
                Ok(Some(Err(ProvisioningRefusal::Conflict(message))))
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests;
