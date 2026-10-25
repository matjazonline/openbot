use std::{collections::HashSet, sync::Arc};

use axum::{
    Form, Router,
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::AppResult,
    application::workflow::{
        WorkflowActor,
        provisioning::{AssistantChannelRequest, AssistantChannels, ReplyMode},
    },
    domain::workflow::CompanyId,
    entities::{agent::Agent, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        agent::{AgentUseCases, LibraryCopyModel, pinned_default_model},
        company::{CompanyModelConnectionWrite, CompanyUseCases, CompanyWrite},
        user::UserUseCases,
    },
};

use super::{
    agent::{AgentInstructionRequest, ModelOverrides, create_agent_from_instructions},
    channel::slugify,
    ui::{load_account, workspace_user},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/onboarding", get(legacy_start))
        .route("/ui/onboarding", get(start))
        .route("/ui/onboarding/company", post(create_company))
        .route(
            "/ui/onboarding/companies/{company_id}/channel",
            get(channel_step).post(create_agents),
        )
        .route(
            "/ui/onboarding/companies/{company_id}/complete",
            get(complete),
        )
}

async fn legacy_start() -> Redirect {
    Redirect::permanent("/ui/onboarding")
}

#[derive(Debug, Deserialize)]
struct OnboardingCompanyForm {
    name: String,
    slug: String,
    api_key: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OnboardingChannelForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    instructions: String,
    library_agent_ids: Option<String>,
}

async fn start(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    match company_use_cases.list_user_companies(user.id).await {
        Ok(companies) if companies.is_empty() => Ok(Html(pages::onboarding_company_page(
            &mailbox_user,
            &config.app_domain_name,
            None,
        ))
        .into_response()),
        Ok(_) => Ok(Redirect::to("/ui").into_response()),
        Err(err) => Ok(Html(pages::onboarding_company_page(
            &mailbox_user,
            &config.app_domain_name,
            Some(&format!("Could not load your companies: {err}")),
        ))
        .into_response()),
    }
}

async fn create_company(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Form(form): Form<OnboardingCompanyForm>,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    if !company_use_cases
        .list_user_companies(user.id)
        .await?
        .is_empty()
    {
        return Ok(Redirect::to("/ui").into_response());
    }

    let connection = match (
        form.provider
            .as_deref()
            .filter(|value| !value.trim().is_empty()),
        form.model
            .as_deref()
            .filter(|value| !value.trim().is_empty()),
        form.api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty()),
    ) {
        (None, None, None) => None,
        (Some(provider), Some(model), Some(api_key)) => Some(CompanyModelConnectionWrite::new(
            provider,
            Some(api_key.to_string()),
            vec![model.to_string()],
            true,
        )?),
        _ => {
            return Ok(Html(pages::onboarding_company_page(
                &mailbox_user,
                &config.app_domain_name,
                Some("Provider, model, and API key must be supplied together."),
            ))
            .into_response());
        }
    };

    match company_use_cases
        .create_company(
            user.id,
            CompanyWrite {
                name: form.name.clone(),
                slug: form.slug.clone(),
                ..CompanyWrite::default()
            },
        )
        .await
    {
        Ok(company) => {
            if let Some(connection) = connection {
                company_use_cases
                    .replace_model_connections(user.id, company.id, vec![connection])
                    .await?;
            }
            Ok(
                Redirect::to(&format!("/ui/onboarding/companies/{}/channel", company.id))
                    .into_response(),
            )
        }
        Err(err) => Ok(Html(pages::onboarding_company_page(
            &mailbox_user,
            &config.app_domain_name,
            Some(&format!("Could not create the company: {err}")),
        ))
        .into_response()),
    }
}

async fn channel_step(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    let company = company_use_cases.get_company(company_id).await?;
    let Some(company) = company.filter(|company| company.user_id == user.id) else {
        return Ok(Redirect::to("/ui").into_response());
    };
    let library_agents = agent_use_cases.list_library_agents().await?;
    Ok(Html(pages::onboarding_channel_page(
        &mailbox_user,
        &company,
        &library_agents,
        None,
    ))
    .into_response())
}

