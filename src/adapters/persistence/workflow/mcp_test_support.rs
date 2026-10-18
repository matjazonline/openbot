//! A local Streamable HTTP MCP server that records every request, plus the real
//! publish → bind → admit → claim path for `mcp.call`.
use super::*;
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

pub(super) const TOKEN: &str = "fixture-mcp-secret";
pub(super) const TOOL: &str = "lookup_customer";

pub(super) fn input_schema() -> Value {
    json!({"type":"object","properties":{"customer_id":{"type":"string","maxLength":64}},"required":["customer_id"],"additionalProperties":false})
}
pub(super) fn output_schema() -> Value {
    json!({"type":"object","properties":{"name":{"type":"string"},"tier":{"type":"integer"}},"required":["name"],"additionalProperties":false})
}
pub(super) fn listed_tools() -> Value {
    json!([{"name":TOOL,"description":"Find a customer","inputSchema":input_schema(),"outputSchema":output_schema(),"annotations":{"readOnlyHint":true,"idempotentHint":true}}])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Call {
    Json,
    Sse,
    ToolError,
    Link,
    BadStructured,
    Oversize,
    Echo,
    Hang,
    Fail,
    Expired,
}

pub(super) struct ServerState {
    base: Mutex<String>,
    log: Mutex<Vec<String>>,
    auth: Mutex<Vec<String>>,
    arguments: Mutex<Vec<Value>>,
    tools: Mutex<Value>,
    call: Mutex<Call>,
    fail_initialize: AtomicBool,
    hang_list: AtomicBool,
    pub(super) arrived: Notify,
    pub(super) release: Notify,
}
pub(super) struct McpServer {
    pub(super) url: String,
    pub(super) state: Arc<ServerState>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for McpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl McpServer {
    pub(super) fn log(&self) -> Vec<String> {
        self.state.log.lock().unwrap().clone()
    }
    pub(super) fn count(&self, entry: &str) -> usize {
        self.log().iter().filter(|line| *line == entry).count()
    }
    pub(super) fn calls(&self) -> usize {
        self.count("POST tools/call")
    }
    pub(super) fn arguments(&self) -> Vec<Value> {
        self.state.arguments.lock().unwrap().clone()
    }
    pub(super) fn auth(&self) -> Vec<String> {
        self.state.auth.lock().unwrap().clone()
    }
    pub(super) fn set_call(&self, call: Call) {
        *self.state.call.lock().unwrap() = call;
    }
    pub(super) fn set_tools(&self, tools: Value) {
        *self.state.tools.lock().unwrap() = tools;
    }
    pub(super) fn fail_initialize(&self, fail: bool) {
        self.state.fail_initialize.store(fail, Ordering::SeqCst);
    }
    pub(super) fn hang_list(&self, hang: bool) {
        self.state.hang_list.store(hang, Ordering::SeqCst);
    }
    /// Waits (bounded) until the log contains `entry`.
    pub(super) async fn observed(&self, entry: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.count(entry) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{entry} not observed: {:?}", self.log()));
    }
}

pub(super) async fn mcp_server(call: Call) -> McpServer {
    let state = Arc::new(ServerState {
        base: Mutex::new(String::new()),
        log: Mutex::default(),
        auth: Mutex::default(),
        arguments: Mutex::default(),
        tools: Mutex::new(listed_tools()),
        call: Mutex::new(call),
        fail_initialize: AtomicBool::new(false),
        hang_list: AtomicBool::new(false),
        arrived: Notify::new(),
        release: Notify::new(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    *state.base.lock().unwrap() = url.clone();
    let router = Router::new()
        .route("/", post(rpc).delete(end_session))
        .route("/linked", get(linked))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    McpServer { url, state, task }
}

fn record(state: &ServerState, entry: String) {
    state.log.lock().unwrap().push(entry);
}
async fn end_session(State(state): State<Arc<ServerState>>) -> StatusCode {
    record(&state, "DELETE".into());
    StatusCode::OK
}
async fn linked(State(state): State<Arc<ServerState>>) -> StatusCode {
    record(&state, "GET /linked".into());
    StatusCode::OK
}
async fn rpc(State(state): State<Arc<ServerState>>, headers: HeaderMap, body: Bytes) -> Response {
    let message: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let method = message["method"].as_str().unwrap_or_default().to_owned();
    record(&state, format!("POST {method}"));
    if let Some(value) = headers.get(header::AUTHORIZATION) {
        state
            .auth
            .lock()
            .unwrap()
            .push(value.to_str().unwrap_or_default().into());
    }
    let Some(id) = message.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match method.as_str() {
        "initialize" => {
            if state.fail_initialize.load(Ordering::SeqCst) {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
            let mut response = Json(json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}})).into_response();
            response.headers_mut().insert(
                "mcp-session-id",
                HeaderValue::from_static("fixture-session"),
            );
            return response;
        }
        "tools/list" => {
            if state.hang_list.load(Ordering::SeqCst) {
                state.arrived.notify_one();
                state.release.notified().await;
            }
            json!({"tools": state.tools.lock().unwrap().clone()})
        }
        "tools/call" => return call(&state, id, &message).await,
        _ => json!({}),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

async fn call(state: &ServerState, id: Value, message: &Value) -> Response {
    state
        .arguments
        .lock()
        .unwrap()
        .push(message["params"]["arguments"].clone());
    let structured = json!({"name":"Ada","tier":3});
    let mode = *state.call.lock().unwrap();
    let result = match mode {
        Call::Json | Call::Sse => {
            json!({"content":[{"type":"text","text":"Ada"}],"structuredContent":structured,"isError":false,"_meta":{"trace":"remote"}})
        }
        Call::ToolError => {
            json!({"content":[{"type":"text","text":"customer locked"}],"structuredContent":{"unexpected":true},"isError":true})
        }
        Call::Link => {
            let uri = format!("{}linked", state.base.lock().unwrap());
            json!({"content":[{"type":"resource_link","uri":uri,"name":"profile"}],"structuredContent":structured})
        }
        Call::BadStructured => json!({"content":[],"structuredContent":{"name":5},"isError":false}),
        Call::Oversize => {
            let block = json!({"type":"text","text":"x".repeat(15_000)});
            json!({"content":vec![block; 6],"structuredContent":structured,"isError":false})
        }
        Call::Echo => {
            json!({"content":[{"type":"text","text":format!("token {TOKEN}")}],"structuredContent":{"name":TOKEN},"isError":false})
        }
        Call::Hang => {
            state.arrived.notify_one();
            state.release.notified().await;
            json!({"content":[],"structuredContent":structured,"isError":false})
        }
        Call::Fail => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Call::Expired => return StatusCode::NOT_FOUND.into_response(),
    };
    let body = json!({"jsonrpc":"2.0","id":id,"result":result});
    if mode == Call::Sse {
        return (
            [(header::CONTENT_TYPE, "text/event-stream")],
            format!("event: message\ndata: {body}\n\n"),
        )
            .into_response();
    }
    Json(body).into_response()
}

pub(super) fn lease() -> LeasePolicy {
    LeasePolicy::new(Duration::from_secs(30)).unwrap()
}
pub(super) fn operation(company: CompanyId) -> McpOperation {
    McpOperation {
        tool: ToolSnapshot {
            company_id: company,
            contract: registry::ToolContract {
                name: TypeName::parse(TOOL).unwrap(),
                connection: None,
                input_schema: input_schema(),
                output_schema: output_schema(),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("mcp.lookup_customer").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Read,
                recovery: ActionRecovery::Reconcile,
            },
        },
        approval_required: false,
    }
}
fn reviewed() -> Vec<McpDiscoveredTool> {
    vec![McpDiscoveredTool {
        name: TOOL.to_string().try_into().unwrap(),
        description: "Find a customer".into(),
        input_schema: input_schema(),
        output_schema: Some(output_schema()),
    }]
}
pub(super) fn connection_write(endpoint: &str) -> McpConnectionWrite {
    McpConnectionWrite {
        slug: "customers".into(),
        endpoint: endpoint.to_string().try_into().unwrap(),
        enabled: true,
        auth: McpAuth::Bearer,
        discovered_tools: reviewed(),
        tool_grants: vec![TOOL.to_string().try_into().unwrap()],
    }
}

pub(super) struct McpSetup {
    pub(super) fixture: Fixture,
    pub(super) server: McpServer,
    pub(super) connection: CompanyMcpConnection,
    pub(super) operation: Uuid,
    pub(super) version: VersionId,
    pub(super) target: BindingTarget,
    pub(super) client: Arc<HttpMcpClient>,
}
impl McpSetup {
    pub(super) fn runtime(&self) -> McpRuntime {
        McpRuntime::new(
            self.fixture.persistence.clone(),
            self.client.clone(),
            lease(),
        )
    }
    pub(super) fn company(&self) -> CompanyId {
        self.fixture.target.company
    }
    pub(super) fn actor(&self) -> WorkflowActor {
        self.fixture.target.actor
    }
    pub(super) async fn current(&self) -> CompanyMcpConnection {
        McpPersistence::mcp_connections_by_ids(
            &self.fixture.persistence,
            self.company().as_uuid(),
            &[self.connection.id],
        )
        .await
        .unwrap()
        .remove(0)
    }
}
pub(super) fn guarded_client(url: &str) -> Arc<HttpMcpClient> {
    Arc::new(HttpMcpClient::new(
        EndpointPolicy::new([url.to_owned()]).unwrap(),
    ))
}

pub(super) async fn mcp_setup(call: Call) -> McpSetup {
    let server = mcp_server(call).await;
    let mut fixture = Fixture::new().await;
    fixture.persistence = PostgresPersistence::with_credential_cipher(
        fixture.persistence.pool().clone(),
        CredentialCipher::for_test(),
    );
    let (connection, operation) = Box::pin(configured(&fixture, &server.url)).await;
    let version = Box::pin(published(&fixture, connection.id)).await;
    let target = Box::pin(bound(&fixture, connection.id, version)).await;
    let client = guarded_client(&server.url);
    McpSetup {
        fixture,
        server,
        connection,
        operation,
        version,
        target,
        client,
    }
}

pub(super) async fn configured(fixture: &Fixture, url: &str) -> (CompanyMcpConnection, Uuid) {
    let p = &fixture.persistence;
    let company = fixture.target.company;
    let connection = p
        .create_mcp_connection(company.as_uuid(), connection_write(url))
        .await
        .unwrap();
    p.replace_mcp_token(
        company.as_uuid(),
        connection.id,
        connection.revision,
        Some(SecretString::from(TOKEN)),
    )
    .await
    .unwrap();
    let connection = McpPersistence::mcp_connections_by_ids(p, company.as_uuid(), &[connection.id])
        .await
        .unwrap()
        .remove(0);
    let version = p
        .register_mcp_operation(
            company,
            fixture.target.actor,
            McpOperationWrite {
                connection: connection.id,
                catalog_revision: connection.revision,
                expected_version: None,
                operation: operation(company),
            },
        )
        .await
        .unwrap();
    (connection, version)
}

pub(super) fn source() -> Value {
    json!({
        "format_version":1,
        "workflow_id":"00000000-0000-0000-0000-000000000001",
        "input_schema":{"type":"object","properties":{"customer_id":{"type":"string","maxLength":64}},"required":["customer_id"],"additionalProperties":false},
        "parameter_schema":true,
        "output_schema":true,
        "resources":[{"slot":"service","kind":"mcp"}],
        "entry":"lookup",
        "steps":{
            "lookup":{"type":"mcp.call","with":{
                "connection":{"literal":"service"},
                "tool":{"literal":TOOL},
                "arguments":{"object":{"customer_id":{"ref":"/input/customer_id"}}}
            },"routes":{"success":"result"}},
            "result":{"type":"data.map","with":{
                "value":{"default":{"ref":"/steps/lookup/output/structuredContent","value":{"literal":{"name":"none"}}}},
                "output_schema":{"literal":output_schema()}
            },"routes":{"success":"$end"}}
        },
        "limits":{"max_steps":10,"max_context_bytes":65536}
    })
}

pub(super) fn published(
    fixture: &Fixture,
    connection: Uuid,
) -> impl std::future::Future<Output = VersionId> {
    published_source(fixture, connection, source())
}

pub(super) async fn published_source(
    fixture: &Fixture,
    connection: Uuid,
    source: Value,
) -> VersionId {
    let p = &fixture.persistence;
    let directory = mcp::McpPublicationDirectory {
        persistence: p.clone(),
        selections: vec![mcp::McpPublicationSelection {
            connection,
            slot: ResourceName::parse("service").unwrap(),
            tool: TypeName::parse(TOOL).unwrap(),
        }],
    };
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}

pub(super) async fn bound(
    fixture: &Fixture,
    connection: Uuid,
    version: VersionId,
) -> BindingTarget {
    let p = &fixture.persistence;
    let target = BindingTarget {
        company: fixture.target.company,
        actor: fixture.target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = BindingService::new(Preflight, p.clone(), p.clone(), p.clone());
    let state = bindings
        .configure(ConfigureBinding {
            target,
            expected: None,
            association: RelatedAssociation::Company,
            version,
            params: json!({}),
            resources: BTreeMap::from([(
                ResourceName::parse("service").unwrap(),
                RuntimeResourceId::new(connection),
            )]),
        })
        .await
        .unwrap();
    bindings
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target
}

/// Admits one run and returns the activation scope of its first (mcp.call) job.
pub(super) async fn admitted(setup: &McpSetup, key: &str) -> ActivationRequest {
    let p = &setup.fixture.persistence;
    let company = setup.company();
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
        .admit(AdmitWorkflowRequest {
            company_id: company,
            actor: setup.actor(),
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: setup.target.binding,
            idempotency_key: IdempotencyKey::parse(key).unwrap(),
            input: json!({"customer_id":"c-42"}),
        })
        .await
        .unwrap();
    let AdmissionResult::Created(run) = admission else {
        panic!("{admission:?}")
    };
    job_scope(p, company, run, "lookup").await
}
pub(super) async fn job_scope(
    p: &PostgresPersistence,
    company: CompanyId,
    run: RunId,
    step: &str,
) -> ActivationRequest {
    let (execution, job): (Uuid, Uuid) = sqlx::query_as("SELECT execution.id,job.id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.workflow_execution_id=execution.id WHERE execution.run_id=$1 AND execution.step_id=$2")
        .bind(run.as_uuid()).bind(step).fetch_one(p.pool()).await.unwrap();
    ActivationRequest {
        company,
        run,
        execution: ExecutionId::new(execution),
        job: WorkflowJobId(job),
    }
}
pub(super) async fn claim(p: &PostgresPersistence, scope: ActivationRequest) -> ClaimedWorkflow {
    p.claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap()
}
pub(super) async fn claimed(setup: &McpSetup) -> ClaimedWorkflow {
    let scope = admitted(setup, "mcp-run").await;
    claim(&setup.fixture.persistence, scope).await
}
/// Makes a retried job due now (the test does not wait out the backoff) and claims it.
pub(super) async fn reclaim(p: &PostgresPersistence, scope: ActivationRequest) -> ClaimedWorkflow {
    sqlx::query("UPDATE background_tasks SET run_at=clock_timestamp() WHERE id=$1")
        .bind(scope.job.0)
        .execute(p.pool())
        .await
        .unwrap();
    claim(p, scope).await
}
pub(super) fn kind() -> WorkflowStepKind {
    WorkflowStepKind("mcp.call".into())
}
pub(super) fn tool_call(model: &str) -> McpToolCall {
    McpToolCall {
        slot: ResourceName::parse("service").unwrap(),
        tool: TypeName::parse(TOOL).unwrap(),
        arguments: json!({"customer_id":"c-42"}),
        call: ModelToolCallId::parse(model).unwrap(),
    }
}

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct ActionCounts {
    pub(super) dispatches: i64,
    pub(super) entries: i64,
    pub(super) receipts: i64,
}
pub(super) async fn action_counts(p: &PostgresPersistence) -> ActionCounts {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_action_dispatches) AS dispatches,
                (SELECT count(*) FROM workflow_action_remote_entries) AS entries,
                (SELECT count(*) FROM workflow_action_receipts) AS receipts",
    )
    .fetch_one(p.pool())
    .await
    .unwrap()
}
pub(super) fn counts(dispatches: i64, entries: i64, receipts: i64) -> ActionCounts {
    ActionCounts {
        dispatches,
        entries,
        receipts,
    }
}
