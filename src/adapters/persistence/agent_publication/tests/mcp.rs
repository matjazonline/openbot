use super::*;
use crate::application::workflow::{mcp::*, publication::ToolSnapshot, registry::ToolContract};
use crate::entities::mcp::*;
use crate::use_cases::mcp::*;
use serde_json::json;

struct McpSource {
    connection: CompanyMcpConnection,
    binding: AgentToolBinding,
}

fn operation(company: CompanyId) -> McpOperation {
    McpOperation {
        tool: ToolSnapshot {
            company_id: company,
            workflow: None,
            contract: ToolContract {
                name: TypeName::parse("lookup").unwrap(),
                connection: None,
                input_schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}),
                output_schema: json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("mcp.lookup").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Read,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: true,
    }
}

fn connection_write(operation: &McpOperation) -> McpConnectionWrite {
    McpConnectionWrite {
        slug: "crm".into(),
        endpoint: "http://127.0.0.1:9".to_owned().try_into().unwrap(),
        enabled: true,
        auth: McpAuth::None,
        discovered_tools: vec![McpDiscoveredTool {
            name: "lookup".to_owned().try_into().unwrap(),
            description: "Look up a record".into(),
            input_schema: operation.tool.contract.input_schema.clone(),
            output_schema: Some(operation.tool.contract.output_schema.clone()),
        }],
        tool_grants: vec!["lookup".to_owned().try_into().unwrap()],
    }
}

fn source(f: &Fixture) -> impl std::future::Future<Output = McpSource> + '_ {
    source_at(f, "http://127.0.0.1:9")
}
async fn source_at(f: &Fixture, endpoint: &str) -> McpSource {
    let operation = operation(f.scope.company);
    let mut definition = connection_write(&operation);
    definition.endpoint = endpoint.to_string().try_into().unwrap();
    let connection =
        f.p.create_mcp_connection(f.scope.company.as_uuid(), definition)
            .await
            .unwrap();
    let version =
        f.p.register_mcp_operation(
            f.scope.company,
            f.scope.actor,
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: connection.revision,
                expected_version: None,
                operation,
            },
        )
        .await
        .unwrap();
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("crm.lookup").unwrap(),
        source: AgentBindingSource::Mcp {
            connection: connection.id,
            operation: TypeName::parse("lookup").unwrap(),
            reviewed_version: version,
            remote_name: "lookup".to_owned().try_into().unwrap(),
        },
    };
    f.update(AgentWrite {
        tool_bindings: Some(vec![binding.clone()]),
        ..write()
    })
    .await;
    McpSource {
        connection,
        binding,
    }
}

#[tokio::test]
async fn autonomous_agent_resolution_mcp_without_workflow_freezes_remote_name_policy_and_alias() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    let preview = f.p.preview_agent(f.scope).await.unwrap();
    let result = f.p.publish_agent(f.request("mcp").await).await.unwrap();
    assert_eq!(result.definition.tools[0].binding, source.binding);
    assert_eq!(
        result.definition.tools[0].binding.alias.as_str(),
        "crm.lookup"
    );
    assert!(result.definition.tools[0].approval_required);
    assert_eq!(
        preview.definition.canonical_content().unwrap(),
        result.definition.canonical_content().unwrap()
    );
    let workflows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_definitions WHERE company_id=$1")
            .bind(f.scope.company.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(workflows, 0);
    let content = String::from_utf8(result.definition.canonical_content().unwrap()).unwrap();
    assert!(!content.contains("http://127.0.0.1:9"));
}

#[tokio::test]
async fn autonomous_agent_resolution_mcp_schema_drift_refuses_without_replacing_active_revision() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    let mut changed = connection_write(&operation(f.scope.company));
    changed.discovered_tools[0].input_schema = json!({"type":"object","properties":{"other":{"type":"integer"}},"additionalProperties":false});
    f.p.update_mcp_connection(
        f.scope.company.as_uuid(),
        source.connection.id,
        source.connection.revision,
        changed,
    )
    .await
    .unwrap();
    assert!(f.p.preview_agent(f.scope).await.is_err());
    assert!(f.p.publish_agent(f.request("drift").await).await.is_err());
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_resolution_mcp_retired_operation_and_removed_grant_are_not_discovery() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    sqlx::query("UPDATE workflow_mcp_operations SET revoked_at=clock_timestamp() WHERE company_id=$1 AND connection_id=$2")
        .bind(f.scope.company.as_uuid()).bind(source.connection.id).execute(f.p.pool()).await.unwrap();
    assert!(
        f.p.publish_agent(f.request("not-reviewed").await)
            .await
            .is_err()
    );
    let mut changed = connection_write(&operation(f.scope.company));
    changed.tool_grants.clear();
    f.p.update_mcp_connection(
        f.scope.company.as_uuid(),
        source.connection.id,
        source.connection.revision,
        changed,
    )
    .await
    .unwrap();
    assert!(
        f.p.publish_agent(f.request("not-granted").await)
            .await
            .is_err()
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
}

