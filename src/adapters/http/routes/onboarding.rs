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
    app_error::{AppError, AppResult},
    application::workflow::{WorkflowActor, provisioning::AssistantChannels},
    domain::workflow::CompanyId,
    entities::{agent::Agent, value_objects::EmailAddress},
    infra::config::AppConfig,
    use_cases::{
        agent::AgentUseCases,
        company::{CompanyModelConnectionWrite, CompanyUseCases, CompanyWrite},
        onboarding::{OnboardingOutcome, OnboardingSubmission, OnboardingUseCases},
        user::UserUseCases,
    },
};

use super::ui::{load_account, workspace_user};

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
    submission_key: Option<String>,
    /// The company's first department, when the owner names one (03C item 0).
    #[serde(default)]
    department_name: Option<String>,
}

impl OnboardingChannelForm {
    fn submission(&self) -> AppResult<OnboardingSubmission> {
        let submission_key = self
            .submission_key
            .as_deref()
            .and_then(|key| Uuid::parse_str(key).ok())
            .ok_or_else(|| {
                AppError::BadRequest("A valid onboarding submission key is required.".into())
            })?;
        let library_agent_ids = self
            .library_agent_ids
            .as_deref()
            .unwrap_or_default()
            .split([',', ' ', ';', '\n'])
            .filter(|id| !id.is_empty())
            .map(|id| {
                Uuid::parse_str(id)
                    .map_err(|_| AppError::BadRequest("Invalid library agent selection.".into()))
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok(OnboardingSubmission {
            submission_key,
            library_agent_ids,
            name: self.name.clone(),
            instructions: self.instructions.clone(),
            department_name: self.department_name.clone(),
        })
    }
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
    let company = company_use_cases.owned_company(user.id, company_id).await?;
    let library_agents = agent_use_cases.list_library_agents().await?;
    Ok(Html(pages::onboarding_channel_page(
        &mailbox_user,
        &company,
        &library_agents,
        None,
    ))
    .into_response())
}

/// Step 2: the chosen agents, each created with its default desk and pinned to the company default
/// model, and the named department with the first agent in it (01DEP-P03-D4). A refusal of any
/// of it commits nothing and is shown on the form.
#[allow(clippy::too_many_arguments)] // Typed Axum extractors name each application dependency.
async fn create_agents(
    State(onboarding): State<Arc<OnboardingUseCases>>,
    State(companies): State<Arc<CompanyUseCases>>,
    State(agents): State<Arc<AgentUseCases>>,
    State(users): State<Arc<UserUseCases>>,
    State(config): State<Arc<AppConfig>>,
    user: AuthenticatedUser,
    Path(company_id): Path<Uuid>,
    Form(form): Form<OnboardingChannelForm>,
) -> AppResult<Response> {
    let submission = form.submission()?;
    let submission_key = submission.submission_key;
    let submitted_ids = submission.library_agent_ids.clone();
    let account = load_account(&users, user.id).await?;
    let email = EmailAddress::from(account.email.as_str());
    let mailbox_user = workspace_user(&account, &email, &config);
    let outcome = onboarding.onboard(user.id, company_id, submission).await?;
    let company = companies.owned_company(user.id, company_id).await?;
    let html = match outcome {
        OnboardingOutcome::Rejected(message) => pages::onboarding_channel_draft_page(
            &mailbox_user,
            &company,
            &agents.list_library_agents().await?,
            &pages::OnboardingAgentDraft {
                name: &form.name,
                instructions: &form.instructions,
                library_ids: &submitted_ids,
                submission_key,
            },
            Some(&message),
        ),
        OnboardingOutcome::Completed(created) => {
            let onboarded = created
                .iter()
                .map(|created| pages::OnboardedAgent {
                    agent: &created.agent,
                    channel: created_channel(&created.channel),
                })
                .collect::<Vec<_>>();
            pages::onboarding_complete_page(&mailbox_user, &company, &onboarded)
        }
    };
    Ok(Html(html).into_response())
}

/// What the completion page shows for an agent on a reload: the default desk its creation
/// recorded (01DEP-P03-D4).
async fn stored_onboarding_channel(
    channels: &dyn AssistantChannels,
    company_id: Uuid,
    caller: WorkflowActor,
    agent: &Agent,
) -> AppResult<pages::OnboardingChannel> {
    let stored = channels
        .assistant_channel_result(CompanyId::new(company_id), caller, &onboarding_key(agent))
        .await?;
    Ok(stored.map_or(pages::OnboardingChannel::Missing, |created| {
        created_channel(&created)
    }))
}

/// The command key that records an agent's default desk, which the completion page reads back.
fn onboarding_key(agent: &Agent) -> String {
    crate::adapters::persistence::company_agent::default_desk_key(agent.id)
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
    let company = company_use_cases.owned_company(user.id, company_id).await?;
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
            draft_revision: crate::entities::agent_capability::AgentDraftRevision::new(1).unwrap(),
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
            department_id: None,
            members_only: false,
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
                submission_key: Uuid::new_v4(),
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
        assert!(channel_page.contains("Each agent gets a desk with the same name"));
        assert!(channel_page.contains("Custom agent and desk name"));
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

        // Each agent with its desk, or where to make one when none is recorded.
        let scheduler = library_agent.clone();
        let triage = super::tests::library_agent("Triage", "triage");
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
                    channel: pages::OnboardingChannel::Missing,
                },
            ],
        );
        assert!(complete_page.contains("Step 3 of 3"));
        assert!(complete_page.contains("Your email agents are ready"));
        assert!(complete_page.contains("scheduler@acme.example.com"));
        assert!(complete_page.contains("Forward a message"));
        assert!(complete_page.contains(&format!(
            "href=\"/ui/agents?company_id={}&agent_id={}\"",
            company.id, triage.id
        )));
        assert!(complete_page.contains(&format!(
            "href=\"/ui?company_id={}&channel_id={}\" class=\"btn btn-primary\">Finish</a>",
            company.id, channel.id
        )));

        let no_channels = pages::onboarding_complete_page(
            &user,
            &company,
            &[pages::OnboardedAgent {
                agent: &triage,
                channel: pages::OnboardingChannel::Missing,
            }],
        );
        assert!(no_channels.contains("Your agents are created"));
        assert!(!no_channels.contains("Forward a message"));
        assert!(no_channels.contains(&format!(
            "href=\"/ui?company_id={}\" class=\"btn btn-primary\">Finish</a>",
            company.id
        )));
    }

    /// 01DEP-P03-D4: the completion page reads back the default desk an agent's creation recorded
    /// under `agent:{id}`, for the company's managers only; an agent with no recorded desk (a
    /// fixture insert) shows none.
    #[tokio::test]
    async fn agent_default_desk_onboarding_completion_reads_back_the_default_desk() {
        use crate::adapters::persistence::company_agent::CompanyAgentService;
        use crate::adapters::persistence::workflow::provisioning::{
            AssistantChannelService, ProvisioningDeployment,
        };
        use crate::adapters::persistence::{PostgresPersistence, test_support::test_pool};
        use crate::adapters::workflow_source::WorkflowSourceDecoder;
        use crate::application::workflow::templates::TemplateCatalogue;
        use crate::use_cases::{
            agent::{AgentPersistence, AgentWrite, CompanyAgents},
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
        let templates = ProvisioningDeployment::new(
            Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
            &[],
            "agents.test",
        );
        let caller = WorkflowActor::authenticated(owner).unwrap();
        let created = CompanyAgentService::new(persistence.clone(), templates.clone())
            .create(
                caller,
                CompanyId::new(company),
                AgentWrite {
                    name: "Agent desk".into(),
                    slug: "desk".into(),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        let bare = AgentPersistence::create(
            &persistence,
            company,
            AgentWrite {
                name: "Bare".into(),
                slug: "bare".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let channels = AssistantChannelService::new(persistence.clone(), templates);

        let read = stored_onboarding_channel(&channels, company, caller, &created.agent)
            .await
            .unwrap();
        assert_eq!(
            read,
            pages::OnboardingChannel::Created {
                channel_id: created.desk_id,
                address: format!("desk@{suffix}.agents.test"),
            }
        );
        assert_eq!(
            stored_onboarding_channel(&channels, company, caller, &bare)
                .await
                .unwrap(),
            pages::OnboardingChannel::Missing
        );
        // Someone who does not manage the company reads nothing.
        assert_eq!(
            stored_onboarding_channel(
                &channels,
                company,
                WorkflowActor::authenticated(outsider).unwrap(),
                &created.agent
            )
            .await
            .unwrap(),
            pages::OnboardingChannel::Missing
        );
    }
}

#[cfg(test)]
#[path = "onboarding_authorization_tests.rs"]
mod authorization_tests;