/// Step 2 (p08 D4.7): the chosen agents, agent-only and pinned to the company default model when
/// there is one, then an assistant channel for each agent that has a model. The completion page
/// is rendered here, so a refusal shows next to its agent.
#[allow(clippy::too_many_arguments)]
async fn create_agents(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(assistant_channels): State<Arc<dyn AssistantChannels>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<OnboardingChannelForm>,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    let company = company_use_cases.get_company(company_id).await?;
    let Some(company) = company.filter(|company| company.user_id == user.id) else {
        return Ok(Redirect::to("/ui").into_response());
    };
    let library_agents = agent_use_cases.list_library_agents().await?;
    let submitted_ids =
        super::channel::parse_agent_ids_form(form.library_agent_ids.clone()).unwrap_or_default();
    let draft = pages::OnboardingAgentDraft {
        name: &form.name,
        instructions: &form.instructions,
        library_ids: &submitted_ids,
    };
    let mut selected_ids = HashSet::new();
    let selected_library_agents = submitted_ids
        .iter()
        .filter(|id| selected_ids.insert(**id))
        .map(|id| library_agents.iter().find(|agent| agent.id == *id))
        .collect::<Option<Vec<_>>>();
    let Some(selected_library_agents) = selected_library_agents else {
        return Ok(Html(pages::onboarding_channel_draft_page(
            &mailbox_user,
            &company,
            &library_agents,
            &draft,
            Some("One or more selected library agents are no longer available."),
        ))
        .into_response());
    };
    let custom_name = form.name.trim();
    let custom_instructions = form.instructions.trim();
    if selected_library_agents.is_empty()
        && custom_name.is_empty()
        && custom_instructions.is_empty()
    {
        return Ok(Html(pages::onboarding_channel_draft_page(
            &mailbox_user,
            &company,
            &library_agents,
            &draft,
            Some("Select at least one library agent or fill in the custom agent form."),
        ))
        .into_response());
    }
    if custom_name.is_empty() != custom_instructions.is_empty() {
        return Ok(Html(pages::onboarding_channel_draft_page(
            &mailbox_user,
            &company,
            &library_agents,
            &draft,
            Some("A custom agent needs both a channel name and instructions."),
        ))
        .into_response());
    }

    let connections = agent_use_cases
        .list_company_model_connections(user.id, company_id)
        .await?;
    let default_model = pinned_default_model(&connections);
    let mut created_agents = Vec::new();

    if !custom_name.is_empty() {
        let slug = slugify(custom_name);
        match create_agent_from_instructions(
            &agent_use_cases,
            AgentInstructionRequest {
                user_id: user.id,
                company_id,
                name: custom_name,
                slug: &slug,
                instructions: custom_instructions,
                overrides: ModelOverrides {
                    provider: default_model
                        .as_ref()
                        .map(|(provider, _)| provider.as_str()),
                    model: default_model.as_ref().map(|(_, model)| model.as_str()),
                },
                avatar_url: None,
            },
        )
        .await
        {
            Ok(agent) => created_agents.push(agent),
            Err(err) => {
                return Ok(Html(pages::onboarding_channel_draft_page(
                    &mailbox_user,
                    &company,
                    &library_agents,
                    &draft,
                    Some(&err),
                ))
                .into_response());
            }
        }
    }

    for library_agent in selected_library_agents {
        match agent_use_cases
            .create_agent_from_library(
                user.id,
                company_id,
                library_agent.id,
                LibraryCopyModel::PinDefault,
            )
            .await
        {
            Ok(provisioned) => created_agents.push(provisioned.agent),
            Err(err) => {
                for agent in created_agents {
                    let _ = agent_use_cases
                        .delete_agent(user.id, company_id, agent.id)
                        .await;
                }
                return Ok(Html(pages::onboarding_channel_draft_page(
                    &mailbox_user,
                    &company,
                    &library_agents,
                    &draft,
                    Some(&format!("Could not create agent from library: {err}")),
                ))
                .into_response());
            }
        }
    }

    let caller = WorkflowActor::authenticated(user.id)?;
    let mut onboarded = Vec::with_capacity(created_agents.len());
    for agent in &created_agents {
        let channel =
            onboarding_channel(assistant_channels.as_ref(), company_id, caller, agent).await?;
        onboarded.push(pages::OnboardedAgent { agent, channel });
    }
    Ok(Html(pages::onboarding_complete_page(
        &mailbox_user,
        &company,
        &onboarded,
    ))
    .into_response())
}

