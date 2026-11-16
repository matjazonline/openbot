//! Real direct history and native target pins must survive failed retirement atomically.
use super::*;
use crate::application::ports::thread::ThreadPersistence;
use crate::entities::address::AddressTarget;

async fn state(f: &Departments) -> serde_json::Value {
    let mut state = serde_json::Map::new();
    for table in [
        "agents",
        "principals",
        "addresses",
        "channel_bindings",
        "agent_revisions",
        "agent_active_revisions",
        "agent_publication_requests",
        "workflow_definitions",
        "workflow_versions",
        "workflow_bindings",
        "workflow_binding_revisions",
        "channel_reply_settings",
        "threads",
    ] {
        let rows: serde_json::Value = sqlx::query_scalar(&format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text),'[]'::jsonb) \
             FROM {table} AS record WHERE record.company_id=$1",
        )).bind(f.company).fetch_one(f.pool()).await.unwrap();
        state.insert(table.into(), rows);
    }
    state.into()
}

#[tokio::test]
async fn agent_address_delete_retained_direct_thread_restores_every_owned_resource() {
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let created = create(&f, agent("retained"), None).await.unwrap();
    ThreadPersistence::create_thread(
        &f.persistence,
        f.company,
        AddressTarget::Principal(created.principal_id),
        "Retained direct work",
        &[],
    )
    .await
    .unwrap();
    let before = state(&f).await;
    let error = AgentPersistence::delete(&f.persistence, created.agent.id)
        .await
        .unwrap_err();
    assert!(
        matches!(error, AppError::Conflict(ref message) if message.contains("direct threads")),
        "{error}"
    );
    assert_eq!(state(&f).await, before);
}

#[tokio::test]
async fn direct_target_native_retained_principal_delete_cannot_commit() {
    let Some(f) = Departments::new().await else {
        return;
    };
    // A bare native principal has no selected workflow; this isolates the retained-thread pin.
    ThreadPersistence::create_thread(
        &f.persistence,
        f.company,
        AddressTarget::Principal(f.agent_principal),
        "Native retained target",
        &[],
    )
    .await
    .unwrap();
    let before = state(&f).await;
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(f.agent_principal.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("threads_target_principal_fk")
    );
    assert_eq!(state(&f).await, before);
}

#[tokio::test]
async fn direct_target_native_foreign_principal_insert_cannot_commit() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let Some(foreign) = Departments::new().await else {
        return;
    };
    let before = state(&f).await;
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO threads(id,company_id,principal_id,subject) VALUES(gen_random_uuid(),$1,$2,'Foreign target')")
        .bind(f.company).bind(foreign.agent_principal.as_uuid()).execute(&mut *tx).await.unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("threads_target_principal_fk")
    );
    assert_eq!(state(&f).await, before);
}

#[tokio::test]
async fn direct_target_native_foreign_principal_update_cannot_commit() {
    let Some(f) = Departments::new().await else {
        return;
    };
    let Some(foreign) = Departments::new().await else {
        return;
    };
    // A valid native row without participants isolates the deferred target pin from the
    // separate immediate FK that correctly makes populated thread membership immutable.
    let thread = Uuid::new_v4();
    sqlx::query("INSERT INTO threads(id,company_id,principal_id,subject) VALUES($1,$2,$3,'Original direct target')")
        .bind(thread).bind(f.company).bind(f.agent_principal.as_uuid()).execute(f.pool()).await.unwrap();
    let before = state(&f).await;
    let other_before = state(&foreign).await;
    let mut tx = f.pool().begin().await.unwrap();
    sqlx::query("UPDATE threads SET principal_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(f.company)
        .bind(thread)
        .bind(foreign.agent_principal.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = tx.commit().await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("threads_target_principal_fk")
    );
    assert_eq!(state(&f).await, before);
    assert_eq!(state(&foreign).await, other_before);
}

#[tokio::test]
async fn agent_address_delete_retained_external_binding_restores_every_owned_resource() {
    use crate::application::workflow::lifecycle::{
        BindingService, BindingTarget, ConfigureBinding,
    };
    use crate::application::workflow::{LifecycleAuthorizer, RelatedAssociation};
    use crate::domain::workflow::{VersionId, WorkflowBindingId};
    let Some(f) = Departments::new().await else {
        return;
    };
    connect(&f).await;
    let created = create(&f, agent("retained-published"), None).await.unwrap();
    let version:Uuid=sqlx::query_scalar(
        "SELECT version.id FROM workflow_definitions AS definition JOIN workflow_versions AS version \
         ON version.company_id=definition.company_id AND version.workflow_id=definition.id \
         WHERE definition.company_id=$1 AND definition.owner_agent_id=$2",
    ).bind(f.company).bind(created.agent.id).fetch_one(f.pool()).await.unwrap();
    let p = &f.persistence;
    let bindings = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let target = BindingTarget {
        company: CompanyId::new(f.company),
        actor: owner(&f),
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version: VersionId::new(version),
            params: serde_json::json!({}),
            resources: Default::default(),
            run_as: None,
            message_trigger: None,
            escalation: None,
        })
        .await
        .unwrap();
    let before = state(&f).await;
    let error = AgentPersistence::delete(p, created.agent.id)
        .await
        .unwrap_err();
    assert!(matches!(error, AppError::Conflict(_)), "{error}");
    assert_eq!(state(&f).await, before);
    let published: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_versions WHERE company_id=$1 AND id=$2)",
    )
    .bind(f.company)
    .bind(version)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert!(
        published,
        "the external binding's exact owned publication remains available"
    );
}
