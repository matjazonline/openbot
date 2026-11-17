use super::*;
use crate::application::ports::department::{
    DepartmentEdit, DepartmentRoleRemoval, DepartmentWrite,
};

fn default_policy(role: DepartmentRole, hops: u8) -> EscalationPolicy {
    EscalationPolicy {
        notify: role,
        hops: EscalationHops::try_from(hops).unwrap(),
    }
}
async fn set_default(f: &RoleFixture, unit: DepartmentId, policy: Option<EscalationPolicy>) {
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .update_department(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            unit,
            DepartmentEdit {
                escalation: Some(policy),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}
async fn selected_policy(f: &GuardFixture) -> Option<Value> {
    sqlx::query_scalar(
        "SELECT policy FROM workflow_escalation_cases WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_optional(f.p().pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_escalation_default_live_unit_policy_changed_after_admission_is_the_trigger_policy()
 {
    let f = Box::pin(GuardFixture::new(None)).await;
    let original = default_policy(DepartmentRole::Lead, 1);
    set_default(&f.people.role, f.people.role.department, Some(original)).await;
    let changed = default_policy(DepartmentRole::Sponsor, 2);
    set_default(&f.people.role, f.people.role.department, Some(changed)).await;
    let saved: Option<Value> =
        sqlx::query_scalar("SELECT escalation FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.p().pool())
            .await
            .unwrap();
    assert_eq!(
        saved, None,
        "unit default was never frozen into binding/run configuration"
    );
    assert_eq!(
        f.p().fail_run_on_guard(f.failure()).await.unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        selected_policy(&f).await,
        Some(serde_json::to_value(changed).unwrap())
    );
    set_default(&f.people.role, f.people.role.department, None).await;
    assert_eq!(
        selected_policy(&f).await,
        Some(serde_json::to_value(changed).unwrap()),
        "case keeps its trigger-time choice"
    );
}

#[tokio::test]
async fn workflow_escalation_default_binding_policy_wins_over_unit_and_ancestor() {
    let explicit = default_policy(DepartmentRole::Reviewer, 1);
    let f = Box::pin(GuardFixture::new(Some(explicit))).await;
    set_default(
        &f.people.role,
        f.people.role.department,
        Some(default_policy(DepartmentRole::Sponsor, 2)),
    )
    .await;
    f.p().fail_run_on_guard(f.failure()).await.unwrap();
    assert_eq!(
        selected_policy(&f).await,
        Some(serde_json::to_value(explicit).unwrap())
    );
}

#[tokio::test]
async fn workflow_escalation_default_nearest_ancestor_supplies_it_and_none_creates_no_ladder() {
    let f = Box::pin(GuardFixture::new(None)).await;
    let role = &f.people.role;
    let departments = crate::adapters::persistence::test_support::department_use_cases(f.p());
    let owner = role.admission.binding.target.actor.user_id();
    let root = departments
        .create_department(
            owner,
            role.company().as_uuid(),
            DepartmentWrite {
                name: "Default root".into(),
                slug: "default-root".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    let near = departments
        .create_department(
            owner,
            role.company().as_uuid(),
            DepartmentWrite {
                name: "Default near".into(),
                slug: "default-near".into(),
                parent_id: Some(root.id),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
    departments
        .update_department(
            owner,
            role.company().as_uuid(),
            role.department,
            DepartmentEdit {
                parent_id: Some(Some(near.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    set_default(role, root.id, Some(default_policy(DepartmentRole::Lead, 1))).await;
    let nearest = default_policy(DepartmentRole::Sponsor, 2);
    set_default(role, near.id, Some(nearest)).await;
    f.p().fail_run_on_guard(f.failure()).await.unwrap();
    assert_eq!(
        selected_policy(&f).await,
        Some(serde_json::to_value(nearest).unwrap())
    );
    let none = Box::pin(GuardFixture::new(None)).await;
    none.p().fail_run_on_guard(none.failure()).await.unwrap();
    assert_eq!(selected_policy(&none).await, None);
    assert_eq!(none.state().await["controls"], 0);
}

#[tokio::test]
async fn workflow_escalation_default_admission_refusal_uses_current_unit_policy() {
    let f = Box::pin(RoleFixture::new()).await;
    let prepared = f.prepared("default-refusal").await;
    let chosen = default_policy(DepartmentRole::Sponsor, 2);
    set_default(&f, f.department, Some(chosen)).await;
    crate::adapters::persistence::test_support::department_use_cases(f.p())
        .clear_role(
            f.admission.binding.target.actor.user_id(),
            f.company().as_uuid(),
            f.department,
            DepartmentRoleRemoval {
                role: DepartmentRole::Lead,
                principal: f.principal,
                successor: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        f.p().admit(&prepared).await,
        Err(AppError::WorkflowRole(_))
    ));
    let row:Value=sqlx::query_scalar("SELECT jsonb_build_object('kind',source_kind,'policy',policy,'department',origin_department_id) FROM workflow_escalation_cases WHERE company_id=$1")
        .bind(f.company().as_uuid()).fetch_one(f.p().pool()).await.unwrap();
    assert_eq!(
        row,
        json!({"kind":"admission","policy":chosen,"department":f.department})
    );
}

#[tokio::test]
async fn workflow_escalation_default_empty_followed_reviewers_uses_the_same_live_default() {
    let people = Box::pin(PeopleFixture::with_tracking(
        1,
        json!(["reviewer"]),
        ReviewerTracking::FollowRole,
    ))
    .await;
    let chosen = default_policy(DepartmentRole::Sponsor, 2);
    set_default(&people.role, people.role.department, Some(chosen)).await;
    let run = people.role.admitted("default-empty-follow").await;
    let scope = people.scope(run).await;
    people
        .role
        .p()
        .park_decision(scope, &notices(people.role.p()))
        .await
        .unwrap()
        .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(people.role.p())
        .clear_role(
            people.role.admission.binding.target.actor.user_id(),
            people.role.company().as_uuid(),
            people.role.department,
            DepartmentRoleRemoval {
                role: DepartmentRole::Reviewer,
                principal: people.principals[0],
                successor: None,
            },
        )
        .await
        .unwrap();
    let saved:Value=sqlx::query_scalar("SELECT jsonb_build_object('kind',source_kind,'policy',policy) FROM workflow_escalation_cases WHERE company_id=$1")
        .bind(people.role.company().as_uuid()).fetch_one(people.role.p().pool()).await.unwrap();
    assert_eq!(saved, json!({"kind":"empty_reviewers","policy":chosen}));
}

async fn assert_policy_guard(f: &RoleFixture, choices: PolicyChoices) {
    let expected = choices.pinned.unwrap_or(choices.current);
    let retained = choices.pinned.unwrap_or(choices.old);
    let forged = if choices.pinned.is_some() {
        choices.current
    } else {
        choices.old
    };
    // The source already has a case. Its uniqueness makes the valid control an idempotent
    // no-op, but the production BEFORE INSERT guard must still validate every candidate.
    let candidate = |policy| {
        sqlx::query(
        "INSERT INTO workflow_escalation_cases \
         (company_id,id,source_kind,run_id,execution_id,admission_refusal_id,empty_decision_id, \
          binding_id,binding_revision,origin_department_id,policy,reason_code,step_id,role) \
         SELECT company_id,gen_random_uuid(),source_kind,run_id,execution_id,admission_refusal_id,empty_decision_id, \
                binding_id,binding_revision,origin_department_id,$2,reason_code,step_id,role \
         FROM workflow_escalation_cases WHERE company_id=$1 ON CONFLICT DO NOTHING",
    ).bind(f.company().as_uuid()).bind(serde_json::to_value(policy).unwrap())
    };
    assert_eq!(
        candidate(expected)
            .execute(f.p().pool())
            .await
            .unwrap()
            .rows_affected(),
        0
    );
    let error = candidate(forged).execute(f.p().pool()).await.unwrap_err();
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert_eq!(error.constraint(), Some("workflow_escalation_case_guard"));
    let policies: Vec<Value> =
        sqlx::query_scalar("SELECT policy FROM workflow_escalation_cases WHERE company_id=$1")
            .bind(f.company().as_uuid())
            .fetch_all(f.p().pool())
            .await
            .unwrap();
    assert_eq!(policies, vec![serde_json::to_value(retained).unwrap()]);
}

#[derive(Clone, Copy)]
struct PolicyChoices {
    pinned: Option<EscalationPolicy>,
    old: EscalationPolicy,
    current: EscalationPolicy,
}

fn policy_choices() -> [PolicyChoices; 2] {
    [None, Some(default_policy(DepartmentRole::Reviewer, 1))].map(|pinned| PolicyChoices {
        pinned,
        old: default_policy(DepartmentRole::Lead, 1),
        current: default_policy(DepartmentRole::Sponsor, 2),
    })
}

#[tokio::test]
async fn workflow_escalation_default_sql_run_guard_rejects_stale_defaults_and_binding_override() {
    for choices in policy_choices() {
        let PolicyChoices {
            pinned,
            old,
            current,
        } = choices;
        let f = Box::pin(GuardFixture::new(pinned)).await;
        set_default(&f.people.role, f.people.role.department, Some(old)).await;
        f.p().fail_run_on_guard(f.failure()).await.unwrap();
        set_default(&f.people.role, f.people.role.department, Some(current)).await;
        assert_policy_guard(&f.people.role, choices).await;
    }
}

#[tokio::test]
async fn workflow_escalation_default_sql_admission_guard_rejects_stale_defaults_and_binding_override()
 {
    for choices in policy_choices() {
        let PolicyChoices {
            pinned,
            old,
            current,
        } = choices;
        let f = Box::pin(RoleFixture::new()).await;
        let binding = &f.admission.binding;
        let mut request = binding.request(Some(binding.state().await.revision));
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.channel));
        request.escalation = pinned;
        binding.service().configure(request).await.unwrap();
        set_default(&f, f.department, Some(old)).await;
        let prepared = f.prepared("forged-default-admission").await;
        crate::adapters::persistence::test_support::department_use_cases(f.p())
            .clear_role(
                binding.target.actor.user_id(),
                f.company().as_uuid(),
                f.department,
                DepartmentRoleRemoval {
                    role: DepartmentRole::Lead,
                    principal: f.principal,
                    successor: None,
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            f.p().admit(&prepared).await,
            Err(AppError::WorkflowRole(_))
        ));
        set_default(&f, f.department, Some(current)).await;
        assert_policy_guard(&f, choices).await;
    }
}

#[tokio::test]
async fn workflow_escalation_default_sql_empty_guard_rejects_stale_defaults_and_binding_override() {
    for choices in policy_choices() {
        let PolicyChoices {
            pinned,
            old,
            current,
        } = choices;
        let people = Box::pin(PeopleFixture::with_tracking(
            1,
            json!(["reviewer"]),
            ReviewerTracking::FollowRole,
        ))
        .await;
        let f = Box::pin(GuardFixture::admit(people, pinned, None)).await;
        set_default(&f.people.role, f.people.role.department, Some(old)).await;
        f.p()
            .park_decision(f.scope, &notices(f.p()))
            .await
            .unwrap()
            .unwrap();
        crate::adapters::persistence::test_support::department_use_cases(f.p())
            .clear_role(
                f.people.role.admission.binding.target.actor.user_id(),
                f.people.role.company().as_uuid(),
                f.people.role.department,
                DepartmentRoleRemoval {
                    role: DepartmentRole::Reviewer,
                    principal: f.people.principals[0],
                    successor: None,
                },
            )
            .await
            .unwrap();
        set_default(&f.people.role, f.people.role.department, Some(current)).await;
        assert_policy_guard(&f.people.role, choices).await;
    }
}
