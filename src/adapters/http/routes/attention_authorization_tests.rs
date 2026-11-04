use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

#[tokio::test]
async fn attention_application_gating_refuses_team_work_and_hides_foreign_tenants() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let query = |view: &str| {
        Query(ListQuery {
            view: Some(view.into()),
            cursor: None,
            limit: None,
        })
    };
    denied(
        list_attention(
            State(f.attention.clone()),
            Fixture::viewer(&f.owner),
            Path(f.foreign.id),
            query("my_work"),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(
        list_attention(
            State(f.attention.clone()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
            query("team_work"),
        )
        .await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    denied(
        operational_summary(
            State(f.attention.clone()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
        )
        .await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    assert!(
        list_attention(
            State(f.attention.clone()),
            Fixture::viewer(&f.member),
            Path(f.company.id),
            query("my_work")
        )
        .await
        .is_ok()
    );
    let failing = Arc::new(AttentionUseCases::new(
        f.persistence.clone(),
        Arc::new(crate::use_cases::company_test_support::MockCompanyPersistence::failing()),
        f.persistence.clone(),
        f.persistence.clone(),
        Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
    ));
    denied(
        list_attention(
            State(failing),
            Fixture::viewer(&f.owner),
            Path(f.company.id),
            query("my_work"),
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        &f.company.name,
    )
    .await;
}
