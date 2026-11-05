use super::super::authorization_test_support::{Fixture, ScopeCase, denied};
use super::*;

#[tokio::test]
async fn invite_pages_and_fragments_hide_foreign_companies_and_propagate_failures() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let invite = f
        .persistence
        .create_invite(
            crate::adapters::persistence::test_support::invite_command(
                &f.persistence,
                f.foreign.id,
                &format!("invite-{}@example.test", Uuid::new_v4().simple()),
                CompanyAccessRole::Member,
            )
            .await,
        )
        .await
        .unwrap();
    for case in f.denial_cases() {
        check_fragments(&f, case, invite.id).await;
    }
}

async fn check_fragments(f: &Fixture, case: ScopeCase<'_>, invite_id: Uuid) {
    let ScopeCase {
        companies,
        invites,
        user,
        company_id: id,
        status,
    } = case;
    denied(
        company_invites_page(
            State(companies),
            State(invites.clone()),
            Fixture::auth(user),
            Path(id),
        )
        .await,
        status,
        &f.foreign.name,
    )
    .await;
    denied(
        edit_invite_form(
            State(invites.clone()),
            Fixture::auth(user),
            Path((id, invite_id)),
        )
        .await,
        status,
        &f.foreign.name,
    )
    .await;
    denied(
        cancel_invite_edit(
            State(invites.clone()),
            Fixture::auth(user),
            Path((id, invite_id)),
        )
        .await,
        status,
        &f.foreign.name,
    )
    .await;
    denied(
        delete_company_invite(
            State(invites.clone()),
            Fixture::auth(user),
            Path((id, invite_id)),
        )
        .await,
        status,
        &f.foreign.name,
    )
    .await;
    denied(
        create_company_invite(
            State(invites),
            Fixture::auth(user),
            Path(id),
            Form(InviteForm {
                access_level: None,
                email: "new@example.test".into(),
                role: None,
            }),
        )
        .await,
        status,
        &f.foreign.name,
    )
    .await;
}
