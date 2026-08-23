use crate::use_cases::agent::AgentWrite;
use std::sync::Arc;

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
    name: String,
    instructions: String,
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
        Ok(companies) if companies.is_empty() => {
            Ok(Html(pages::onboarding_company_page(&mailbox_user, None)).into_response())
        }
        Ok(_) => Ok(Redirect::to("/ui").into_response()),
        Err(err) => Ok(Html(pages::onboarding_company_page(
            &mailbox_user,
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
            Some(&format!("Could not create the company: {err}")),
        ))
        .into_response()),
    }
}

async fn channel_step(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
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
    Ok(Html(pages::onboarding_channel_page(
        &mailbox_user,
        &company,
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
    let slug = slugify(&form.name);

    let system_prompt = match agent_use_cases
        .generate_system_prompt(user.id, company_id, &form.instructions, None, None, None)
        .await
    {
        Ok(prompt) => prompt,
        Err(err) => {
            return Ok(Html(pages::onboarding_channel_page(
                &mailbox_user,
                &company,
                Some(&format!("Could not prepare the agent instructions: {err}")),
            ))
            .into_response());
        }
    };

    let agent = match agent_use_cases
        .create_agent(
            user.id,
            company_id,
            AgentWrite {
                name: form.name.clone(),
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
                Some(&format!("Could not create the agent: {err}")),
            ))
            .into_response());
        }
    };

    let write = ChannelWrite {
        name: form.name.clone(),
        slug,
        agent_ids: Some(vec![agent.id]),
        enabled: true,
        add_3rd_party: true,
        ..ChannelWrite::default()
    };

    match channel_use_cases
        .create_channel(user.id, company_id, write, false)
        .await
    {
        Ok(channel) => Ok(Redirect::to(&format!(
            "/ui/onboarding/companies/{company_id}/channels/{}/complete",
            channel.id
        ))
        .into_response()),
        Err(err) => {
            let _ = agent_use_cases
                .delete_agent(user.id, company_id, agent.id)
                .await;
            Ok(Html(pages::onboarding_channel_page(
                &mailbox_user,
                &company,
                Some(&format!("Could not create the channel: {err}")),
            ))
            .into_response())
        }
    }
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
    use crate::entities::{channel::Channel, company::Company};

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

        let company_page = pages::onboarding_company_page(&user, None);
        assert!(company_page.contains("Step 1 of 3"));
        assert!(company_page.contains("action=\"/ui/onboarding/company\""));
        assert!(company_page.contains("daisyui"));
        assert!(company_page.contains("/ui/profile"));

        let channel_page = pages::onboarding_channel_page(&user, &company, None);
        assert!(channel_page.contains("Step 2 of 3"));
        assert!(channel_page.contains(&format!(
            "action=\"/ui/onboarding/companies/{}/channel\"",
            company.id
        )));

        let complete_page =
            pages::onboarding_complete_page(&user, &company, &channel, "example.com");
        assert!(complete_page.contains("Step 3 of 3"));
        assert!(complete_page.contains("customer-support@acme.example.com"));
        assert!(complete_page.contains("Forward a message"));
        assert!(complete_page.contains("Reply in the thread"));
    }
}
