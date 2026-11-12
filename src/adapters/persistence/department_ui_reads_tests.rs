//! The reads behind the Departments tab (01-departments 04A), against PostgreSQL: the people and
//! agents its pickers offer, the names its detail shows, and the desks a department owns.
//! `department_ui_`.
use super::*;
use crate::entities::department::MAX_DEPARTMENT_PRINCIPAL_CHOICES;

#[tokio::test]
async fn department_ui_reads_name_people_and_agents_and_list_owned_desks() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let company = CompanyId::new(f.company);
    let choices = f
        .departments
        .principals(company, MAX_DEPARTMENT_PRINCIPAL_CHOICES)
        .await
        .unwrap();
    // People and agents only, agents first; external and system principals are never offered.
    let ids: Vec<PrincipalId> = choices.iter().map(|choice| choice.principal_id).collect();
    assert!(!ids.contains(&f.external_principal) && !ids.contains(&f.system_principal));
    let first_person = choices
        .iter()
        .position(|choice| choice.kind == PrincipalKind::Person)
        .unwrap();
    assert!(
        choices[..first_person]
            .iter()
            .all(|choice| choice.kind == PrincipalKind::Agent)
    );
    assert!(
        choices[first_person..]
            .iter()
            .all(|choice| choice.kind == PrincipalKind::Person)
    );
    // An agent by its current name and handle, a person by username and email.
    let worker = choices
        .iter()
        .find(|choice| choice.principal_id == f.agent_principal)
        .expect("the agent is offered");
    assert_eq!(worker.detail, "worker");
    sqlx::query(
        "UPDATE agents SET name = 'Renamed worker' WHERE id = \
         (SELECT agent_id FROM principals WHERE id = $1)",
    )
    .bind(f.agent_principal.as_uuid())
    .execute(f.pool())
    .await
    .unwrap();
    let member = choices
        .iter()
        .find(|choice| choice.principal_id == f.member_principal)
        .expect("the member is offered");
    let (username, email): (String, String) =
        sqlx::query_as("SELECT username::text, email::text FROM users WHERE id = $1")
            .bind(f.member)
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(
        (member.name.as_str(), member.detail.as_str()),
        (username.as_str(), email.as_str())
    );
    // The bound holds.
    assert_eq!(f.departments.principals(company, 1).await.unwrap().len(), 1);

    // By id: only this company's, under the agent's current name.
    let named = f
        .departments
        .principals_by_id(
            company,
            &[f.agent_principal, f.member_principal, f.foreign_principal],
        )
        .await
        .unwrap();
    let names: Vec<&str> = named.iter().map(|named| named.name.as_str()).collect();
    assert_eq!(named.len(), 2, "{names:?}");
    assert!(names.contains(&"Renamed worker"));
    assert!(
        f.departments
            .principals_by_id(company, &[])
            .await
            .unwrap()
            .is_empty()
    );

    // A department owns its own desk, listed at its primary address.
    let sales = f.create("Sales", None).await;
    let desk = f.desk_of(sales.id).await;
    let desks = f.departments.desks(company, sales.id).await.unwrap();
    assert_eq!(desks.len(), 1);
    assert_eq!((desks[0].id, desks[0].slug.as_str()), (desk, "sales"));
    assert!(desks[0].own, "the department's own desk is marked");
    // A second desk the department owns is listed after its own, unmarked.
    let extra = crate::application::ports::channel::ChannelPersistence::create(
        &f.persistence,
        f.company,
        crate::application::ports::channel::ChannelWrite {
            name: "Sales overflow".into(),
            slug: "sales-overflow".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id;
    sqlx::query("UPDATE channels SET department_id = $2 WHERE id = $1")
        .bind(extra)
        .bind(sales.id.as_uuid())
        .execute(f.pool())
        .await
        .unwrap();
    let desks = f.departments.desks(company, sales.id).await.unwrap();
    assert_eq!(
        desks
            .iter()
            .map(|desk| (desk.id, desk.own))
            .collect::<Vec<_>>(),
        [(desk, true), (extra, false)]
    );
    let other = f.create("Support", None).await;
    assert!(
        f.departments
            .desks(company, other.id)
            .await
            .unwrap()
            .iter()
            .all(|listed| listed.id != desk)
    );

    // The use case's view names the department's people and refuses a member's choices.
    f.use_cases
        .add_member(f.owner, f.company, sales.id, f.member_principal)
        .await
        .unwrap();
    let view = f
        .use_cases
        .department_view(f.owner, f.company, sales.id)
        .await
        .unwrap();
    assert!(view.principal(f.member_principal).is_some());
    assert_eq!(view.desks.len(), 2);
    assert!(view.desks[0].own);
    assert!(matches!(
        f.use_cases.principal_choices(f.member, f.company).await,
        Err(AppError::NotFound(_))
    ));
    let (offered, cut) = f
        .use_cases
        .principal_choices(f.admin, f.company)
        .await
        .unwrap();
    assert!(!cut && offered.len() == choices.len());
}
