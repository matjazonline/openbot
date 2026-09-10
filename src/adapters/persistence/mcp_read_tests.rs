use super::*;

#[tokio::test]
async fn selection_readers_do_not_wait_for_company_writes_and_see_one_revision() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool.clone());
    let owner = company(&p).await;
    let a = agent(&p, owner, "readers").await;
    let connection = p.create_mcp_connection(owner, write("crm")).await.unwrap();
    let initial = p.agent_mcp_selection(owner, a).await.unwrap();
    let mut writer = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(owner)
        .execute(&mut *writer)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO agent_mcp_selections(company_id, agent_id, connection_id) VALUES ($1,$2,$3)",
    )
    .bind(owner)
    .bind(a)
    .bind(connection.id)
    .execute(&mut *writer)
    .await
    .unwrap();
    sqlx::query("INSERT INTO agent_mcp_selection_revisions(company_id, agent_id, revision) VALUES ($1,$2,2)")
        .bind(owner).bind(a).execute(&mut *writer).await.unwrap();
    // Both readers must finish while the competing writer still holds the company lock.
    let readers = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(
            p.agent_mcp_selection(owner, a),
            p.agent_mcp_selection(owner, a)
        )
    })
    .await;
    writer.commit().await.unwrap();
    let (left, right) = readers.expect("selection readers blocked on a company writer");
    for selection in [left.unwrap(), right.unwrap()] {
        assert_eq!(selection.revision, initial.revision);
        assert_eq!(selection.connection_ids, initial.connection_ids);
    }
    let committed = p.agent_mcp_selection(owner, a).await.unwrap();
    assert_eq!(committed.revision, 2);
    assert_eq!(committed.connection_ids, vec![connection.id]);
    CompanyPersistence::delete(&p, owner).await.unwrap();
}

#[tokio::test]
async fn runtime_catalog_reads_are_bounded_tenant_scoped_and_exclude_deleted_rows() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool);
    let owner = company(&p).await;
    let foreign = company(&p).await;
    let selected = p
        .create_mcp_connection(owner, write("selected"))
        .await
        .unwrap();
    let deleted = p
        .create_mcp_connection(owner, write("deleted"))
        .await
        .unwrap();
    let other = p
        .create_mcp_connection(foreign, write("foreign"))
        .await
        .unwrap();
    p.delete_mcp_connection(owner, deleted.id, deleted.revision)
        .await
        .unwrap();
    let rows = p
        .mcp_connections_by_ids(
            owner,
            &[
                selected.id,
                selected.id,
                deleted.id,
                other.id,
                Uuid::new_v4(),
            ],
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, selected.id);
    assert_eq!(rows[0].discovered_tools, selected.discovered_tools);
    assert_eq!(rows[0].tool_grants, selected.tool_grants);
    assert!(
        p.mcp_connections_by_ids(owner, &[])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        p.mcp_connections_by_ids(owner, &[selected.id; MAX_MCP_SELECTIONS + 1])
            .await
            .is_err()
    );
    assert!(p.agent_mcp_selection(owner, Uuid::new_v4()).await.is_err());
    CompanyPersistence::delete(&p, owner).await.unwrap();
    CompanyPersistence::delete(&p, foreign).await.unwrap();
}
