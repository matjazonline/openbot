use super::super::authorization_test_support::{Fixture, ScopeCase, denied};
use super::simulation::{OpenSimulatedThreadContext, open_simulated_thread_logic};
use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn channel_management_and_simulation_hide_foreign_companies_and_refuse_members() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for ScopeCase {
        companies,
        user,
        company_id: company,
        status,
        ..
    } in f.denial_cases()
    {
        let channel = if company == f.foreign.id {
            f.foreign_channel
        } else {
            f.channel
        };
        denied(
            list_channels_page(
                State(companies.clone()),
                State(f.channels.clone()),
                State(f.departments()),
                State(f.config.clone()),
                Fixture::auth(user),
                Path(company),
                Query(ChannelsPageQuery::default()),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
        denied(
            simulate_channel_page(
                State(companies),
                State(f.channels.clone()),
                State(f.threads.clone()),
                State(f.persistence.clone() as Arc<dyn MessageOutcomeReader>),
                State(f.config.clone()),
                Fixture::viewer(user),
                Path((company, channel)),
                Query(SimulateQuery { thread_id: None }),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
    }
}

#[tokio::test]
async fn simulation_and_application_thread_reads_hide_other_channels_and_tenants_like_unknown_ids()
{
    use crate::application::ports::channel::ChannelPersistence;
    use crate::application::ports::thread::ThreadPersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let other_channel = ChannelPersistence::create(
        f.persistence.as_ref(),
        f.company.id,
        ChannelWrite {
            name: "Other".into(),
            slug: "other".into(),
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let other_thread = f
        .persistence
        .create_thread(other_channel.id, "Other channel secret", &[])
        .await
        .unwrap();
    let viewer = Fixture::viewer(&f.owner);
    assert!(
        f.threads
            .readable_thread(&viewer, f.company.id, f.channel, f.thread)
            .await
            .unwrap()
            .is_some()
    );
    let mut bodies = Vec::new();
    for id in [Uuid::new_v4(), f.foreign_thread, other_thread.id] {
        assert!(
            f.threads
                .readable_thread(&viewer, f.company.id, f.channel, id)
                .await
                .unwrap()
                .is_none()
        );
        bodies.push(
            denied(
                open_simulated_thread_logic(
                    OpenSimulatedThreadContext {
                        company_use_cases: f.companies.clone(),
                        channel_use_cases: f.channels.clone(),
                        thread_use_cases: f.threads.clone(),
                        outcome_reader: f.persistence.clone(),
                        config: f.config.clone(),
                        viewer: viewer.clone(),
                        company_id: f.company.id,
                        channel_id: f.channel,
                    },
                    &id.to_string(),
                )
                .await,
                StatusCode::NOT_FOUND,
                &f.foreign.name,
            )
            .await,
        );
    }
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
}
