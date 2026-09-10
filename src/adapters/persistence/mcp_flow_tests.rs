//! Complete Rig tool flows against two local MCP servers and a local model endpoint.
use super::*;
use crate::{
    adapters::{
        harness::rig::{
            providers::{ProviderRegistry, ResolvedModelRequest},
            tools::{ToolBridge, mcp_model_id},
        },
        mcp::{HttpMcpClient, policy::EndpointPolicy},
    },
    app_error::{AppError, AppResult},
    entities::harness::{AgentCapabilitySpec, HarnessConfig, SubAgentScope},
    services::{
        harness::{ApprovalAsk, ApprovalVerdict, HarnessApprovals, mcp::McpToolDeclaration},
        mcp_client::McpClient,
        mcp_runtime::*,
        test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario},
    },
};
use async_trait::async_trait;
use rig::{agent::AgentBuilder, completion::Prompt};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

fn rpc(
    method: &'static str,
    id: Option<u64>,
    result: Value,
    token: &'static str,
) -> ScriptedExchange {
    let response = id
        .map(|id| ScriptedResponse::json(json!({"jsonrpc":"2.0","id":id,"result":result})))
        .unwrap_or_else(|| {
            let mut r = ScriptedResponse::json_body(String::new());
            r.status = 202;
            r
        });
    ScriptedExchange::new(
        move |r| {
            if r.body["method"] != method
                || !r.header_matches("authorization", &format!("Bearer {token}"))
            {
                return Err("MCP method or credential mismatch");
            }
            if method == "tools/call"
                && (r.body["params"]["name"] != "search"
                    || r.body["params"]["arguments"] != json!({"key":"record"}))
            {
                return Err("MCP call mismatch");
            }
            Ok(())
        },
        response,
    )
}
fn discovery(token: &'static str) -> Vec<ScriptedExchange> {
    vec![
        rpc(
            "initialize",
            Some(0),
            json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
            token,
        ),
        rpc("notifications/initialized", None, Value::Null, token),
        rpc(
            "tools/list",
            Some(1),
            json!({"tools":[{"name":"search","description":"Search","inputSchema":{"type":"object"}}]}),
            token,
        ),
    ]
}
fn exchanges(token: &'static str) -> Vec<ScriptedExchange> {
    let mut exchanges = discovery(token);
    exchanges.extend(discovery(token));
    exchanges.push(rpc(
        "tools/call",
        Some(2),
        json!({"content":[{"type":"text","text":"record-value"}],"isError":false}),
        token,
    ));
    exchanges
}
struct Receipt {
    identity: McpToolRef,
    args: Value,
    result: Option<Value>,
}
#[derive(Default)]
struct Journal {
    receipts: Mutex<HashMap<String, Receipt>>,
}
#[async_trait]
impl McpInvocationJournal for Journal {
    async fn completed(
        &self,
        _: &McpRunScope,
        declaration: &McpToolDeclaration,
        id: &str,
        args: &Value,
    ) -> AppResult<Option<Value>> {
        let receipts = self.receipts.lock().await;
        let Some(receipt) = receipts.get(id) else {
            return Ok(None);
        };
        if receipt.identity != declaration.identity || receipt.args != *args {
            return Err(AppError::Conflict("Receipt identity mismatch".into()));
        }
        Ok(receipt.result.clone())
    }
    async fn claim(
        &self,
        _: &McpRunScope,
        declaration: &McpToolDeclaration,
        id: &str,
        args: &Value,
    ) -> AppResult<McpClaim> {
        let mut receipts = self.receipts.lock().await;
        if receipts.contains_key(id) {
            return Err(AppError::Conflict("Already claimed".into()));
        }
        receipts.insert(
            id.into(),
            Receipt {
                identity: declaration.identity.clone(),
                args: args.clone(),
                result: None,
            },
        );
        Ok(McpClaim::Execute)
    }
    async fn complete(&self, _: &McpRunScope, id: &str, value: &Value) -> AppResult<()> {
        self.receipts
            .lock()
            .await
            .get_mut(id)
            .expect("claimed receipt")
            .result = Some(value.clone());
        Ok(())
    }
    async fn indeterminate(&self, _: &McpRunScope, _: &str) -> AppResult<()> {
        Err(AppError::BadRequest(
            "unexpected indeterminate fixture effect".into(),
        ))
    }
}
struct NoApprovals;
#[async_trait]
impl HarnessApprovals for NoApprovals {
    async fn decide(&self, _: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict> {
        panic!("MCP must never request approval")
    }
}
fn spec() -> AgentCapabilitySpec {
    AgentCapabilitySpec {
        response_contract: None,
        harness: HarnessKind::Rig,
        name: "fixture".into(),
        system_prompt: "test".into(),
        provider: "openai".into(),
        model: "model".into(),
        provider_base_url: None,
        skills: vec![],
        granted_tools: vec![],
        sub_agents: SubAgentScope::AllCompanySiblings,
        harness_config: HarnessConfig::empty(HarnessKind::Rig),
    }
}
fn completion(calls: Value) -> Value {
    json!({"id":"chat","object":"chat.completion","created":0,"model":"model","choices":[{"index":0,"finish_reason":if calls.is_null(){"stop"}else{"tool_calls"},"message":if calls.is_null(){json!({"role":"assistant","content":"verified"})}else{json!({"role":"assistant","content":null,"tool_calls":calls})}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
}
async fn definition(
    p: &PostgresPersistence,
    company: Uuid,
    url: &str,
    slug: &str,
    token: &str,
) -> Uuid {
    let mut value = write(slug);
    value.endpoint = url.to_owned().try_into().unwrap();
    let connection = p.create_mcp_connection(company, value).await.unwrap();
    p.replace_mcp_token(company, connection.id, 1, Some(SecretString::from(token)))
        .await
        .unwrap();
    connection.id
}
#[tokio::test]
async fn mcp_two_connections_execute_through_rig_without_approval_and_replay_without_network() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = Arc::new(PostgresPersistence::with_credential_cipher(
        pool,
        CredentialCipher::for_test(),
    ));
    let company = company(&p).await;
    let agent = agent(&p, company, "two").await;
    let left = scripted_scenario(exchanges("left-secret")).await;
    let right = scripted_scenario(exchanges("right-secret")).await;
    let client = Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([
            format!("{}/", left.base_url.trim_end_matches('/')),
            format!("{}/", right.base_url.trim_end_matches('/')),
        ])
        .unwrap(),
    ));
    let first = definition(&p, company, &left.base_url, "left", "left-secret").await;
    let second = definition(&p, company, &right.base_url, "right", "right-secret").await;
    // An unselected definition must never be fetched/compiled by prepare or by an individual
    // invocation. A whole-catalog read would fail validation on this stored schema.
    let unrelated = p
        .create_mcp_connection(company, write("unrelated"))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE company_mcp_connections SET discovery_json = $3 WHERE company_id = $1 AND id = $2",
    )
    .bind(company)
    .bind(unrelated.id)
    .bind(json!([{"name":"search","description":"Unused","input_schema":{"type":"invalid-type"}}]))
    .execute(p.pool())
    .await
    .unwrap();
    p.replace_agent_mcp_selection(
        p.agent_mcp_selection(company, agent).await.unwrap(),
        Some(vec![first, second]),
    )
    .await
    .unwrap();
    let journal = Arc::new(Journal::default());
    let runtime = Arc::new(McpRuntime::new(p.clone(), p.clone(), client.clone()));
    let host = runtime
        .prepare(
            McpRunScope {
                company_id: company,
                agent_id: agent,
                task_id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
            },
            HarnessKind::Rig,
            journal.clone(),
        )
        .await
        .unwrap();
    model_calls_both(host.clone()).await;
    assert_eq!(left.finish().await, Ok(7));
    assert_eq!(right.finish().await, Ok(7));
    let receipt_id = journal
        .receipts
        .lock()
        .await
        .iter()
        .find(|(_, r)| r.identity == host.available()[0].identity)
        .unwrap()
        .0
        .clone();
    assert!(
        host.invoke(&host.available()[0], &receipt_id, json!({"key":"record"}))
            .await
            .unwrap()
            .success
    );
    assert!(
        host.invoke(&host.available()[1], &receipt_id, json!({"key":"record"}))
            .await
            .is_err()
    );
    client.shutdown().await.unwrap();
    CompanyPersistence::delete(p.as_ref(), company)
        .await
        .unwrap();
}

#[tokio::test]
async fn mcp_management_authorizes_all_operations_and_refresh_never_expands_grants() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = Arc::new(PostgresPersistence::with_credential_cipher(
        pool,
        CredentialCipher::for_test(),
    ));
    let tenant = company(&p).await;
    let owner = CompanyPersistence::get_by_id(p.as_ref(), tenant)
        .await
        .unwrap()
        .unwrap()
        .user_id;
    let a = agent(&p, tenant, "selected").await;
    let mut script = discovery("management-secret");
    script.pop();
    script.push(rpc(
        "tools/list",
        Some(1),
        json!({"tools":[
            {"name":"search","inputSchema":{"type":"object","required":["key"]}},
            {"name":"new_tool","inputSchema":{"type":"object"}}
        ]}),
        "management-secret",
    ));
    let server = scripted_scenario(script).await;
    let client = Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([format!("{}/", server.base_url.trim_end_matches('/'))]).unwrap(),
    ));
    let use_cases = McpUseCases::new(p.clone(), p.clone(), p.clone(), client.clone());
    let id = definition(&p, tenant, &server.base_url, "managed", "management-secret").await;
    let current = use_cases.get(owner, tenant, id).await.unwrap();
    let selected = use_cases
        .select(
            owner,
            use_cases.selection(owner, tenant, a).await.unwrap(),
            Some(vec![id]),
        )
        .await
        .unwrap();
    assert_management_denied(&use_cases, &current, &selected).await;
    let usage = use_cases.selecting_agents(owner, tenant, id).await.unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].id, a);
    let refreshed = use_cases.refresh(owner, &current).await.unwrap();
    assert_eq!(refreshed.discovered_tools.len(), 2);
    assert!(
        refreshed.tool_grants.is_empty(),
        "changed and new tools require review"
    );
    assert!(refreshed.secret_set);
    assert!(refreshed.revision > current.revision);
    assert_eq!(
        use_cases.selection(owner, tenant, a).await.unwrap(),
        selected
    );
    client.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
    let other_company = company(&p).await;
    assert!(use_cases.get(owner, other_company, id).await.is_err());
    assert!(use_cases.selection(owner, other_company, a).await.is_err());
}

