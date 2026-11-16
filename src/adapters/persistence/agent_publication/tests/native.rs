use super::*;

#[tokio::test]
async fn autonomous_agent_publication_native_request_result_is_immutable_and_company_owned() {
    let f = Fixture::new().await;
    let request = f.request("original").await;
    let first = f.p.publish_agent(request.clone()).await.unwrap();
    for statement in [
        "UPDATE agent_publication_requests SET expected_draft_revision=2 WHERE company_id=$1",
        "DELETE FROM agent_publication_requests WHERE company_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(f.scope.company.as_uuid())
            .execute(f.p.pool())
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    assert_eq!(f.p.publish_agent(request).await.unwrap().id, first.id);
    CompanyPersistence::delete(&f.p, f.scope.company.as_uuid())
        .await
        .unwrap();
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_native_result_cannot_reference_another_agent_or_company() {
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let foreign = other
        .p
        .publish_agent(other.request("foreign").await)
        .await
        .unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO agent_publication_requests(company_id,agent_id,key,expected_draft_revision,result_revision) VALUES($1,$2,'foreign',1,$3)")
        .bind(f.scope.company.as_uuid()).bind(f.scope.agent.as_uuid()).bind(foreign.id.as_uuid())
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    tx.rollback().await.unwrap();
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
}

#[tokio::test]
async fn autonomous_agent_native_permissions_round_trip_freeze_and_library_copy() {
    use crate::domain::local_agents::LocalPermissions;
    let f = Fixture::new().await;
    let initial = AgentPersistence::get_by_id(&f.p, f.scope.agent.as_uuid())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(initial.native_permissions, LocalPermissions::Full);
    let mut frozen = None;
    for mode in [
        LocalPermissions::Auto,
        LocalPermissions::AcceptEdits,
        LocalPermissions::Full,
    ] {
        f.update(AgentWrite {
            native_permissions: mode,
            ..write()
        })
        .await;
        let loaded = AgentPersistence::get_by_id(&f.p, f.scope.agent.as_uuid())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.native_permissions, mode);
        let publication =
            f.p.publish_agent(f.request(mode.as_str()).await)
                .await
                .unwrap();
        assert_eq!(publication.definition.native_permissions, mode);
        let state = f.p.authoring_state(f.scope).await.unwrap();
        assert_eq!(state.active.unwrap().native_permissions, mode);
        if frozen.is_none() {
            frozen = Some(publication);
        }
    }
    assert_eq!(
        frozen.unwrap().definition.native_permissions,
        LocalPermissions::Auto
    );
    let library = AgentPersistence::create_library(
        &f.p,
        AgentWrite {
            name: "Native library".into(),
            slug: format!("native-library-{}", Uuid::new_v4().simple()),
            native_permissions: LocalPermissions::AcceptEdits,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let copy = crate::adapters::persistence::test_support::company_agents(&f.p)
        .copy_from_library(
            f.scope.actor,
            f.scope.company,
            library.id,
            AgentWrite {
                name: library.name.clone(),
                slug: library.slug.clone(),
                native_permissions: library.native_permissions,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .agent;
    assert_eq!(copy.native_permissions, LocalPermissions::AcceptEdits);
    let error = sqlx::query("UPDATE agents SET native_permissions='provider_default' WHERE id=$1")
        .bind(copy.id)
        .execute(f.p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}
