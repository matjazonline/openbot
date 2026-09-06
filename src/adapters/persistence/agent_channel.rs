use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence,
        agent::insert_agent_on,
        channel::insert_email_allowlist_grants,
        integration::email_binding::{CanonicalEmailBinding, write_canonical_email_binding},
        participant::create_agent_principal_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        creation::CreationProvenance,
        value_objects::{ChannelSlug, SkillSlug},
    },
    services::agent_channel_tool::{
        AgentChannelProvisioning, ProvisionAgentChannelRequest, ProvisionedAgentChannel,
    },
};

#[async_trait]
impl AgentChannelProvisioning for PostgresPersistence {
    async fn provision_agent_channel(
        &self,
        mut request: ProvisionAgentChannelRequest,
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
        let channel_created_by = serde_json::to_value(&request.channel.created_by)
            .map_err(|error| AppError::Internal(error.to_string()))?;

        request.agent.skill_ids =
            company_skill_ids(&mut tx, request.company_id, &request.skill_slugs).await?;
        insert_agent_on(&mut tx, agent_id, Some(request.company_id), &request.agent).await?;
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

async fn company_skill_ids(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    requested: &[SkillSlug],
) -> AppResult<Vec<Uuid>> {
    if requested.is_empty() {
        return Ok(Vec::new());
    }
    let requested_values = requested
        .iter()
        .map(|slug| slug.as_str())
        .collect::<Vec<_>>();
    let available = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, slug::text FROM skills \
         WHERE company_id = $1 AND slug::text = ANY($2) FOR KEY SHARE",
    )
    .bind(company_id)
    .bind(&requested_values)
    .fetch_all(&mut **transaction)
    .await
    .map_err(AppError::from)?;

    requested
        .iter()
        .map(|requested_slug| {
            available
                .iter()
                .find(|(_, slug)| slug == requested_slug.as_str())
                .map(|(id, _)| *id)
                .ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "Company skill '{}' does not exist.",
                        requested_slug.as_str()
                    ))
                })
        })
        .collect()
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
        entities::{creation::CreationProvenance, skill::SkillInstruction, value_objects::ToolId},
        use_cases::{
            agent::{AgentPersistence, AgentWrite},
            channel::{ChannelPersistence, ChannelWrite},
            company::{CompanyPersistence, CompanyWrite},
            skill::{AgentCapabilityReader, SkillManagementPersistence, SkillWrite},
            user::UserPersistence,
        },
    };

    async fn company_with_owner(
        persistence: &PostgresPersistence,
        suffix: &str,
    ) -> (
        crate::entities::user::User,
        crate::entities::company::Company,
    ) {
        let email = format!("provision-{suffix}@example.com");
        let user = persistence
            .create_user(&format!("provision-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let company = CompanyPersistence::create(
            persistence,
            user.id,
            CompanyWrite {
                name: "Provisioning Co".into(),
                slug: format!("provision-{suffix}"),
                ..CompanyWrite::default()
            },
        )
        .await
        .unwrap();
        (user, company)
    }

    #[tokio::test]
    async fn provisioning_is_atomic_attributed_and_idempotent() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let (user, company) = company_with_owner(&persistence, &suffix).await;
        let skill = SkillManagementPersistence::create_company(
            &persistence,
            company.id,
            SkillWrite {
                slug: format!("review-{suffix}"),
                name: "Review a request".into(),
                description: "Applies the company's review procedure.".into(),
                trigger: "When a request needs review.".into(),
                instructions: vec![SkillInstruction::Prompt {
                    text: "Apply the review procedure before answering.".into(),
                }],
                created_by: Some(CreationProvenance::user(user.id)),
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
            skill_slugs: vec![skill.slug.clone()],
            agent: AgentWrite {
                name: "Researcher".into(),
                slug: "researcher".into(),
                description: Some("Researches questions".into()),
                system_prompt: Some("Research carefully".into()),
                granted_tool_ids: vec![ToolId::from("web_fetch")],
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
        assert_eq!(agent.granted_tool_ids, [ToolId::from("web_fetch")]);
        let capabilities =
            AgentCapabilityReader::load_for_execution(&persistence, company.id, agent.id)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(capabilities.skills.len(), 1);
        assert_eq!(capabilities.skills[0].id, skill.id);
        assert_eq!(channel.created_by, provenance);
        assert_eq!(channel.agent_ids, Some(vec![agent.id]));
        assert!(channel.enabled);
        assert!(!channel.add_3rd_party);
    }

    #[tokio::test]
    async fn skill_resolution_refuses_another_companys_skill() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let first_suffix = Uuid::new_v4().simple().to_string();
        let second_suffix = Uuid::new_v4().simple().to_string();
        let (_, first_company) = company_with_owner(&persistence, &first_suffix).await;
        let (second_owner, second_company) = company_with_owner(&persistence, &second_suffix).await;
        let skill = SkillManagementPersistence::create_company(
            &persistence,
            second_company.id,
            SkillWrite {
                slug: format!("private-{second_suffix}"),
                name: "Private procedure".into(),
                description: "Belongs only to the second company.".into(),
                trigger: "When the second company requests it.".into(),
                instructions: vec![SkillInstruction::Prompt {
                    text: "Use the private procedure.".into(),
                }],
                created_by: Some(CreationProvenance::user(second_owner.id)),
            },
        )
        .await
        .unwrap();

        let mut transaction = persistence.pool().begin().await.unwrap();
        let error = company_skill_ids(&mut transaction, first_company.id, &[skill.slug])
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::BadRequest(_)));
        assert!(error.to_string().contains("does not exist"));
    }
}
