use super::*;

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
        p.mcp_connections_by_ids(owner, &[selected.id; MAX_MCP_CONNECTIONS + 1])
            .await
            .is_err()
    );
    CompanyPersistence::delete(&p, owner).await.unwrap();
    CompanyPersistence::delete(&p, foreign).await.unwrap();
}
