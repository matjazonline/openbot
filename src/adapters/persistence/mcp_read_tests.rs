use super::*;

#[tokio::test]
async fn catalog_reads_are_tenant_scoped_and_exclude_deleted_rows() {
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
    // The owner's catalog: its live connection only, not the deleted one or another company's.
    let rows = p.list_mcp_connections(owner).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, selected.id);
    assert_eq!(rows[0].discovered_tools, selected.discovered_tools);
    assert_eq!(rows[0].tool_grants, selected.tool_grants);
    let foreign_rows = p.list_mcp_connections(foreign).await.unwrap();
    assert_eq!(
        foreign_rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![other.id]
    );
    CompanyPersistence::delete(&p, owner).await.unwrap();
    CompanyPersistence::delete(&p, foreign).await.unwrap();
}
