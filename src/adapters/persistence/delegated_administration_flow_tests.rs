use super::*;

#[path = "delegated_administration_boundary_tests.rs"]
mod boundaries;

async fn binding(d: &Delegated, unit: DepartmentId) -> Uuid {
    let id: String = sqlx::query_scalar(
        "SELECT result->>'binding_id' FROM workflow_provisioning_commands WHERE company_id=$1 AND command_key=$2")
        .bind(d.f.company).bind(format!("department:{unit}"))
        .fetch_one(d.f.pool()).await.unwrap();
    Uuid::parse_str(&id).unwrap()
}

async fn current(d: &Delegated, binding: Uuid) -> (bool, Uuid) {
    sqlx::query_as(
        "SELECT head.active, revision.run_as_user_id FROM workflow_bindings AS head \
         JOIN workflow_binding_revisions AS revision ON revision.company_id=head.company_id \
           AND revision.binding_id=head.id AND revision.revision=head.configuration_revision \
         WHERE head.company_id=$1 AND head.id=$2",
    )
    .bind(d.f.company)
    .bind(binding)
    .fetch_one(d.f.pool())
    .await
    .unwrap()
}

async fn unreadable_desk(d: &Delegated, unit: DepartmentId) {
    use crate::use_cases::channel::{ChannelReaders, readable_channel};
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(d.f.member)
        .fetch_one(d.f.pool())
        .await
        .unwrap();
    let viewer = crate::entities::user::Viewer {
        user_id: d.f.member,
        email: email.into(),
        email_verified_at: None,
    };
    let readers = ChannelReaders {
        companies: &d.f.persistence,
        channels: &d.f.persistence,
        participants: &d.f.persistence,
    };
    assert!(
        readable_channel(&readers, &viewer, d.f.company, d.f.desk_of(unit).await)
            .await
            .unwrap()
            .is_none(),
        "management must not add a conversation View grant"
    );
}

#[tokio::test]
async fn delegated_admin_agent_lead_transitions_configure_without_conversation_access() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let f = &d.f;
    let unit = f
        .use_cases
        .create_department(
            f.member,
            f.company,
            f.write("Remote", Some(d.child.id)),
            Some(f.agent_principal),
        )
        .await
        .unwrap();
    let bound = binding(&d, unit.id).await;
    assert_eq!(current(&d, bound).await, (true, f.member));
    unreadable_desk(&d, unit.id).await;

    // A raw unpublished agent exercises initial capture rather than a reused active revision.
    let unpublished = agent_principal(f.pool(), f.company, "unpublished").await;
    sqlx::query("UPDATE agents SET provider='openai',model='gpt-4o-mini' WHERE company_id=$1 AND id=(SELECT agent_id FROM principals WHERE id=$2)")
        .bind(f.company).bind(unpublished.as_uuid()).execute(f.pool()).await.unwrap();
    f.use_cases
        .assign_role(
            f.member,
            f.company,
            unit.id,
            DepartmentRole::Lead,
            unpublished,
        )
        .await
        .unwrap();
    let published: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND agent_id=(SELECT agent_id FROM principals WHERE id=$2)")
        .bind(f.company).bind(unpublished.as_uuid()).fetch_one(f.pool()).await.unwrap();
    assert_eq!(published, 1);
    assert_eq!(current(&d, bound).await, (true, f.member));
    unreadable_desk(&d, unit.id).await;

    f.use_cases
        .assign_role(
            f.member,
            f.company,
            unit.id,
            DepartmentRole::Lead,
            f.owner_principal,
        )
        .await
        .unwrap();
    assert!(!current(&d, bound).await.0);
    f.use_cases
        .assign_role(
            f.member,
            f.company,
            unit.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await
        .unwrap();
    assert_eq!(current(&d, bound).await, (true, f.member));
    f.use_cases
        .clear_role(
            f.member,
            f.company,
            unit.id,
            DepartmentRole::Lead,
            f.agent_principal,
        )
        .await
        .unwrap();
    assert!(!current(&d, bound).await.0);
    unreadable_desk(&d, unit.id).await;
}

#[tokio::test]
async fn delegated_admin_competing_lead_removal_precedes_a_locked_management_write() {
    let Some(database) = own_database().await else {
        return;
    };
    let d = Arc::new(Delegated::on(database.pool.clone()).await);
    let f = &d.f;
    let mut removal = begin_department_write(&f.persistence, d.scope(f.owner))
        .await
        .unwrap();
    clear_department_role_on(
        &mut removal,
        &f.departments.templates,
        d.company(),
        WorkflowActor::authenticated(f.owner).unwrap(),
        d.unit.id,
        DepartmentRole::Lead,
        f.member_principal,
    )
    .await
    .unwrap();
    let writer = tokio::spawn({
        let d = d.clone();
        async move {
            d.f.departments
                .add_member(d.scope(d.f.member), d.unit.id, d.f.owner_principal)
                .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    removal.commit().await.unwrap();
    hidden(writer.await.unwrap());
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM department_members WHERE company_id=$1 AND department_id=$2 AND principal_id=$3")
        .bind(f.company).bind(d.unit.id.as_uuid()).bind(f.owner_principal.as_uuid())
        .fetch_one(f.pool()).await.unwrap();
    assert_eq!(count, 0, "a revoked lead leaves no membership effect");
    assert!(
        !manages_unit(&f.persistence, f.member, d.company(), d.unit.id)
            .await
            .unwrap()
    );
}