/// One agent's assistant channel: slug = its handle, name = its name, autonomous replies, team
/// access, under its onboarding key. An agent without a model is skipped, not refused.
async fn onboarding_channel(
    channels: &dyn AssistantChannels,
    company_id: Uuid,
    caller: WorkflowActor,
    agent: &Agent,
) -> AppResult<pages::OnboardingChannel> {
    if agent.model.is_none() {
        return Ok(pages::OnboardingChannel::NoModel);
    }
    let created = channels
        .create_assistant_channel(
            CompanyId::new(company_id),
            caller,
            AssistantChannelRequest {
                agent_slug: agent.slug.clone(),
                channel_slug: agent.slug.clone(),
                channel_name: agent.name.clone(),
                reply_mode: ReplyMode::Autonomous,
                command_key: onboarding_key(agent),
            },
        )
        .await?;
    Ok(match created {
        Ok(created) => created_channel(&created),
        Err(refusal) => pages::OnboardingChannel::Refused(refusal.message()),
    })
}

/// What the completion page shows for an agent on a reload: the channel its onboarding command
/// stored, if any. A refusal is not stored, so an agent with a model and no channel is missing.
async fn stored_onboarding_channel(
    channels: &dyn AssistantChannels,
    company_id: Uuid,
    caller: WorkflowActor,
    agent: &Agent,
) -> AppResult<pages::OnboardingChannel> {
    let stored = channels
        .assistant_channel_result(CompanyId::new(company_id), caller, &onboarding_key(agent))
        .await?;
    Ok(match stored {
        Some(created) => created_channel(&created),
        None if agent.model.is_none() => pages::OnboardingChannel::NoModel,
        None => pages::OnboardingChannel::Missing,
    })
}

/// The assistant-channel command key of an onboarding agent: stable per agent, so a replay
/// returns the stored channel and the completion page can read it back.
fn onboarding_key(agent: &Agent) -> String {
    format!("onboarding:{}", agent.id)
}

fn created_channel(created: &serde_json::Value) -> pages::OnboardingChannel {
    let channel = &created["channel"];
    match (
        channel["id"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok()),
        channel["address"].as_str(),
    ) {
        (Some(channel_id), Some(address)) => pages::OnboardingChannel::Created {
            channel_id,
            address: address.to_owned(),
        },
        _ => pages::OnboardingChannel::Missing,
    }
}