async fn model_calls_both(host: Arc<dyn crate::services::harness::mcp::HarnessMcpToolHost>) {
    let calls=host.available().iter().enumerate().map(|(i,d)|json!({"id":format!("call-{i}"),"type":"function","function":{"name":mcp_model_id(&d.identity),"arguments":"{\"key\":\"record\"}"}})).collect::<Vec<_>>();
    assert_ne!(calls[0]["function"]["name"], calls[1]["function"]["name"]);
    let llm = scripted_scenario(vec![
        ScriptedExchange::new(
            |r| {
                if r.body.to_string().contains("left-secret")
                    || r.body.to_string().contains("right-secret")
                {
                    Err("secret leaked")
                } else {
                    Ok(())
                }
            },
            ScriptedResponse::json(completion(json!(calls))),
        ),
        ScriptedExchange::new(
            |r| {
                let messages = r.body["messages"].as_array().ok_or("messages")?;
                let results = messages
                    .iter()
                    .filter(|m| m["role"] == "tool" && m.to_string().contains("record-value"))
                    .count();
                if results == 2 {
                    Ok(())
                } else {
                    Err("both tool results required")
                }
            },
            ScriptedResponse::json(completion(Value::Null)),
        ),
    ])
    .await;
    let bridge = Arc::new(
        ToolBridge::compile(&spec(), None)
            .unwrap()
            .with_mcp(host.clone())
            .unwrap()
            .with_approvals(Arc::new(NoApprovals)),
    );
    let model = ProviderRegistry::standard()
        .unwrap()
        .model(&ResolvedModelRequest {
            provider: &"openai".into(),
            model: &"model".into(),
            secret: &SecretString::from("model-key"),
            endpoint: Some(&llm.base_url),
        })
        .unwrap();
    let result = bridge
        .build_agent(AgentBuilder::from_model_handle(model))
        .prompt("Use both tools")
        .max_turns(2)
        .tool_concurrency(1)
        .await
        .unwrap();
    assert_eq!(result, "verified");
    assert_eq!(llm.finish().await, Ok(2));
}

