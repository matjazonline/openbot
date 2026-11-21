//! Existing lead/descendant topology shared by scope consumers.
use super::*;

async fn lead_units(f: &Fixture, agent: Uuid) -> crate::entities::department::DepartmentId {
    use crate::entities::department::DepartmentRole;
    use crate::ports::department::DepartmentWrite;
    let company = f.target.company;
    let actor = f.target.actor.user_id();
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let root = crate::adapters::persistence::company::root_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        company,
    )
    .await
    .unwrap();
    let local = departments
        .create_department(
            actor,
            company.as_uuid(),
            DepartmentWrite {
                name: "Memory local".into(),
                slug: "memory-local".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id;
    for (slug, parent) in [
        ("report-a", local),
        ("report-b", local),
        ("root-report", root),
    ] {
        departments
            .create_department(
                actor,
                company.as_uuid(),
                DepartmentWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    parent_id: Some(parent),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
    }
    let principal = crate::adapters::persistence::memory::agent_principal_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        company.as_uuid(),
        agent,
    )
    .await
    .unwrap();
    // Local is both own and a child readable through the root lead. This makes
    // the real consumer prove deduplication across overlapping selectors.
    for unit in [root, local] {
        departments
            .assign_role(
                actor,
                company.as_uuid(),
                unit,
                crate::application::ports::department::DepartmentRoleAssignment {
                    role: DepartmentRole::Lead,
                    principal: principal,
                    decider: false,
                },
            )
            .await
            .unwrap();
    }
    local
}

pub(super) async fn lead_desk(f: &Fixture, agent: Uuid, viewer: Uuid) -> RelatedAssociation {
    use crate::ports::channel::ChannelWrite;
    // Box substantial topology setup separately from the shared agent fixture.
    let local = Box::pin(lead_units(f, agent)).await;
    let company = f.target.company;
    let principal = crate::adapters::persistence::memory::person_principal_on(
        &mut f.persistence.pool().acquire().await.unwrap(),
        company.as_uuid(),
        viewer,
    )
    .await
    .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(&f.persistence)
        .add_member(
            f.target.actor.user_id(),
            company.as_uuid(),
            local,
            principal,
        )
        .await
        .unwrap();
    let desk = crate::adapters::persistence::test_support::create_channel(
        &f.persistence,
        company.as_uuid(),
        ChannelWrite {
            name: "Memory scope tools".into(),
            slug: "memory-scope-tools".into(),
            enabled: true,
            department_id: Some(local),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    RelatedAssociation::Channel(RelatedChannelId::new(desk.id))
}
