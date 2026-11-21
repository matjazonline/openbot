use super::*;

async fn assert_frozen_roles(f: &PeopleFixture, run: crate::domain::workflow::RunId, root: Uuid) {
    let manifest = f.role.manifest(run).await;
    assert_eq!(manifest.roles.len(), 2);
    for role in manifest.roles {
        assert!(matches!(
            role.role,
            DepartmentRole::Owner | DepartmentRole::Admin
        ));
        assert_eq!(role.department_id.as_uuid(), root);
        assert_eq!(role.principals.len(), 1);
        assert!(role.principals[0].agent_id.is_none());
    }
}

async fn assert_unavailable(f: &PeopleFixture, scope: ActivationRequest, decision: Uuid) {
    let refusal: Option<String> = sqlx::query_scalar(
            "SELECT assignment.content->>'refusal' FROM workflow_follow_decisions AS current \
             JOIN workflow_reviewer_assignments AS assignment USING(company_id,decision_id,assignment_revision) \
             WHERE current.company_id=$1 AND current.decision_id=$2",
        ).bind(scope.company.as_uuid()).bind(decision).fetch_one(f.role.p().pool()).await.unwrap();
    assert_eq!(refusal.as_deref(), Some("role_unavailable"));
    let retained: String = sqlx::query_scalar("SELECT role FROM membership_of($1,$2)")
        .bind(f.role.admission.binding.target.actor.user_id())
        .bind(scope.company.as_uuid())
        .fetch_one(f.role.p().pool())
        .await
        .unwrap();
    assert_eq!(
        retained, "owner",
        "the unavailable Admin does not remove the Owner role"
    );
}

async fn capture(tracking: ReviewerTracking) {
    let follows = matches!(tracking, ReviewerTracking::FollowRole);
    let f = Box::pin(PeopleFixture::with_tracking(
        1,
        json!(["owner", "admin"]),
        tracking,
    ))
    .await;
    let owner = f.role.admission.binding.target.actor;
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.role.company().as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    let owner_principal: Uuid = sqlx::query_scalar("SELECT principal_id FROM membership_of($1,$2)")
        .bind(owner.user_id())
        .bind(f.role.company().as_uuid())
        .fetch_one(f.role.p().pool())
        .await
        .unwrap();
    let run = f.role.admitted("root-role-reviewers").await;
    if !follows {
        assert_frozen_roles(&f, run, root).await;
    }
    let scope = f.scope(run).await;
    assert!(
        f.role
            .p()
            .park_decision(scope, &notices(f.role.p()))
            .await
            .unwrap()
            .is_some()
    );
    let decision: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2")
            .bind(scope.company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    let view = f
        .role
        .p()
        .read_decision(scope.company, owner, DecisionId(decision))
        .await
        .unwrap();
    let mut expected = vec![owner_principal, f.principals[0].as_uuid()];
    expected.sort();
    let mut actual = view.reviewers;
    actual.sort();
    assert_eq!(actual, expected);
    assert!(
        view.can_submit,
        "the actual root owner is an eligible reviewer"
    );
    crate::adapters::persistence::test_support::department_use_cases(f.role.p())
        .clear_role(
            owner.user_id(),
            scope.company.as_uuid(),
            DepartmentId::new(root),
            crate::application::ports::department::DepartmentRoleRemoval {
                role: DepartmentRole::Admin,
                principal: f.principals[0],
                successor: None,
            },
        )
        .await
        .unwrap();
    let current = f
        .role
        .p()
        .read_decision(scope.company, owner, DecisionId(decision))
        .await
        .unwrap();
    if follows {
        assert!(current.reviewers.is_empty());
        assert!(
            !current.can_submit,
            "a missing selected Admin role blocks the followed decision"
        );
        assert_unavailable(&f, scope, decision).await;
    } else {
        let mut actual = current.reviewers;
        actual.sort();
        assert_eq!(
            actual, expected,
            "frozen selection retains its captured holders"
        );
    }
}

#[tokio::test]
async fn root_membership_frozen_human_reviewers_capture_owner_and_admin_from_parent_root() {
    Box::pin(capture(ReviewerTracking::Frozen)).await;
}

#[tokio::test]
async fn root_membership_followed_human_reviewers_refresh_after_root_admin_role_removal() {
    Box::pin(capture(ReviewerTracking::FollowRole)).await;
}