async fn assert_management_denied(
    use_cases: &McpUseCases,
    current: &CompanyMcpConnection,
    selected: &AgentMcpSelection,
) {
    let outsider = Uuid::new_v4();
    let tenant = current.company_id;
    let id = current.id;
    let a = selected.agent_id;
    assert!(use_cases.get(outsider, tenant, id).await.is_err());
    assert!(use_cases.list(outsider, tenant).await.is_err());
    assert!(
        use_cases
            .create(outsider, tenant, write("unauthorized"))
            .await
            .is_err()
    );
    assert!(
        use_cases
            .update(outsider, current, write("unauthorized"))
            .await
            .is_err()
    );
    assert!(use_cases.delete(outsider, current).await.is_err());
    assert!(
        use_cases
            .replace_token(outsider, current, None)
            .await
            .is_err()
    );
    assert!(use_cases.refresh(outsider, current).await.is_err());
    assert!(use_cases.selection(outsider, tenant, a).await.is_err());
    assert!(
        use_cases
            .select(outsider, selected.clone(), Some(vec![]))
            .await
            .is_err()
    );
    assert!(
        use_cases
            .selecting_agents(outsider, tenant, id)
            .await
            .is_err()
    );
}

#[path = "mcp_durable_tests.rs"]
mod durable;
