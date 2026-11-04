use super::super::authorization_test_support::{Fixture, ScopeCase, denied};
use super::*;

#[tokio::test]
async fn onboarding_page_hides_foreign_companies_and_refuses_members() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for ScopeCase {
        companies,
        user,
        company_id: id,
        status,
        ..
    } in f.denial_cases()
    {
        denied(
            channel_step(
                State(companies),
                State(f.agents.clone()),
                State(f.users.clone()),
                State(f.config.clone()),
                Fixture::auth(user),
                Path(id),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
    }
}

#[tokio::test]
async fn onboarding_submission_key_is_required_and_denials_propagate() {
    use crate::adapters::{
        persistence::{
            onboarding::PostgresOnboarding, workflow::provisioning::ProvisioningDeployment,
        },
        workflow_source::WorkflowSourceDecoder,
    };
    use crate::application::workflow::templates::TemplateCatalogue;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let onboarding = Arc::new(OnboardingUseCases::new(
        f.persistence.clone(),
        f.persistence.clone(),
        Arc::new(PostgresOnboarding::new(
            f.persistence.as_ref().clone(),
            ProvisioningDeployment::new(
                Arc::new(TemplateCatalogue::shipped(&WorkflowSourceDecoder).unwrap()),
                &[],
                "agents.test",
            ),
        )),
        f.agents.clone(),
    ));
    for key in [None, Some("malformed".into())] {
        let form = OnboardingChannelForm {
            name: String::new(),
            instructions: String::new(),
            library_agent_ids: None,
            submission_key: key,
        };
        assert!(matches!(form.submission(), Err(AppError::BadRequest(_))));
    }
    for ScopeCase {
        companies,
        user,
        company_id,
        status,
        ..
    } in f.denial_cases()
    {
        denied(
            create_agents(
                State(onboarding.clone()),
                State(companies),
                State(f.agents.clone()),
                State(f.users.clone()),
                State(f.config.clone()),
                Fixture::auth(user),
                Path(company_id),
                Form(OnboardingChannelForm {
                    name: "Custom".into(),
                    instructions: "Help customers".into(),
                    library_agent_ids: None,
                    submission_key: Some(Uuid::new_v4().to_string()),
                }),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
    }
}
