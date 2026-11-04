use super::super::authorization_test_support::{Fixture, ScopeCase, denied};
use super::*;

#[tokio::test]
async fn company_pages_and_writes_distinguish_foreign_member_and_database_failure() {
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
            edit_company_form(State(companies.clone()), Fixture::auth(user), Path(id)).await,
            status,
            &f.foreign.name,
        )
        .await;
        denied(
            cancel_company_edit(State(companies.clone()), Fixture::auth(user), Path(id)).await,
            status,
            &f.foreign.name,
        )
        .await;
        let form = || {
            serde_json::from_value(serde_json::json!({"name":"Changed","slug":"changed"})).unwrap()
        };
        denied(
            update_company(
                State(companies.clone()),
                Fixture::auth(user),
                Path(id),
                Form(form()),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
        denied(
            update_company_json(
                State(companies.clone()),
                Fixture::auth(user),
                Path(id),
                Json(form()),
            )
            .await,
            status,
            &f.foreign.name,
        )
        .await;
        denied(
            delete_company(State(companies.clone()), Fixture::auth(user), Path(id)).await,
            status,
            &f.foreign.name,
        )
        .await;
        denied(
            delete_company_json(State(companies), Fixture::auth(user), Path(id)).await,
            status,
            &f.foreign.name,
        )
        .await;
    }
}
