use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence,
        channel::insert_email_allowlist_grants,
        integration::email_binding::{CanonicalEmailBinding, write_canonical_email_binding},
        participant::create_agent_principal_on,
    },
    app_error::{AppError, AppResult},
    entities::{creation::CreationProvenance, value_objects::ChannelSlug},
    services::agent_channel_tool::{
        AgentChannelProvisioning, ProvisionAgentChannelRequest, ProvisionedAgentChannel,
    },
};

#[async_trait]
impl AgentChannelProvisioning for PostgresPersistence {
    async fn provision_agent_channel(
        &self,
        request: ProvisionAgentChannelRequest,
    ) -> AppResult<ProvisionedAgentChannel> {
        let warnings = request.warnings.clone();
        let mut tx = self.pool().begin().await.map_err(AppError::from)?;
        let lock_key = format!("{}:{}", request.source_task_id, request.request_hash);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(AppError::from)?;

        if let Some((agent_id, channel_id, stored_warnings)) =
            sqlx::query_as::<_, (Uuid, Uuid, serde_json::Value)>(
            "SELECT agent_id, channel_id, warnings FROM agent_channel_provisions WHERE task_id = $1 AND request_hash = $2",
        )
        .bind(request.source_task_id)
        .bind(&request.request_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?
        {
            tx.commit().await.map_err(AppError::from)?;
            return Ok(ProvisionedAgentChannel {
                created: false,
                agent_id,
                channel_id,
                warnings: serde_json::from_value(stored_warnings).map_err(|error| {
                    AppError::Internal(format!("Stored provisioning warnings are invalid: {error}"))
                })?,
            });
        }

        let agent_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let agent_created_by = serde_json::to_value(&request.agent.created_by)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let channel_created_by = serde_json::to_value(&request.channel.created_by)
            .map_err(|error| AppError::Internal(error.to_string()))?;

        sqlx::query(
            r#"INSERT INTO agents
               (id, company_id, name, slug, system_prompt, description, created_by,
                memory_enabled, memory_persistence_mode, memory_recall_mode, memory_max_results)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        )
        .bind(agent_id)
        .bind(request.company_id)
        .bind(&request.agent.name)
        .bind(&request.agent.slug)
        .bind(&request.agent.system_prompt)
        .bind(&request.agent.description)
        .bind(agent_created_by)
        .bind(request.agent.memory_enabled)
        .bind(request.agent.memory_persistence_mode.as_str())
        .bind(request.agent.memory_recall_mode.as_str())
        .bind(i16::from(request.agent.memory_max_results))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        create_agent_principal_on(&mut tx, request.company_id, agent_id, &request.agent.name)
            .await?;

        sqlx::query(
            r#"INSERT INTO channels
               (id, company_id, owner_agent_id, name, description, access_mode, enabled,
                add_3rd_party, created_by, retrieve_company_memory, retrieve_agent_memory,
                retrieve_user_memory, persist_company_memory, persist_agent_memory,
                persist_user_memory)
               VALUES ($1, $2, $3, $4, $5, $6, TRUE, $7, $8, $9, $10, $11, $12, $13, $14)"#,
        )
        .bind(channel_id)
        .bind(request.company_id)
        .bind(agent_id)
        .bind(&request.channel.name)
        .bind(&request.channel.description)
        .bind(channel_access_mode(
            request.channel.participant_emails.as_ref(),
        ))
        .bind(request.channel.add_3rd_party)
        .bind(channel_created_by)
        .bind(request.channel.retrieve_company_memory)
        .bind(request.channel.retrieve_agent_memory)
        .bind(request.channel.retrieve_user_memory)
        .bind(request.channel.persist_company_memory)
        .bind(request.channel.persist_agent_memory)
        .bind(request.channel.persist_user_memory)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let participants = request
            .channel
            .participant_emails
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|email| {
                !email.eq_ignore_ascii_case(crate::entities::channel::PUBLIC_PARTICIPANT)
            })
            .map(|email| email.to_string())
            .collect();
        insert_email_allowlist_grants(&mut tx, request.company_id, channel_id, participants)
            .await?;

        sqlx::query(
            "INSERT INTO channel_slugs (company_id, channel_id, slug, is_primary) VALUES ($1, $2, $3, TRUE)",
        )
        .bind(request.company_id)
        .bind(channel_id)
        .bind(&request.channel.slug)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        // The provisioned channel gets its canonical email interface in this same transaction, so
        // an agent-created channel is reachable on exactly the terms a manager-created one is.
        let binding_created_by = request
            .channel
            .created_by
            .clone()
            .unwrap_or_else(CreationProvenance::system);
        write_canonical_email_binding(
            &mut tx,
            CanonicalEmailBinding {
                company_id: request.company_id,
                channel_id,
                channel_slug: &ChannelSlug::new(request.channel.slug.clone()),
                channel_name: &request.channel.name,
                created_by: &binding_created_by,
            },
        )
        .await?;
        sqlx::query(
            "INSERT INTO channel_agents (company_id, channel_id, agent_id, position) VALUES ($1, $2, $3, 0)",
        )
        .bind(request.company_id)
        .bind(channel_id)
        .bind(agent_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        sqlx::query(
            "INSERT INTO agent_channel_provisions (task_id, request_hash, agent_id, channel_id, warnings) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(request.source_task_id)
        .bind(&request.request_hash)
        .bind(agent_id)
        .bind(channel_id)
        .bind(serde_json::to_value(&warnings).map_err(|error| {
            AppError::Internal(format!("Provisioning warnings could not be stored: {error}"))
        })?)
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;

        tx.commit().await.map_err(AppError::from)?;
        Ok(ProvisionedAgentChannel {
            created: true,
            agent_id,
            channel_id,
            warnings,
        })
    }
}

fn channel_access_mode(participants: Option<&Vec<String>>) -> &'static str {
    match participants {
        Some(values)
            if values.iter().any(|value| {
                value.eq_ignore_ascii_case(crate::entities::channel::PUBLIC_PARTICIPANT)
            }) =>
        {
            "public"
        }
        Some(values) if !values.is_empty() => "allowlist",
        _ => "team",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adapters::persistence::test_support::test_pool,
        entities::creation::CreationProvenance,
        use_cases::{
            agent::{AgentPersistence, AgentWrite},
            channel::{ChannelPersistence, ChannelWrite},
            company::{CompanyPersistence, CompanyWrite},
            user::UserPersistence,
        },
    };

    #[tokio::test]
    async fn provisioning_is_atomic_attributed_and_idempotent() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("provision-{suffix}@example.com");
        let user = persistence
            .create_user(&format!("provision-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            user.id,
            CompanyWrite {
                name: "Provisioning Co".into(),
                slug: format!("provision-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        let parent = AgentPersistence::create(
            &persistence,
            company.id,
            AgentWrite {
                name: "Coordinator".into(),
                slug: "coordinator".into(),
                created_by: Some(CreationProvenance::user(user.id)),
                ..AgentWrite::default()
            },
        )
        .await
        .unwrap();
        let source = ChannelPersistence::create(
            &persistence,
            company.id,
            ChannelWrite {
                name: "Coordinator".into(),
                slug: "coordinator".into(),
                agent_ids: Some(vec![parent.id]),
                enabled: true,
                add_3rd_party: false,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: Some(CreationProvenance::user(user.id)),
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap();
        let task_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO background_tasks (id, company_id, channel_id, correlation_id, task_type, payload) \
             VALUES ($1, $2, $3, gen_random_uuid(), 'agent_run', '{}')",
        ).bind(task_id).bind(company.id).bind(source.id).execute(persistence.pool()).await.unwrap();
        let provenance =
            CreationProvenance::agent(parent.id, parent.name.clone(), source.id, task_id);
        let request = ProvisionAgentChannelRequest {
            warnings: vec![crate::use_cases::agent::ProvisioningWarning {
                code: "public_access_removed".into(),
                message: "original effective-default warning".into(),
            }],
            request_hash: "stable-request".into(),
            company_id: company.id,
            source_task_id: task_id,
            agent: AgentWrite {
                name: "Researcher".into(),
                slug: "researcher".into(),
                description: Some("Researches questions".into()),
                system_prompt: Some("Research carefully".into()),
                created_by: Some(provenance.clone()),
                ..AgentWrite::default()
            },
            channel: ChannelWrite {
                name: "Researcher".into(),
                slug: "researcher".into(),
                enabled: true,
                add_3rd_party: false,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: Some(provenance.clone()),
                ..ChannelWrite::default()
            },
        };
        let created = persistence
            .provision_agent_channel(request.clone())
            .await
            .unwrap();
        let mut changed_defaults_retry = request;
        changed_defaults_retry.warnings.clear();
        let retried = persistence
            .provision_agent_channel(changed_defaults_retry)
            .await
            .unwrap();
        assert!(created.created);
        assert!(!retried.created);
        assert_eq!(created.agent_id, retried.agent_id);
        assert_eq!(created.channel_id, retried.channel_id);
        assert_eq!(created.warnings, retried.warnings);
        assert_eq!(
            retried.warnings[0].message,
            "original effective-default warning"
        );
        let agent = AgentPersistence::get_by_id(&persistence, created.agent_id)
            .await
            .unwrap()
            .unwrap();
        let channel = ChannelPersistence::get_by_id(&persistence, created.channel_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(agent.created_by, provenance);
        assert_eq!(channel.created_by, provenance);
        assert_eq!(channel.agent_ids, Some(vec![agent.id]));
        assert!(channel.enabled);
        assert!(!channel.add_3rd_party);
    }
}