#[tokio::test]
async fn autonomous_agent_resolution_mcp_reapproval_requires_fresh_concrete_binding() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    let old_version = match source.binding.source {
        AgentBindingSource::Mcp {
            reviewed_version, ..
        } => reviewed_version,
        _ => unreachable!(),
    };
    let new_version =
        f.p.register_mcp_operation(
            f.scope.company,
            f.scope.actor,
            McpOperationWrite {
                connection: source.connection.id,
                catalog_revision: source.connection.revision,
                expected_version: Some(old_version),
                operation: operation(f.scope.company),
            },
        )
        .await
        .unwrap();
    assert_ne!(new_version, old_version);
    assert!(
        f.p.publish_agent(f.request("stale-binding").await)
            .await
            .is_err()
    );
    let mut fresh = source.binding;
    fresh.id = AgentBindingId::new(Uuid::new_v4());
    if let AgentBindingSource::Mcp {
        reviewed_version, ..
    } = &mut fresh.source
    {
        *reviewed_version = new_version;
    }
    f.update(AgentWrite {
        tool_bindings: Some(vec![fresh.clone()]),
        ..write()
    })
    .await;
    let result =
        f.p.publish_agent(f.request("fresh-binding").await)
            .await
            .unwrap();
    assert_eq!(result.definition.tools[0].binding, fresh);
}

#[tokio::test]
async fn autonomous_agent_publication_capture_locks_mcp_catalog_operation_and_grant() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    let request = f.request("capture-first").await;
    let (publication, result) = super::races::prepare(&f, &request).await;
    for statement in [
        "UPDATE company_mcp_connections SET discovery_json=jsonb_set(discovery_json,'{0,description}','\"Edited\"'::jsonb) WHERE company_id=$1 AND id=$2",
        "UPDATE workflow_mcp_operations SET revoked_at=clock_timestamp() WHERE company_id=$1 AND connection_id=$2",
        "UPDATE company_mcp_tool_grants SET revoked_at=clock_timestamp() WHERE company_id=$1 AND connection_id=$2 AND revoked_at IS NULL",
    ] {
        let mut editor = f.p.pool().begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *editor)
            .await
            .unwrap();
        let error = sqlx::query(statement)
            .bind(f.scope.company.as_uuid())
            .bind(source.connection.id)
            .execute(&mut *editor)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        editor.rollback().await.unwrap();
    }
    publication.commit().await.unwrap();
    let mut removed = connection_write(&operation(f.scope.company));
    removed.tool_grants.clear();
    f.p.update_mcp_connection(
        f.scope.company.as_uuid(),
        source.connection.id,
        source.connection.revision,
        removed,
    )
    .await
    .unwrap();
    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM agent_binding_grants WHERE binding_id=$1",
    )
    .bind(source.binding.id.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert!(revoked);
    assert_eq!(
        f.p.published_agent_revision(f.scope, result.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        result.digest
    );
}

#[tokio::test]
async fn autonomous_agent_publication_catalog_edit_first_cannot_commit_a_mixed_review() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    let request = f.request("catalog-edit").await;
    let mut editor = f.p.pool().begin().await.unwrap();
    sqlx::query("UPDATE company_mcp_connections SET discovery_json=jsonb_set(discovery_json,'{0,input_schema}','{\"type\":\"object\",\"properties\":{\"other\":{\"type\":\"integer\"}},\"additionalProperties\":false}'::jsonb) WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(source.connection.id).execute(&mut *editor).await.unwrap();
    assert!(matches!(
        f.p.publish_agent(request.clone()).await,
        Err(AppError::DatabaseTimeout(_))
    ));
    editor.commit().await.unwrap();
    assert!(f.p.publish_agent(request).await.is_err());
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_resolution_malformed_stored_catalog_is_an_error_not_an_empty_capture() {
    let f = Fixture::new().await;
    let source = source(&f).await;
    sqlx::query("UPDATE company_mcp_connections SET discovery_json='[{\"name\":\"lookup\",\"description\":\"Missing schema\"}]'::jsonb WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(source.connection.id).execute(f.p.pool()).await.unwrap();
    assert!(f.p.preview_agent(f.scope).await.is_err());
    assert!(
        f.p.publish_agent(f.request("corrupt-catalog").await)
            .await
            .is_err()
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
}

#[tokio::test]
async fn vault_binding_agent_publication_refuses_an_integration_mcp_source() {
    let f = Fixture::new().await;
    let source = source_at(&f, "https://mcp.example.test/rpc").await;
    f.p.publish_agent(f.request("before-vault").await)
        .await
        .unwrap();
    attach_vault(
        &f,
        source.connection.id,
        source.connection.revision,
        crate::application::use_cases::integration_binding::BindingKind::Mcp,
    )
    .await;
    assert!(
        f.p.publish_agent(f.request("after-vault").await)
            .await
            .is_err()
    );
}
