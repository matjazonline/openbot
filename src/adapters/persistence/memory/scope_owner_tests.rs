//! Unit/principal ownership, deterministic creation and retained retirement against real SQL.
use super::tests::{Fixture, backend_pid, blocked_by};
use super::*;
use crate::application::memory::{NamespaceOwner, NamespaceReadiness};
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::department::{DepartmentEdit, DepartmentWrite};
use crate::entities::department::DepartmentId;
use crate::use_cases::agent::{AgentPersistence, AgentWrite};

#[tokio::test]
async fn memory_scope_shape_rejects_foreign_ineligible_mismatched_and_duplicate_owners() {
    let f = Fixture::new().await.unwrap();
    f.enable().await.unwrap();
    // This foreign department exists in the same database, distinguishing a tenant
    // mismatch from a merely missing owner.
    let other_company = super::tests::company(&f.persistence, f.owner).await;
    let foreign_root = crate::adapters::persistence::company::root_on(
        &mut *f.pool().acquire().await.unwrap(),
        crate::domain::workflow::CompanyId::new(other_company),
    )
    .await
    .unwrap();
    let external: Uuid = sqlx::query_scalar("INSERT INTO principals (id,company_id,kind,display_label) VALUES (gen_random_uuid(),$1,'external','External') RETURNING id")
        .bind(f.company).fetch_one(f.pool()).await.unwrap();
    let system = crate::adapters::persistence::participant::ensure_system_principal_on(
        &mut *f.pool().acquire().await.unwrap(),
        f.company,
    )
    .await
    .unwrap();
    for (kind, unit, principal) in [
        ("principal", None, Some(external)),
        ("principal", None, Some(system.as_uuid())),
        ("unit", Some(foreign_root.as_uuid()), None),
        ("principal", Some(f.root.as_uuid()), None),
        ("unit", None, Some(f.agent_principal.as_uuid())),
        (
            "unit",
            Some(f.root.as_uuid()),
            Some(f.agent_principal.as_uuid()),
        ),
        ("unit", None, None),
        ("company", Some(f.root.as_uuid()), None),
    ] {
        let error = sqlx::query("INSERT INTO memory_namespaces (company_id,id,scope_kind,unit_id,principal_id,state) VALUES ($1,gen_random_uuid(),$2,$3,$4,'provisioning')")
            .bind(f.company).bind(kind).bind(unit).bind(principal).execute(f.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    for owner in [
        NamespaceOwner::Unit(f.root),
        NamespaceOwner::Principal(f.agent_principal),
    ] {
        assert_eq!(f.ensure(owner).await, NamespaceReadiness::NotReady);
        let key = namespace_columns(owner);
        let duplicate = sqlx::query("INSERT INTO memory_namespaces (company_id,id,scope_kind,unit_id,principal_id,state) VALUES ($1,gen_random_uuid(),$2,$3,$4,'provisioning')")
            .bind(f.company).bind(key.kind).bind(key.unit).bind(key.principal).execute(f.pool()).await.unwrap_err();
        assert_eq!(
            duplicate.as_database_error().unwrap().code().as_deref(),
            Some("23505")
        );
        let error = sqlx::query("UPDATE memory_namespaces SET id=gen_random_uuid() WHERE company_id=$1 AND scope_kind=$2")
            .bind(f.company).bind(key.kind).execute(f.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().constraint(),
            Some("memory_namespace_owner")
        );
    }
}

fn department(slug: &str) -> DepartmentWrite {
    DepartmentWrite {
        name: slug.into(),
        slug: slug.into(),
        description: None,
        charter: None,
        parent_id: None,
        created_by: None,
    }
}

async fn remove_desks(f: &Fixture, unit: DepartmentId) {
    let desks: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id=$1 AND department_id=$2")
            .bind(f.company)
            .bind(unit.as_uuid())
            .fetch_all(f.pool())
            .await
            .unwrap();
    for desk in desks {
        ChannelPersistence::delete(&f.persistence, desk)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn memory_scope_lifecycle_units_create_reparent_membership_retire_and_recreate() {
    let f = Fixture::new().await.unwrap();
    f.enable().await.unwrap();
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(&f.persistence);
    let unit = departments
        .create_department(f.owner, f.company, department("care"), None)
        .await
        .unwrap();
    let target = NamespaceOwner::Unit(unit.id);
    let original = f.namespaces(target).await;
    assert_eq!(
        original.len(),
        1,
        "creation committed exactly one namespace"
    );
    let parent = departments
        .create_department(f.owner, f.company, department("support"), None)
        .await
        .unwrap();
    departments
        .update_department(
            f.owner,
            f.company,
            unit.id,
            DepartmentEdit {
                parent_id: Some(Some(parent.id)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    departments
        .add_member(f.owner, f.company, unit.id, f.agent_principal)
        .await
        .unwrap();
    departments
        .remove_member(f.owner, f.company, unit.id, f.agent_principal)
        .await
        .unwrap();
    assert_eq!(
        f.namespaces(target).await,
        original,
        "position/roster is not ownership"
    );
    remove_desks(&f, unit.id).await;
    departments
        .delete_department(f.owner, f.company, unit.id)
        .await
        .unwrap();
    let retired = f.namespaces(target).await;
    assert_eq!(
        (retired.len(), retired[0].0, retired[0].1.as_str()),
        (1, original[0].0, "removing")
    );
    assert_eq!(
        f.lifecycle(Some(original[0].0)).await,
        Some(("absent".into(), "pending".into(), 2))
    );
    assert_eq!(f.ensure(target).await, NamespaceReadiness::OwnerUnavailable);
    let recreated = departments
        .create_department(f.owner, f.company, department("care"), None)
        .await
        .unwrap();
    assert_ne!(recreated.id, unit.id);
    assert_ne!(
        f.namespaces(NamespaceOwner::Unit(recreated.id)).await[0].0,
        original[0].0
    );
}

#[tokio::test]
async fn memory_scope_lifecycle_principal_creation_and_rollback_are_atomic() {
    let f = Fixture::new().await.unwrap();
    f.enable().await.unwrap();
    let mut tx = f.pool().begin().await.unwrap();
    let principal = crate::adapters::persistence::participant::create_agent_principal_on(
        &mut tx,
        f.company,
        f.agent,
        "New label",
    )
    .await
    .unwrap();
    assert_eq!(principal, f.agent_principal);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_namespaces WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(f.company)
    .bind(principal.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1);
    tx.rollback().await.unwrap();
    assert!(
        f.namespaces(NamespaceOwner::Principal(principal))
            .await
            .is_empty()
    );
    let created = AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "New".into(),
            slug: "new".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let new = super::agent_principal_on(
        &mut *f.pool().acquire().await.unwrap(),
        f.company,
        created.id,
    )
    .await
    .unwrap();
    assert_eq!(f.namespaces(NamespaceOwner::Principal(new)).await.len(), 1);
    // Person restoration/upsert provisions in its transaction too; no duplicate namespace.
    let (username, email): (String, String) =
        sqlx::query_as("SELECT username::text,email::text FROM users WHERE id=$1")
            .bind(f.member)
            .fetch_one(f.pool())
            .await
            .unwrap();
    let mut tx = f.pool().begin().await.unwrap();
    let person = crate::adapters::persistence::participant::create_person_principal_on(
        &mut tx, f.company, f.member, &username, &email,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(person, f.member_principal);
    assert_eq!(
        f.namespaces(NamespaceOwner::Principal(person)).await.len(),
        1
    );
}

#[tokio::test]
async fn memory_scope_lifecycle_existing_principal_upsert_and_delete_compete_in_both_orders() {
    let f = Fixture::new().await.unwrap();
    f.enable().await.unwrap();
    let mut writer = f.pool().begin().await.unwrap();
    let holder = backend_pid(&mut writer).await;
    let principal = crate::adapters::persistence::participant::create_agent_principal_on(
        &mut writer,
        f.company,
        f.agent,
        "Upsert",
    )
    .await
    .unwrap();
    assert_eq!(principal, f.agent_principal);
    let p = f.persistence.clone();
    let agent = f.agent;
    let deletion = tokio::spawn(async move { AgentPersistence::delete(&p, agent).await });
    blocked_by(f.pool(), holder).await;
    writer.commit().await.unwrap();
    deletion.await.unwrap().unwrap();
    assert_eq!(
        f.namespaces(NamespaceOwner::Principal(principal)).await[0].1,
        "removing"
    );
    let second = AgentPersistence::create(
        &f.persistence,
        f.company,
        AgentWrite {
            name: "Second".into(),
            slug: "second".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut delete = f.pool().begin().await.unwrap();
    let holder = backend_pid(&mut delete).await;
    let locks = crate::adapters::persistence::agent::lock_agent_delete_on(&mut delete, second.id)
        .await
        .unwrap()
        .unwrap();
    crate::adapters::persistence::agent::finish_agent_delete_on(&mut delete, locks)
        .await
        .unwrap();
    let pool = f.pool().clone();
    let company = f.company;
    let upsert = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let result = crate::adapters::persistence::participant::create_agent_principal_on(
            &mut tx, company, second.id, "Too late",
        )
        .await;
        tx.rollback().await.unwrap();
        result
    });
    blocked_by(f.pool(), holder).await;
    delete.commit().await.unwrap();
    assert!(matches!(upsert.await.unwrap(), Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn memory_scope_shape_delivery_retains_target_and_subject_provenance() {
    let f = Fixture::new().await.unwrap();
    f.enable().await.unwrap();
    f.ensure(NamespaceOwner::Unit(f.root)).await;
    let namespace = f.namespaces(NamespaceOwner::Unit(f.root)).await[0].0;
    let retained:(Uuid,Uuid,Uuid)=sqlx::query_as("INSERT INTO memory_deliveries (company_id,id,namespace_id,kind,run_id,execution_id,idempotency_key,expires_at,request,disable_epoch,state,available_at,unit_id,principal_id,subject_principal_id) VALUES ($1,gen_random_uuid(),$2,'save',gen_random_uuid(),gen_random_uuid(),$3,clock_timestamp()+interval '1 hour','{\"version\":\"1\",\"body\":{}}',0,'pending',clock_timestamp(),$4,$5,$6) RETURNING unit_id,principal_id,subject_principal_id")
        .bind(f.company).bind(namespace).bind(format!("shape-{}",Uuid::new_v4().simple())).bind(f.root.as_uuid()).bind(f.agent_principal.as_uuid()).bind(f.member_principal.as_uuid()).fetch_one(f.pool()).await.unwrap();
    assert_eq!(
        retained,
        (
            f.root.as_uuid(),
            f.agent_principal.as_uuid(),
            f.member_principal.as_uuid()
        )
    );
}