/// The completion page again, from what was stored: each company agent and the channel its
/// onboarding command created.
#[allow(clippy::too_many_arguments)]
async fn complete(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    State(assistant_channels): State<Arc<dyn AssistantChannels>>,
    State(config): State<Arc<AppConfig>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    let company = company_use_cases.get_company(company_id).await?;
    let Some(company) = company.filter(|company| company.user_id == user.id) else {
        return Ok(Redirect::to("/ui").into_response());
    };
    let agents = agent_use_cases
        .list_company_agents(user.id, company_id)
        .await?;
    let caller = WorkflowActor::authenticated(user.id)?;
    let mut onboarded = Vec::with_capacity(agents.len());
    for agent in &agents {
        let channel =
            stored_onboarding_channel(assistant_channels.as_ref(), company_id, caller, agent)
                .await?;
        onboarded.push(pages::OnboardedAgent { agent, channel });
    }
    Ok(Html(pages::onboarding_complete_page(
        &mailbox_user,
        &company,
        &onboarded,
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::entities::{
        agent::Agent, channel::Channel, company::Company, company_member::CompanyMembership,
    };

    fn company() -> Company {
        Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".to_string(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        }
    }

    fn library_agent(name: &str, slug: &str) -> Agent {
        Agent {
            id: Uuid::new_v4(),
            company_id: None,
            name: name.to_string(),
            slug: slug.to_string(),
            provider: None,
            model: None,
            system_prompt: Some("Help with email.".to_string()),
            description: Some("A ready-made helper".to_string()),
            avatar_url: None,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn onboarding_pages_render_each_step() {
        let company = company();
        let email = EmailAddress::from("owner@example.com");
        let user = pages::MailboxUser {
            id: company.user_id,
            username: "owner",
            email: &email,
            avatar_url: None,
            is_operator: false,
            company_membership: CompanyMembership::Owner,
        };
        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Customer Support".to_string(),
            description: None,
            slug: "customer-support".into(),
            alias_slugs: Vec::new(),
            participant_emails: None,
            access_mode: crate::entities::channel::ChannelAccessMode::Team,
            principal_grants: Vec::new(),
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
            settings_revision: 0,
        };

        let company_page = pages::onboarding_company_page(&user, "example.com", None);
        assert!(company_page.contains("Step 1 of 3"));
        assert!(company_page.contains("action=\"/ui/onboarding/company\""));
        assert!(company_page.contains("href=\"/ui\" class=\"btn btn-ghost\">Skip</a>"));
        assert!(company_page.contains(">.example.com</span>"));
        assert!(company_page.contains("/assets/app.css"));
        assert!(company_page.contains("/ui/profile"));

        let library_agent = library_agent("Scheduler", "scheduler");
        let channel_page = pages::onboarding_channel_page(
            &user,
            &company,
            std::slice::from_ref(&library_agent),
            None,
        );
        assert!(channel_page.contains("Step 2 of 3"));
        let page = pages::onboarding_channel_draft_page(
            &user,
            &company,
            &[],
            &pages::OnboardingAgentDraft {
                name: "Retained name",
                instructions: "Retained <instructions>",
                library_ids: &[],
            },
            Some("Fix the selected configuration"),
        );
        assert!(!page.contains("harness_kind"));
        assert!(page.contains("value=\"Retained name\""));
        assert!(page.contains("Retained &lt;instructions&gt;"));

        assert!(channel_page.contains(&format!(
            "action=\"/ui/onboarding/companies/{}/channel\"",
            company.id
        )));
        assert!(channel_page.contains("name=\"library_agent_ids\""));
        assert!(channel_page.contains(&format!("value=\"{}\"", library_agent.id)));
        assert!(channel_page.contains("Scheduler"));
        assert!(channel_page.contains("Each agent gets a channel with the same name"));
        assert!(channel_page.contains("Custom agent and channel name"));
        assert!(channel_page.contains("Required when creating a custom agent."));
        assert!(
            !channel_page
                .contains("Channel name <span class=\"font-normal opacity-60\">(optional)")
        );
        assert!(channel_page.contains(r##"data-submit="busy-once""##));
        // The spinner is Tailwind's `animate-spin` on an inline SVG, not daisyUI's
        // `loading-spinner` -- see `busy_submit_button` for why.
        assert!(
            channel_page
                .contains(r##"<svg data-progress class="hidden size-4 shrink-0 animate-spin""##)
        );
        assert!(channel_page.contains("Creating email agents…"));

        // p08 D4.7: each agent with its channel, or why it has none and where to make one.
        let scheduler = library_agent.clone();
        let triage = super::tests::library_agent("Triage", "triage");
        let unpinned = super::tests::library_agent("Drafts", "drafts");
        let complete_page = pages::onboarding_complete_page(
            &user,
            &company,
            &[
                pages::OnboardedAgent {
                    agent: &scheduler,
                    channel: pages::OnboardingChannel::Created {
                        channel_id: channel.id,
                        address: "scheduler@acme.example.com".into(),
                    },
                },
                pages::OnboardedAgent {
                    agent: &triage,
                    channel: pages::OnboardingChannel::Refused(
                        "The address is <already> used".into(),
                    ),
                },
                pages::OnboardedAgent {
                    agent: &unpinned,
                    channel: pages::OnboardingChannel::NoModel,
                },
            ],
        );
        assert!(complete_page.contains("Step 3 of 3"));
        assert!(complete_page.contains("Your email agents are ready"));
        assert!(complete_page.contains("scheduler@acme.example.com"));
        assert!(complete_page.contains("Forward a message"));
        assert!(complete_page.contains("The address is &lt;already&gt; used"));
        assert!(complete_page.contains(&format!(
            "href=\"/ui/agents?company_id={}&agent_id={}\"",
            company.id, triage.id
        )));
        assert!(complete_page.contains(
            "Connect a model in company settings, then create an assistant channel from each agent's page."
        ));
        assert!(complete_page.contains(&format!(
            "href=\"/ui?company_id={}&channel_id={}\" class=\"btn btn-primary\">Finish</a>",
            company.id, channel.id
        )));

        let no_channels = pages::onboarding_complete_page(
            &user,
            &company,
            &[pages::OnboardedAgent {
                agent: &unpinned,
                channel: pages::OnboardingChannel::NoModel,
            }],
        );
        assert!(no_channels.contains("Your agents are created"));
        assert!(!no_channels.contains("Forward a message"));
        assert!(no_channels.contains(&format!(
            "href=\"/ui?company_id={}\" class=\"btn btn-primary\">Finish</a>",
            company.id
        )));
    }

    /// p08 D4.7, F085.3: each agent with a model gets one team assistant channel under its
    /// onboarding key, and replaying the step for the existing agents returns that channel and
    /// adds none. An agent without a model is skipped, a taken address is the refusal, and a
    /// reload reads back only what was stored -- for the company's managers.
    #[tokio::test]
    async fn the_channel_step_gives_each_modelled_agent_one_assistant_channel() {
        use crate::adapters::persistence::workflow::provisioning::{
            AssistantChannelService, ProvisioningDeployment,
        };
        use crate::adapters::persistence::{PostgresPersistence, test_support::test_pool};
        use crate::adapters::workflow_source::WorkflowSourceDecoder;
        use crate::application::workflow::templates::TemplateCatalogue;
        use crate::use_cases::{
            agent::{AgentPersistence, AgentWrite},
            company::CompanyPersistence,
            user::UserPersistence,
        };

        let Some(pool) = test_pool().await else {
            return;
        };
        let persistence = PostgresPersistence::new(pool);
        let suffix = Uuid::new_v4().simple().to_string();
        let owner = persistence
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let outsider_name = format!("{suffix}-out");
        let outsider = persistence
            .create_user(
                &outsider_name,
                &format!("{outsider_name}@example.test"),
                "hash",
            )
            .await
            .unwrap()
            .id;
        let company = CompanyPersistence::create(
            &persistence,
            owner,
            CompanyWrite {
                name: "Onboarded".into(),
                slug: suffix.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        CompanyPersistence::replace_model_connections_for_user(
            &persistence,
            owner,
            company,
            vec![
                CompanyModelConnectionWrite::new(
                    "openai",
                    Some("test-key".into()),
                    vec!["gpt-4o-mini".into()],
                    true,
                )
                .unwrap(),
            ],
        )
        .await
        .unwrap();
        let agent = |slug: &str, pinned: bool| AgentWrite {
            name: format!("Agent {slug}"),
            slug: slug.into(),
            provider: pinned.then(|| "openai".into()),
            model: pinned.then(|| "gpt-4o-mini".into()),
            ..Default::default()
        };
        let desk = AgentPersistence::create(&persistence, company, agent("desk", true))
            .await
            .unwrap();
        let drafts = AgentPersistence::create(&persistence, company, agent("drafts", false))
            .await
            .unwrap();
        let taken = AgentPersistence::create(&persistence, company, agent("taken", true))
            .await
            .unwrap();
        let channels = AssistantChannelService::new(
            persistence.clone(),
            ProvisioningDeployment::new(
                Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
                &[],
                "agents.test",
            ),
        );
        let caller = WorkflowActor::authenticated(owner).unwrap();
        let company_channels = || async {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM channels WHERE company_id = $1")
                .bind(company)
                .fetch_one(persistence.pool())
                .await
                .unwrap()
        };

        // Something else already answers on "taken".
        channels
            .create_assistant_channel(
                CompanyId::new(company),
                caller,
                AssistantChannelRequest {
                    agent_slug: "desk".into(),
                    channel_slug: "taken".into(),
                    channel_name: "Taken".into(),
                    reply_mode: ReplyMode::Autonomous,
                    command_key: "elsewhere".into(),
                },
            )
            .await
            .unwrap()
            .unwrap();
        let before = company_channels().await;

        let first = onboarding_channel(&channels, company, caller, &desk)
            .await
            .unwrap();
        let pages::OnboardingChannel::Created {
            channel_id,
            address,
        } = first.clone()
        else {
            panic!("the desk agent gets its channel: {first:?}");
        };
        assert_eq!(address, format!("desk@{suffix}.agents.test"));
        let (slug, name, access): (String, String, String) = sqlx::query_as(
            "SELECT slug.slug::text, channel.name, channel.access_mode FROM channels AS channel \
             JOIN channel_slugs AS slug ON slug.channel_id = channel.id AND slug.is_primary \
             WHERE channel.id = $1 AND channel.company_id = $2",
        )
        .bind(channel_id)
        .bind(company)
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(
            (slug.as_str(), name.as_str(), access.as_str()),
            ("desk", "Agent desk", "team")
        );
        assert_eq!(company_channels().await, before + 1);

        // A replay of the step for the agents that already exist adds nothing.
        let replayed = onboarding_channel(&channels, company, caller, &desk)
            .await
            .unwrap();
        assert_eq!(replayed, first);
        assert_eq!(company_channels().await, before + 1);

        assert_eq!(
            onboarding_channel(&channels, company, caller, &drafts)
                .await
                .unwrap(),
            pages::OnboardingChannel::NoModel
        );
        let refused = onboarding_channel(&channels, company, caller, &taken)
            .await
            .unwrap();
        assert!(
            matches!(&refused, pages::OnboardingChannel::Refused(message) if message.contains("'taken'")),
            "{refused:?}"
        );
        assert_eq!(company_channels().await, before + 1);

        // A reload reads back what was stored; a refusal was not.
        assert_eq!(
            stored_onboarding_channel(&channels, company, caller, &desk)
                .await
                .unwrap(),
            first
        );
        assert_eq!(
            stored_onboarding_channel(&channels, company, caller, &taken)
                .await
                .unwrap(),
            pages::OnboardingChannel::Missing
        );
        assert_eq!(
            stored_onboarding_channel(&channels, company, caller, &drafts)
                .await
                .unwrap(),
            pages::OnboardingChannel::NoModel
        );
        // Someone who does not manage the company reads nothing.
        assert_eq!(
            stored_onboarding_channel(
                &channels,
                company,
                WorkflowActor::authenticated(outsider).unwrap(),
                &desk
            )
            .await
            .unwrap(),
            pages::OnboardingChannel::Missing
        );
    }
}
