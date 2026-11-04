use super::super::authorization_test_support::{Fixture, denied};
use super::*;
use axum::http::StatusCode;

fn workspace(f: &Fixture, user: Uuid) -> Workspace {
    Workspace {
        company_use_cases: f.companies.clone(),
        invite_use_cases: f.invites.clone(),
        user_use_cases: f.users.clone(),
        config: f.config.clone(),
        user_id: user,
    }
}

#[tokio::test]
async fn team_roles_come_from_membership_and_nonowners_get_forbidden() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    denied(
        invite_form_pane(workspace(&f, f.owner.id), Path(f.foreign.id)).await,
        StatusCode::NOT_FOUND,
        &f.foreign.name,
    )
    .await;
    denied(
        invite_form_pane(workspace(&f, f.member.id), Path(f.company.id)).await,
        StatusCode::FORBIDDEN,
        &f.company.name,
    )
    .await;
    let access = f
        .companies
        .readable_company(f.member.id, f.company.id)
        .await
        .unwrap();
    let ws = workspace(&f, f.member.id);
    assert!(matches!(ws.view(&access).role(), TeamRole::Member));
    let mut authoritative = access.clone();
    authoritative.company.user_id = f.member.id;
    assert!(
        matches!(ws.view(&authoritative).role(), TeamRole::Member),
        "display owner id cannot elevate membership"
    );
    assert!(
        close_pane(ws, Path(f.company.id)).await.is_ok(),
        "members can read the team"
    );
}

async fn denied_mutations(
    f: &Fixture,
    company_id: Uuid,
    expected: StatusCode,
    make_workspace: impl Fn() -> Workspace,
) {
    let responses = [
        create_invite(
            make_workspace(),
            Path(company_id),
            Form(InviteForm {
                email: format!("new-{}@example.test", Uuid::new_v4().simple()),
                role: Some(CompanyAccessRole::Member),
            }),
        )
        .await,
        update_member_role(
            make_workspace(),
            Path((company_id, f.member.id)),
            Form(MemberRoleForm {
                role: CompanyAccessRole::Admin,
            }),
        )
        .await,
        remove_member(make_workspace(), Path((company_id, f.member.id))).await,
    ];
    for response in responses {
        if expected == StatusCode::INTERNAL_SERVER_ERROR {
            assert!(
                matches!(response, Err(AppError::Database(_))),
                "original database failure survives"
            );
        }
        denied(response, expected, &f.company.name).await;
    }
}

#[tokio::test]
async fn team_mutations_refuse_members_admins_foreign_and_unknown_company_scopes() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for role in ["member", "admin"] {
        sqlx::query("UPDATE company_members SET role=$3 WHERE company_id=$1 AND user_id=$2")
            .bind(f.company.id)
            .bind(f.member.id)
            .bind(role)
            .execute(f.persistence.pool())
            .await
            .expect("fixture member role changed");
        denied_mutations(&f, f.company.id, StatusCode::FORBIDDEN, || {
            workspace(&f, f.member.id)
        })
        .await;
    }
    for company_id in [f.foreign.id, Uuid::new_v4()] {
        denied_mutations(&f, company_id, StatusCode::NOT_FOUND, || {
            workspace(&f, f.owner.id)
        })
        .await;
    }
}

#[tokio::test]
async fn team_mutations_propagate_database_errors_after_successful_readable_company_scope() {
    use crate::use_cases::company_test_support::MockCompanyPersistence;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let invites = Arc::new(CompanyInviteUseCases::new(
        Arc::new(MockCompanyPersistence::failing()),
        f.persistence.clone(),
    ));
    denied_mutations(&f, f.company.id, StatusCode::INTERNAL_SERVER_ERROR, || {
        let mut ws = workspace(&f, f.owner.id);
        ws.invite_use_cases = invites.clone();
        ws
    })
    .await;
}

#[tokio::test]
async fn team_owner_business_refusals_keep_inline_feedback() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let invalid = create_invite(
        workspace(&f, f.owner.id),
        Path(f.company.id),
        Form(InviteForm {
            email: "not-an-address".into(),
            role: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(invalid.status(), StatusCode::OK);
    let invite = f
        .invites
        .create_company_invite(
            f.owner.id,
            f.company.id,
            &format!("draft-{}@example.test", Uuid::new_v4().simple()),
            CompanyAccessRole::Member,
        )
        .await
        .unwrap();
    let invalid = update_invite(
        workspace(&f, f.owner.id),
        Path((f.company.id, invite.id)),
        Form(InviteForm {
            email: "not-an-address".into(),
            role: None,
        }),
    )
    .await
    .unwrap();
    assert_eq!(invalid.status(), StatusCode::OK);
}

#[tokio::test]
async fn team_owner_self_refusals_are_client_errors_and_owner_member_panes_stay_hidden() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    for refusal in [
        f.invites
            .remove_company_team_member(f.owner.id, f.company.id, f.owner.id)
            .await,
        f.invites
            .update_company_team_member_role(
                f.owner.id,
                f.company.id,
                f.owner.id,
                CompanyAccessRole::Member,
            )
            .await
            .map(|_| ()),
    ] {
        let error = refusal.unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)));
        assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    }
    // The team-member list intentionally excludes the separately displayed owner.
    denied(
        remove_member(workspace(&f, f.owner.id), Path((f.company.id, f.owner.id))).await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
    denied(
        update_member_role(
            workspace(&f, f.owner.id),
            Path((f.company.id, f.owner.id)),
            Form(MemberRoleForm {
                role: CompanyAccessRole::Member,
            }),
        )
        .await,
        StatusCode::NOT_FOUND,
        &f.company.name,
    )
    .await;
    assert!(
        f.companies
            .readable_company(f.owner.id, f.company.id)
            .await
            .unwrap()
            .membership
            .is_owner()
    );
}
