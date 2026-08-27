use crate::use_cases::agent::AgentWrite;
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
    entities::value_objects::EmailAddress,
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases,
        channel::{ChannelUseCases, ChannelWrite},
        company::{CompanyUseCases, CompanyWrite},
        user::UserUseCases,
    },
};

use super::{
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
            get(channel_step).post(create_channel),
        )
        .route(
            "/ui/onboarding/companies/{company_id}/channels/{channel_id}/complete",
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

    match company_use_cases
        .create_company(
            user.id,
            CompanyWrite {
                name: form.name.clone(),
                slug: form.slug.clone(),
                api_key: form.api_key.clone(),
                provider: form.provider.clone(),
                model: form.model.clone(),
                ..CompanyWrite::default()
            },
        )
        .await
    {
        Ok(company) => Ok(Redirect::to(&format!(
            "/ui/onboarding/companies/{}/channel",
            company.id
        ))
        .into_response()),
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

async fn create_channel(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
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
    let mut selected_ids = HashSet::new();
    let selected_library_agents = submitted_ids
        .iter()
        .filter(|id| selected_ids.insert(**id))
        .map(|id| library_agents.iter().find(|agent| agent.id == *id))
        .collect::<Option<Vec<_>>>();
    let Some(selected_library_agents) = selected_library_agents else {
        return Ok(Html(pages::onboarding_channel_page(
            &mailbox_user,
            &company,
            &library_agents,
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
        return Ok(Html(pages::onboarding_channel_page(
            &mailbox_user,
            &company,
            &library_agents,
            Some("Select at least one library agent or fill in the custom agent form."),
        ))
        .into_response());
    }
    if custom_name.is_empty() != custom_instructions.is_empty() {
        return Ok(Html(pages::onboarding_channel_page(
            &mailbox_user,
            &company,
            &library_agents,
            Some("A custom agent needs both a channel name and instructions."),
        ))
        .into_response());
    }

    let mut channel_specs = selected_library_agents
        .iter()
        .map(|agent| (agent.name.clone(), agent.slug.clone(), agent.id))
        .collect::<Vec<_>>();
    let mut custom_agent_id = None;

    if !custom_name.is_empty() {
        let system_prompt = match agent_use_cases
            .generate_system_prompt(user.id, company_id, custom_instructions, None, None, None)
            .await
        {
            Ok(prompt) => prompt,
            Err(err) => {
                return Ok(Html(pages::onboarding_channel_page(
                    &mailbox_user,
                    &company,
                    &library_agents,
                    Some(&format!("Could not prepare the agent instructions: {err}")),
                ))
                .into_response());
            }
        };
        let slug = slugify(custom_name);
        let agent = match agent_use_cases
            .create_agent(
                user.id,
                company_id,
                AgentWrite {
                    name: custom_name.to_string(),
                    slug: slug.clone(),
                    system_prompt: Some(system_prompt.clone()),
                    ..AgentWrite::default()
                },
            )
            .await
        {
            Ok(agent) => agent,
            Err(err) => {
                return Ok(Html(pages::onboarding_channel_page(
                    &mailbox_user,
                    &company,
                    &library_agents,
                    Some(&format!("Could not create the agent: {err}")),
                ))
                .into_response());
            }
        };
        custom_agent_id = Some(agent.id);
        channel_specs.push((custom_name.to_string(), slug, agent.id));
    }

    let mut created_channels = Vec::new();
    for (name, slug, agent_id) in channel_specs {
        let write = ChannelWrite {
            name,
            slug,
            agent_ids: Some(vec![agent_id]),
            enabled: true,
            add_3rd_party: true,
            ..ChannelWrite::default()
        };
        match channel_use_cases
            .create_channel(user.id, company_id, write, false)
            .await
        {
            Ok(channel) => created_channels.push(channel),
            Err(err) => {
                for channel in created_channels {
                    let _ = channel_use_cases
                        .delete_channel(user.id, company_id, channel.id)
                        .await;
                }
                if let Some(agent_id) = custom_agent_id {
                    let _ = agent_use_cases
                        .delete_agent(user.id, company_id, agent_id)
                        .await;
                }
                return Ok(Html(pages::onboarding_channel_page(
                    &mailbox_user,
                    &company,
                    &library_agents,
                    Some(&format!("Could not create the channels: {err}")),
                ))
                .into_response());
            }
        }
    }
    let channel = created_channels
        .first()
        .expect("at least one channel was requested");
    Ok(Redirect::to(&format!(
        "/ui/onboarding/companies/{company_id}/channels/{}/complete",
        channel.id
    ))
    .into_response())
}

async fn complete(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    State(user_use_cases): State<Arc<UserUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    let account = load_account(&user_use_cases, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    let company = company_use_cases.get_company(company_id).await?;
    let Some(company) = company.filter(|company| company.user_id == user.id) else {
        return Ok(Redirect::to("/ui").into_response());
    };
    let channel = channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await?;
    let Some(channel) = channel else {
        return Ok(Redirect::to("/ui").into_response());
    };

    Ok(Html(pages::onboarding_complete_page(
        &mailbox_user,
        &company,
        &channel,
        &config.app_domain_name,
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
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".to_string(),
            slug: "acme".into(),
            api_key: None,
            provider: None,
            model: None,
            enable_llm_spam_guardrail: None,
            avatar_url: None,
            memory_provider: None,
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
            api_key: None,
            system_prompt: Some("Help with email.".to_string()),
            description: Some("A ready-made helper".to_string()),
            config_json: None,
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
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            retrieve_company_memory: false,
            retrieve_agent_memory: false,
            retrieve_user_memory: false,
            persist_company_memory: false,
            persist_agent_memory: false,
            persist_user_memory: false,
            memory_recall_mode: crate::entities::memory::MemoryRecallMode::Fast,
            memory_max_results: 5,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

        let company_page = pages::onboarding_company_page(&user, "example.com", None);
        assert!(company_page.contains("Step 1 of 3"));
        assert!(company_page.contains("action=\"/ui/onboarding/company\""));
        assert!(company_page.contains("href=\"/ui\" class=\"btn btn-ghost\">Skip</a>"));
        assert!(company_page.contains(">.example.com</span>"));
        assert!(company_page.contains("/assets/app.css"));
        assert!(company_page.contains("/ui/profile"));

        let library_agent = library_agent("Scheduler", "scheduler");
        let channel_page =
            pages::onboarding_channel_page(&user, &company, &[library_agent.clone()], None);
        assert!(channel_page.contains("Step 2 of 3"));
        assert!(channel_page.contains(&format!(
            "action=\"/ui/onboarding/companies/{}/channel\"",
            company.id
        )));
        assert!(channel_page.contains("name=\"library_agent_ids\""));
        assert!(channel_page.contains(&format!("value=\"{}\"", library_agent.id)));
        assert!(channel_page.contains("Scheduler"));
        assert!(channel_page.contains("Each agent gets a channel with the same name"));
        assert!(channel_page.contains("this.setAttribute('aria-busy', 'true')"));
        assert!(channel_page.contains("loading loading-spinner loading-sm hidden"));
        assert!(channel_page.contains("Creating email agents…"));

        let complete_page =
            pages::onboarding_complete_page(&user, &company, &channel, "example.com");
        assert!(complete_page.contains("Step 3 of 3"));
        assert!(complete_page.contains("customer-support@acme.example.com"));
        assert!(complete_page.contains("Forward a message"));
        assert!(complete_page.contains("Reply in the thread"));
        assert!(complete_page.contains(&format!(
            "href=\"/ui?company_id={}&channel_id={}\" class=\"btn btn-primary\">Finish</a>",
            company.id, channel.id
        )));
    }
}
