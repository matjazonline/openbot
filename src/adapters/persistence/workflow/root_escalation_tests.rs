use super::*;
use crate::application::workflow::binding::{EscalationHops, EscalationPolicy};

async fn refused(role: DepartmentRole, hops: u8) -> PeopleFixture {
    let f = Box::pin(PeopleFixture::new(1)).await;
    let binding = &f.role.admission.binding;
    let state = binding.state().await;
    let mut request = binding.request(Some(state.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: role,
        hops: EscalationHops::try_from(hops).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    let prepared = f.role.prepared("root-escalation").await;
    crate::adapters::persistence::test_support::department_use_cases(f.role.p())
        .clear_role(
            binding.target.actor.user_id(),
            f.role.company().as_uuid(),
            f.role.department,
            DepartmentRole::Reviewer,
            f.principals[0],
        )
        .await
        .unwrap();
    assert!(matches!(
        f.role.p().admit(&prepared).await,
        Err(AppError::WorkflowRole(refusal))
            if refusal.kind == RoleRefusalKind::NoHolder
                && refusal.role == DepartmentRole::Reviewer
    ));
    f
}

async fn assert_delivery(f: &PeopleFixture, role: DepartmentRole) {
    let owner = f.role.admission.binding.target.actor.user_id();
    let expected_user = if role == DepartmentRole::Owner {
        owner
    } else {
        f.users[0]
    };
    let expected_principal: Uuid =
        sqlx::query_scalar("SELECT principal_id FROM membership_of($1,$2)")
            .bind(expected_user)
            .bind(f.role.company().as_uuid())
            .fetch_one(f.role.p().pool())
            .await
            .unwrap();
    let worker = crate::adapters::persistence::test_support::notification_worker(
        f.role.p(),
        Default::default(),
    );
    assert_eq!(
        worker.drain_once().await.unwrap(),
        1,
        "materialize the real refusal's first hop"
    );
    let recipients: Value = sqlx::query_scalar(
        "SELECT recipient_facts FROM workflow_run_escalations WHERE company_id=$1",
    )
    .bind(f.role.company().as_uuid())
    .fetch_one(f.role.p().pool())
    .await
    .unwrap();
    let recipients = recipients.as_array().unwrap();
    assert_eq!(recipients.len(), 1);
    assert_eq!(recipients[0]["principal"], json!(expected_principal));
    assert_eq!(recipients[0]["kind"], "person");
    assert_eq!(
        worker.drain_once().await.unwrap(),
        1,
        "project the selected root person's notice"
    );
    let actual: Vec<Uuid> = sqlx::query_scalar("SELECT recipient_user_id FROM notifications WHERE company_id=$1 ORDER BY recipient_user_id")
        .bind(f.role.company().as_uuid()).fetch_all(f.role.p().pool()).await.unwrap();
    assert_eq!(actual, vec![expected_user]);
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(f.role.company().as_uuid())
        .fetch_one(f.role.p().pool())
        .await
        .unwrap();
    assert_eq!(runs, 0, "refused admission must not fabricate a run");
}

#[tokio::test]
async fn root_membership_escalation_actual_worker_selects_owner_and_admin_with_both_hop_bounds() {
    for role in [DepartmentRole::Owner, DepartmentRole::Admin] {
        for hops in [1, 2] {
            let f = Box::pin(refused(role, hops)).await;
            assert_delivery(&f, role).await;
        }
    }
}
