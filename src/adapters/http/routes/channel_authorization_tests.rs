use super::super::authorization_test_support::{Fixture, ScopeCase, denied};
use super::simulation::{OpenSimulatedThreadContext, open_simulated_thread_logic};
use super::*;
use axum::http::StatusCode;

#[path = "delegated_channel_authorization_tests.rs"]
mod delegated;

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
    use crate::application::ports::thread::ThreadPersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let other_channel = crate::adapters::persistence::test_support::create_channel(
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
        .create_thread(
            f.company.id,
            crate::entities::address::AddressTarget::Desk(other_channel.id),
            "Other channel secret",
            &[],
        )
        .await
        .unwrap();
    let viewer = Fixture::viewer(&f.owner);
    assert!(
        f.threads
            .readable_thread(
                &viewer,
                f.company.id,
                crate::entities::address::AddressTarget::Desk(f.channel),
                f.thread
            )
            .await
            .unwrap()
            .is_some()
    );
    let mut bodies = Vec::new();
    for id in [Uuid::new_v4(), f.foreign_thread, other_thread.id] {
        assert!(
            f.threads
                .readable_thread(
                    &viewer,
                    f.company.id,
                    crate::entities::address::AddressTarget::Desk(f.channel),
                    id
                )
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

#[tokio::test]
async fn delegated_admin_routes_desk_pages_list_only_managed_desks_and_hide_the_rest() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let d = f.delegation().await;
    let page = |user: &crate::entities::user::User| {
        list_channels_page(
            State(f.companies.clone()),
            State(f.channels.clone()),
            State(f.departments()),
            State(f.config.clone()),
            Fixture::auth(user),
            Path(f.company.id),
            Query(ChannelsPageQuery::default()),
        )
    };
    let lead = page(&d.lead).await.expect("the lead enters").0;
    assert!(lead.contains("Managed desk"), "{lead}");
    assert!(
        !lead.contains("Private channel"),
        "a root desk is not the lead's"
    );
    let sibling = page(&d.sibling).await.expect("the sibling lead enters").0;
    assert!(!sibling.contains("Managed desk"), "{sibling}");
    let owner = page(&f.owner).await.unwrap().0;
    assert!(owner.contains("Managed desk") && owner.contains("Private channel"));
    let get = |user: &crate::entities::user::User| {
        managed_channel(&f.companies, &f.channels, user.id, f.company.id, d.desk)
    };
    assert_eq!(get(&d.lead).await.unwrap().1.id, d.desk);
    denied(
        get(&d.sibling).await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(get(&f.member).await, StatusCode::NOT_FOUND, &f.foreign.name).await;
}

/// Plan06 U6: the desk workspace lists the desks a lead manages, then the ones they may only read;
/// a sibling's desk appears in neither.
#[tokio::test]
async fn delegated_admin_routes_desk_workspace_lists_managed_then_readable_only_desks() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let d = f.delegation().await;
    let rows = f
        .channels
        .list_workspace_channels(&Fixture::viewer(&d.lead), f.company.id)
        .await
        .unwrap();
    let managed: Vec<_> = rows.iter().filter(|row| row.managed).collect();
    let readable: Vec<_> = rows.iter().filter(|row| !row.managed).collect();
    assert!(managed.iter().any(|row| row.channel.id == d.desk));
    assert!(
        managed
            .iter()
            .all(|row| row.channel.department_id == d.unit)
    );
    assert!(
        !readable.is_empty(),
        "the lead reads the root's members desk"
    );
    assert!(
        readable
            .iter()
            .all(|row| row.channel.department_id != d.unit)
    );
    assert!(
        rows.iter()
            .all(|row| row.channel.department_id != d.sibling_unit),
        "a sibling's desk is neither managed nor readable"
    );
    let first_readable = rows.iter().position(|row| !row.managed).unwrap();
    assert!(
        rows[first_readable..].iter().all(|row| !row.managed),
        "managed rows come first"
    );
    let readable_ids: std::collections::HashSet<Uuid> = f
        .channels
        .list_readable_channels(&Fixture::viewer(&d.lead), f.company.id)
        .await
        .unwrap()
        .into_iter()
        .map(|channel| channel.id)
        .collect();
    assert!(
        readable
            .iter()
            .all(|row| readable_ids.contains(&row.channel.id))
    );
    assert!(matches!(
        f.channels
            .list_workspace_channels(&Fixture::viewer(&f.member), f.company.id)
            .await,
        Err(AppError::NotFound(_))
    ));
}
