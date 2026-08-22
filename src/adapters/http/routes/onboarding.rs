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
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases,
        channel::{ChannelUseCases, ChannelWrite},
        company::{CompanyUseCases, CompanyWrite},
    },
};

use super::channel::slugify;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/onboarding", get(start))
        .route("/onboarding/company", post(create_company))
        .route(
            "/onboarding/companies/{company_id}/channel",
            get(channel_step).post(create_channel),
        )
        .route(
            "/onboarding/companies/{company_id}/channels/{channel_id}/complete",
            get(complete),
        )
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
    user: AuthenticatedUser,
) -> Response {
    match company_use_cases.list_user_companies(user.id).await {
        Ok(companies) if companies.is_empty() => {
            Html(pages::onboarding_company_page(None)).into_response()
        }
        Ok(_) => Redirect::to("/companies").into_response(),
        Err(err) => Html(pages::onboarding_company_page(Some(&format!(
            "Could not load your companies: {err}"
        ))))
        .into_response(),
    }
}

async fn create_company(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Form(form): Form<OnboardingCompanyForm>,
) -> Response {
    if company_use_cases
        .list_user_companies(user.id)
        .await
        .is_ok_and(|companies| !companies.is_empty())
    {
        return Redirect::to("/companies").into_response();
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
        Ok(company) => {
            Redirect::to(&format!("/onboarding/companies/{}/channel", company.id)).into_response()
        }
        Err(err) => Html(pages::onboarding_company_page(Some(&format!(
            "Could not create the company: {err}"
        ))))
        .into_response(),
    }
}

async fn channel_step(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
) -> Response {
    match company_use_cases.get_company(company_id).await {
        Ok(Some(company)) if company.user_id == user.id => {
            Html(pages::onboarding_channel_page(&company, None)).into_response()
        }
        _ => Redirect::to("/companies").into_response(),
    }
}

async fn create_channel(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(agent_use_cases): State<Arc<AgentUseCases>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<OnboardingChannelForm>,
) -> Response {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(company)) if company.user_id == user.id => company,
        _ => return Redirect::to("/companies").into_response(),
    };
    let slug = slugify(&form.name);

    let system_prompt = match agent_use_cases
        .generate_system_prompt(user.id, company_id, &form.instructions, None, None, None)
        .await
    {
        Ok(prompt) => prompt,
        Err(err) => {
            return Html(pages::onboarding_channel_page(
                &company,
                Some(&format!("Could not prepare the agent instructions: {err}")),
            ))
            .into_response();
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
            return Html(pages::onboarding_channel_page(
                &company,
                Some(&format!("Could not create the agent: {err}")),
            ))
            .into_response();
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
        Ok(channel) => Redirect::to(&format!(
            "/onboarding/companies/{company_id}/channels/{}/complete",
            channel.id
        ))
        .into_response(),
        Err(err) => {
            let _ = agent_use_cases
                .delete_agent(user.id, company_id, agent.id)
                .await;
            Html(pages::onboarding_channel_page(
                &company,
                Some(&format!("Could not create the channel: {err}")),
            ))
            .into_response()
        }
    }
}

async fn complete(
    State(company_use_cases): State<Arc<CompanyUseCases>>,
    State(channel_use_cases): State<Arc<ChannelUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let company = match company_use_cases.get_company(company_id).await {
        Ok(Some(company)) if company.user_id == user.id => company,
        _ => return Redirect::to("/companies").into_response(),
    };
    let channel = match channel_use_cases
        .get_company_channel(user.id, company_id, channel_id)
        .await
    {
        Ok(Some(channel)) => channel,
        _ => return Redirect::to("/companies").into_response(),
    };

    Html(pages::onboarding_complete_page(
        &company,
        &channel,
        &config.app_domain_name,
    ))
    .into_response()
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
            created_at: Utc::now(),
        }
    }

    #[test]
    fn onboarding_pages_render_each_step() {
        let company = company();
        let channel = Channel {
            enabled: true,
            add_3rd_party: true,
            id: Uuid::new_v4(),
            company_id: company.id,
            name: "Customer Support".to_string(),
            slug: "customer-support".into(),
            alias_slugs: Vec::new(),
            api_key: None,
            provider: None,
            model: None,
            participant_emails: None,
            agent_ids: None,
            channel_config: None,
            created_by: crate::entities::creation::CreationProvenance::system(),
            created_at: Utc::now(),
        };

        let company_page = pages::onboarding_company_page(None);
        assert!(company_page.contains("Setup 1 of 3"));
        assert!(company_page.contains("action=\"/onboarding/company\""));

        let channel_page = pages::onboarding_channel_page(&company, None);
        assert!(channel_page.contains("Setup 2 of 3"));
        assert!(channel_page.contains(&format!(
            "action=\"/onboarding/companies/{}/channel\"",
            company.id
        )));

        let complete_page = pages::onboarding_complete_page(&company, &channel, "example.com");
        assert!(complete_page.contains("Setup 3 of 3"));
        assert!(complete_page.contains("customer-support@acme.example.com"));
        assert!(complete_page.contains("Forward a message"));
        assert!(complete_page.contains("Reply in the thread"));
    }
}
