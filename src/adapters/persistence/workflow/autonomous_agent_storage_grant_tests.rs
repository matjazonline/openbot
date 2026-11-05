use super::*;
use crate::application::workflow::mcp::{McpOperation, McpOperationWrite, McpOperations};
use crate::use_cases::mcp::McpPersistence;

#[tokio::test]
async fn autonomous_agent_storage_public_mcp_grant_edits_retain_identity_and_require_fresh_review()
{
    let f = AdmissionFixture::new().await;
    let p = f.persistence();
    let company = f.binding.target.company;
    let connection = Uuid::new_v4();
    let version = native_mcp(&f, connection).await;
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("crm").unwrap(),
        source: AgentBindingSource::Mcp {
            connection,
            operation: TypeName::parse("lookup").unwrap(),
            reviewed_version: version,
            remote_name: "lookup".to_owned().try_into().unwrap(),
        },
    };
    let agent = new_agent(&f, vec![binding.clone()]).await;
    sqlx::query(
        "INSERT INTO agent_binding_grants(company_id,agent_id,binding_id) VALUES($1,$2,$3)",
    )
    .bind(company.as_uuid())
    .bind(agent.id)
    .bind(binding.id.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    let current = p
        .list_mcp_connections(company.as_uuid())
        .await
        .unwrap()
        .remove(0);
    let mut write = mcp_write(&current);
    let retained = p
        .update_mcp_connection(
            company.as_uuid(),
            connection,
            current.revision,
            write.clone(),
        )
        .await
        .unwrap();
    assert_eq!(operation_version(p, connection).await, version);
    assert!(active_grant(&f, binding.id).await);
    write.tool_grants.clear();
    let removed = p
        .update_mcp_connection(
            company.as_uuid(),
            connection,
            retained.revision,
            write.clone(),
        )
        .await
        .unwrap();
    assert!(removed.tool_grants.is_empty());
    assert!(!operation_active(p, connection).await);
    assert!(!active_grant(&f, binding.id).await);
    assert_eq!(bindings(&f, agent.id).await, vec![binding.clone()]);
    write.tool_grants = current.tool_grants;
    let regranted = p
        .update_mcp_connection(company.as_uuid(), connection, removed.revision, write)
        .await
        .unwrap();
    assert_eq!(regranted.tool_grants.len(), 1);
    assert!(
        !operation_active(p, connection).await,
        "permission regrant is not operation reapproval"
    );
    native_rejection(
        sqlx::query("UPDATE workflow_mcp_operations SET revoked_at=NULL WHERE connection_id=$1")
            .bind(connection)
            .execute(p.pool())
            .await
            .unwrap_err(),
    );
    let reviewed = p
        .register_mcp_operation(
            company,
            f.binding.target.actor,
            McpOperationWrite {
                connection,
                catalog_revision: regranted.revision,
                expected_version: None,
                operation: McpOperation {
                    tool: serde_json::from_value(operation_snapshot(&f)).unwrap(),
                    approval_required: false,
                },
            },
        )
        .await
        .unwrap();
    assert_ne!(reviewed, version);
    assert!(operation_active(p, connection).await);
    assert!(
        !active_grant(&f, binding.id).await,
        "fresh review cannot restore an old binding incarnation"
    );
    assert_eq!(bindings(&f, agent.id).await, vec![binding]);
    native_rejection(
        sqlx::query("DELETE FROM company_mcp_tool_grants WHERE connection_id=$1")
            .bind(connection)
            .execute(p.pool())
            .await
            .unwrap_err(),
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
}

async fn operation_version(p: &PostgresPersistence, connection: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT version FROM workflow_mcp_operations WHERE connection_id=$1")
        .bind(connection)
        .fetch_one(p.pool())
        .await
        .unwrap()
}
async fn operation_active(p: &PostgresPersistence, connection: Uuid) -> bool {
    sqlx::query_scalar(
        "SELECT revoked_at IS NULL FROM workflow_mcp_operations WHERE connection_id=$1",
    )
    .bind(connection)
    .fetch_one(p.pool())
    .await
    .unwrap()
}
