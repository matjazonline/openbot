use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn agent_pages_hide_foreign_names_refuse_members_and_propagate_database_errors() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let foreign = denied(
        list_agents_page(
            State(f.companies.clone()),
            State(f.agents.clone()),
            Fixture::auth(&f.owner),
            Path(f.foreign.id),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    let unknown = denied(
        list_agents_page(
            State(f.companies.clone()),
            State(f.agents.clone()),
            Fixture::auth(&f.owner),
            Path(Uuid::new_v4()),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    assert_eq!(foreign, unknown);
    denied(
        list_agents_page(
            State(f.companies.clone()),
            State(f.agents.clone()),
            Fixture::auth(&f.member),
            Path(f.company.id),
        )
        .await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    denied(
        list_agents_page(
            State(f.failing_companies()),
            State(f.agents.clone()),
            Fixture::auth(&f.owner),
            Path(f.company.id),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.company.name,
    )
    .await;
}
