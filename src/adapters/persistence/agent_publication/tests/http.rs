use super::*;
use crate::application::workflow::{http::*, publication::ToolSnapshot, registry::ToolContract};
use secrecy::SecretString;
use serde_json::json;

fn operation(company: CompanyId) -> HttpOperation {
    HttpOperation {
        tool: ToolSnapshot {
            company_id: company,
            workflow: None,
            contract: ToolContract {
                name: TypeName::parse("http.fetch").unwrap(),
                connection: None,
                input_schema: json!({"type":"object","properties":{"method":{"type":"string"},"path":{"type":"string"}},"required":["method","path"],"additionalProperties":false}),
                output_schema: json!(true),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("http.request").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Read,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: true,
    }
}

async fn source(f: &Fixture, bearer: bool) -> HttpConnection {
    let connection =
        f.p.create_http(
            f.scope.company,
            f.scope.actor,
            HttpConnectionWrite {
                origin: HttpOrigin::parse("http://127.0.0.1:9").unwrap(),
                bearer,
            },
        )
        .await
        .unwrap();
    let connection =
        f.p.register_http_operation(
            f.scope.company,
            f.scope.actor,
            connection.id,
            connection.revision,
            operation(f.scope.company),
        )
        .await
        .unwrap();
    let binding = AgentToolBinding {
        id: AgentBindingId::new(Uuid::new_v4()),
        alias: AgentToolAlias::parse("customer.fetch").unwrap(),
        source: AgentBindingSource::Http {
            connection: connection.id,
            operation: TypeName::parse("http.fetch").unwrap(),
        },
    };
    f.update(AgentWrite {
        tool_bindings: Some(vec![binding]),
        ..write()
    })
    .await;
    connection
}

#[tokio::test]
async fn autonomous_agent_resolution_http_alias_captures_exact_connection_and_approval() {
    let f = Fixture::new().await;
    let connection = source(&f, false).await;
    let result = f.p.publish_agent(f.request("http").await).await.unwrap();
    assert_eq!(
        result.definition.tools[0].binding.alias.as_str(),
        "customer.fetch"
    );
    assert!(result.definition.tools[0].approval_required);
    assert!(
        matches!(result.definition.tools[0].binding.source, AgentBindingSource::Http { connection: id, .. } if id == connection.id)
    );
    let content = String::from_utf8(result.definition.canonical_content().unwrap()).unwrap();
    assert!(!content.contains("http://127.0.0.1:9"));
    let mut binding = result.definition.tools[0].binding.clone();
    binding.id = AgentBindingId::new(Uuid::new_v4());
    let other = AgentPersistence::create(
        &f.p,
        f.scope.company.as_uuid(),
        AgentWrite {
            name: "Other assistant".into(),
            slug: "other".into(),
            tool_bindings: Some(vec![binding.clone()]),
            ..write()
        },
    )
    .await
    .unwrap();
    let scope = AgentPublicationScope {
        agent: AgentId::new(other.id),
        ..f.scope
    };
    let preview = f.p.preview_agent(scope).await.unwrap();
    let other =
        f.p.publish_agent(PublishAgent {
            scope,
            expected_draft_revision: preview.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse("http").unwrap(),
        })
        .await
        .unwrap();
    assert_ne!(other.id, result.id);
    assert_ne!(
        other.definition.tools[0].binding.id,
        result.definition.tools[0].binding.id
    );
    assert_eq!(
        other.definition.tools[0].binding.alias,
        result.definition.tools[0].binding.alias
    );
}

#[tokio::test]
async fn autonomous_agent_resolution_http_requires_current_secret_and_enabled_connection() {
    let f = Fixture::new().await;
    let connection = source(&f, true).await;
    assert!(
        f.p.publish_agent(f.request("missing-secret").await)
            .await
            .is_err()
    );
    let current =
        f.p.replace_http_secret(
            f.scope.company,
            f.scope.actor,
            connection.id,
            connection.revision,
            Some(SecretString::from("fixture-http-token")),
        )
        .await
        .unwrap();
    let first = f.p.publish_agent(f.request("ready").await).await.unwrap();
    let content = String::from_utf8(first.definition.canonical_content().unwrap()).unwrap();
    assert!(!content.contains("fixture-http-token"));
    f.p.set_http_enabled(
        f.scope.company,
        f.scope.actor,
        current.id,
        current.revision,
        false,
    )
    .await
    .unwrap();
    assert!(
        f.p.publish_agent(f.request("disabled").await)
            .await
            .is_err()
    );
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
