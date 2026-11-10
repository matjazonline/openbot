//! The four agent memory tools through their production dispatch and persistence (phase 05 E2,
//! E3, E5, E6) against the contract fake: the read ceiling, the rise and consumption record
//! committed before the model sees a result, replay, scope eligibility, the tool save's freeze
//! and the controlled refusals. Each fixture admits its own run in its own company.
use super::*;
use crate::adapters::memory_service::{MemoryServiceHttp, contract_fake::ContractFake};
use crate::adapters::persistence::credentials::CredentialContext;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::memory::{
    MemoryNamespaces, MemorySettingsStore, NamespaceOwner, NamespaceReadiness,
};
use crate::application::workflow::memory_recall::MemoryRecall;
use crate::application::workflow::memory_tools::{
    MemoryTool, MemoryToolCall, MemoryToolDispatch, MemoryToolPersistence, MemoryToolRuntime,
    MemoryToolScope, memory_tool_schemas,
};
use crate::entities::agent_capability::AgentBuiltinTool;
use crate::infra::memory_config::{MemoryServiceConfig, ProvisionerToken};
use secrecy::SecretString;
use std::time::Duration;

const ADMIN: &str =
    "msk_adm_0190f1a2b3c44d5e8f9a0b1c2d3e4f50_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

const MEMORY: [AgentBuiltinTool; 4] = [
    AgentBuiltinTool::SearchMemory,
    AgentBuiltinTool::MemoryTimeline,
    AgentBuiltinTool::MemoryDetails,
    AgentBuiltinTool::SaveMemory,
];

struct Tools {
    s: SearchFixture,
    fake: ContractFake,
    client: Arc<MemoryServiceHttp>,
}

/// What the run committed, read back from its rows.
#[derive(Debug)]
struct Committed {
    level: i16,
    consumptions: Vec<Value>,
    deliveries: Vec<Value>,
}

impl Tools {
    async fn new(level: AccessLevel) -> Self {
        Box::pin(Self::with_tools(level, &MEMORY)).await
    }

    async fn with_tools(level: AccessLevel, tools: &[AgentBuiltinTool]) -> Self {
        let s = Box::pin(SearchFixture::with_tools(level, None, tools)).await;
        let fake = ContractFake::start(ADMIN).await;
        let client = Arc::new(
            MemoryServiceHttp::new(&MemoryServiceConfig {
                base_url: fake.base_url.clone(),
                provisioner: ProvisionerToken::parse(SecretString::from(ADMIN)).unwrap(),
                request_timeout: Duration::from_secs(5),
                recall_deadline: Duration::from_secs(1),
                interactive_in_flight: 2,
                background_in_flight: 2,
            })
            .unwrap(),
        );
        Self { s, fake, client }
    }

    fn p(&self) -> &PostgresPersistence {
        self.s.p()
    }

    fn company(&self) -> CompanyId {
        self.s.claim.fence.scope.company
    }

    async fn actor(&self) -> Uuid {
        let scope = self.s.claim.fence.scope;
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(scope.company.as_uuid())
            .bind(scope.run.as_uuid())
            .fetch_one(self.p().pool())
            .await
            .unwrap()
    }

    async fn namespace(&self, owner: NamespaceOwner) -> Option<(Uuid, String)> {
        let (kind, agent, user) = match owner {
            NamespaceOwner::Company => ("company", None, None),
            NamespaceOwner::Agent(agent) => ("agent", Some(agent), None),
            NamespaceOwner::User(user) => ("user", None, Some(user)),
        };
        sqlx::query_as(
            "SELECT id, state FROM memory_namespaces WHERE company_id=$1 AND scope_kind=$2 \
             AND agent_id IS NOT DISTINCT FROM $3 AND user_id IS NOT DISTINCT FROM $4",
        )
        .bind(self.company().as_uuid())
        .bind(kind)
        .bind(agent)
        .bind(user)
        .fetch_optional(self.p().pool())
        .await
        .unwrap()
    }

    /// The owner's namespace, provisioned as the reconciler would, under a token the fake grants.
    async fn ready(&self, owner: NamespaceOwner) -> Uuid {
        let company = self.company();
        assert_eq!(
            self.p().ensure_namespace(company, owner).await.unwrap(),
            NamespaceReadiness::NotReady
        );
        let (namespace, _) = self.namespace(owner).await.unwrap();
        let token = self.fake.client_token(company.as_uuid(), namespace);
        let envelope = self
            .p()
            .credential_cipher()
            .unwrap()
            .seal_envelope(
                &CredentialContext::memory_client_credential(company.as_uuid(), namespace),
                &SecretString::from(token),
            )
            .unwrap();
        sqlx::query("UPDATE memory_namespaces SET state='ready',credential_id=gen_random_uuid(),credential_envelope=$3 WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid()).bind(namespace).bind(envelope).execute(self.p().pool()).await.unwrap();
        namespace
    }

    /// An observation the service holds; later seeds occur later (timeline order).
    fn seed(&self, namespace: Uuid, level: u8, title: &str, body: &str) -> Uuid {
        let id = Uuid::new_v4();
        let mut state = self.fake.state.lock().unwrap();
        let minute = state.observations.len();
        state.observations.push((
            namespace.hyphenated().to_string(),
            json!({"observation_id":id,"kind":"fact","title":title,"body":body,
                "occurred_at":format!("2026-11-01T00:{minute:02}:00Z"),"access_level":level,
                "revision":1,"content_sha256":format!("{level:064}"),"sources":[],
                "index_state":"pending"}),
        ));
        id
    }

    fn runtime(&self) -> MemoryToolRuntime {
        MemoryToolRuntime::new(Arc::new(self.p().clone()), self.client.clone())
    }

    async fn prepare(&self, tool: MemoryTool, arguments: Value) -> MemoryToolCall {
        // A later preparation revalidates the tool reads an earlier attempt showed the model
        // (E7), as under `with_memory`; see `ToolReadsOnly`.
        let prepared = self
            .s
            .prepare_tool_on(&self.pausing_tool_reads(), tool.name(), arguments)
            .await;
        MemoryToolCall {
            origin: prepared.origin,
            tool,
            call: prepared.call,
            arguments: prepared.arguments,
        }
    }

    async fn call(&self, call: MemoryToolCall) -> Value {
        match self
            .runtime()
            .call_memory_tool(self.s.claim.fence, call)
            .await
            .unwrap()
        {
            AgentToolObservation::Completed(result) => result,
            _ => panic!("a memory tool call completes"),
        }
    }

    fn service_calls(&self, operation: &str) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|call| *call == operation)
            .count()
    }

    async fn committed(&self) -> Committed {
        let scope = self.s.claim.fence.scope;
        let pool = self.p().pool();
        let level = sqlx::query_scalar(
            "SELECT content_level::smallint FROM workflow_runs WHERE company_id=$1 AND id=$2",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
        let consumptions = sqlx::query_scalar("SELECT jsonb_build_object('invocation',invocation_id,'read_ceiling',read_ceiling::smallint,'exposed',exposed_level::smallint,'items',items) FROM workflow_memory_consumptions WHERE company_id=$1 AND run_id=$2 ORDER BY created_at")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_all(pool).await.unwrap();
        let deliveries = sqlx::query_scalar("SELECT jsonb_build_object('id',delivery.id,'key',delivery.idempotency_key,'invocation',delivery.invocation_id,'state',delivery.state,'scope',namespace.scope_kind,'agent',namespace.agent_id,'user',namespace.user_id,'context',delivery.request->'body'->'context','items',delivery.request->'body'->'items') FROM memory_deliveries AS delivery JOIN memory_namespaces AS namespace ON namespace.company_id=delivery.company_id AND namespace.id=delivery.namespace_id WHERE delivery.company_id=$1 AND delivery.run_id=$2 ORDER BY delivery.id")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_all(pool).await.unwrap();
        Committed {
            level,
            consumptions,
            deliveries,
        }
    }

    async fn invocation_result(&self, call: &MemoryToolCall) -> Option<Value> {
        let id = call.invocation().unwrap();
        self.s
            .checkpoint()
            .await
            .invocations
            .iter()
            .find(|item| item.call.invocation_id == id)
            .and_then(|item| item.result.clone())
    }
}

#[path = "memory_recall_tests.rs"]
mod recall;

#[path = "memory_capture_e2e_tests.rs"]
mod capture;

fn copy(call: &MemoryToolCall) -> MemoryToolCall {
    MemoryToolCall {
        origin: call.origin,
        tool: call.tool,
        call: call.call.clone(),
        arguments: call.arguments.clone(),
    }
}

fn error_code(result: &Value) -> &str {
    result["error"]["code"].as_str().unwrap_or_default()
}

fn ids(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The whole chain on the production dispatch: the model is offered the four tools with their
/// built-in contracts, a search answer within the ceiling reaches it only after its rise and
/// consumption commit, and a save freezes an intent citing what the execution recalled.
#[tokio::test]
async fn memory_integration_agent_memory_tools_search_and_save_through_the_production_dispatch() {
    let t = Tools::new(AccessLevel::L5).await;
    let scope = t.s.claim.fence.scope;
    knowledge::exactness_fixture::mark_exact(t.p().pool(), scope.company, scope.run).await;
    let company = t.ready(NamespaceOwner::Company).await;
    let agent = t.ready(NamespaceOwner::Agent(t.s.agent)).await;
    let public = t.seed(
        company,
        0,
        "Concise replies",
        "The customer prefers concise replies.",
    );
    let private = t.seed(
        company,
        5,
        "Concise replies floor",
        "PRIVATE-FLOOR concise replies 60",
    );
    t.seed(
        company,
        7,
        "Concise replies secret",
        "SECRET-SEVEN concise replies",
    );
    let own = t.seed(
        agent,
        3,
        "Concise replies agent",
        "AGENT-THREE concise replies",
    );
    let mut llm = scripted_llm(vec![
        LlmTurn::tool_call(
            MemoryTool::Search.name(),
            json!({"query":"concise replies"}),
        ),
        LlmTurn::tool_call(
            MemoryTool::Save.name(),
            json!({"facts":["The customer wants short answers."]}),
        ),
        LlmTurn::text("\"noted\""),
    ])
    .await;
    register_scripted_agent_base_url(t.s.agent, &llm.base_url);
    let memory: Arc<dyn MemoryToolDispatch> = Arc::new(t.runtime());
    let runtime = t.s.runtime_with(|tools| tools.with_memory(memory));
    let outcome =
        Box::pin(runtime.execute(&WorkflowStepKind("agent.run".into()), &t.s.claim)).await;
    assert_eq!(outcome.completed_result().unwrap(), json!("noted"));
    let requests = llm.observed();
    assert_eq!(requests.len(), 3);
    for tool in MemoryTool::ALL {
        let offered = requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| &tool["function"])
            .find(|function| function["name"] == tool.name())
            .unwrap_or_else(|| panic!("{} offered", tool.name()));
        assert_eq!(offered["description"], tool.description());
        let schema = memory_tool_schemas(tool).input;
        assert_eq!(
            offered["parameters"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            "{}: no namespace, tenant, user or level input",
            tool.name()
        );
        assert_eq!(offered["parameters"]["additionalProperties"], false);
    }
    let answered = requests[1]["messages"].to_string();
    assert!(answered.contains("PRIVATE-FLOOR") && answered.contains("AGENT-THREE"));
    assert!(!answered.contains("SECRET-SEVEN"), "above the read ceiling");
    assert!(requests[2]["messages"].to_string().contains("accepted"));
    let committed = t.committed().await;
    assert_eq!(committed.level, 5, "raised to the highest exposed item");
    assert!(knowledge::exactness_fixture::exact(t.p().pool(), scope.company, scope.run).await);
    let [consumption] = committed.consumptions.as_slice() else {
        panic!("one consumption: {:?}", committed.consumptions)
    };
    assert_eq!(consumption["read_ceiling"], 5);
    assert_eq!(consumption["exposed"], 5);
    let mut recalled: Vec<(String, String, i64)> = consumption["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["scope"].as_str().unwrap().to_owned(),
                item["id"].as_str().unwrap().to_owned(),
                item["access_level"].as_i64().unwrap(),
            )
        })
        .collect();
    recalled.sort();
    let mut expected = vec![
        ("agent".to_owned(), own.to_string(), 3),
        ("company".to_owned(), public.to_string(), 0),
        ("company".to_owned(), private.to_string(), 5),
    ];
    expected.sort();
    assert_eq!(recalled, expected);
    let [delivery] = committed.deliveries.as_slice() else {
        panic!("one save intent: {:?}", committed.deliveries)
    };
    let invocation = consumption["invocation"].as_str().unwrap();
    assert_ne!(delivery["invocation"], consumption["invocation"]);
    let save_invocation = Uuid::parse_str(delivery["invocation"].as_str().unwrap()).unwrap();
    assert_eq!(
        delivery["key"],
        format!("ob.t.{}", save_invocation.simple()),
        "one logical save per tool call (search was {invocation})"
    );
    assert_eq!(delivery["scope"], "agent");
    assert_eq!(delivery["agent"], json!(t.s.agent));
    assert_eq!(delivery["state"], "pending");
    assert_eq!(
        delivery["context"]["consumed"],
        json!({"kind":"exact","level":5})
    );
    assert_eq!(delivery["context"]["producing_ceiling"], 5);
    assert_eq!(
        delivery["context"]["external_sources"],
        json!([
            {"reference": format!("memory:company:{private}"), "access_level": 5},
            {"reference": format!("memory:agent:{own}"), "access_level": 3},
            {"reference": format!("memory:company:{public}"), "access_level": 0},
        ]),
        "distinct recalled observations, level desc then id"
    );
    assert_eq!(
        delivery["items"][0]["body"],
        "The customer wants short answers."
    );
    // The run's actor started it in its own session: the user scope was eligible but not yet
    // provisioned, so the search requested it in its own transaction and searched the rest.
    let (_, state) = t
        .namespace(NamespaceOwner::User(t.actor().await))
        .await
        .expect("user namespace requested");
    assert_ne!(state, "ready");
    assert_eq!(t.service_calls("observations/search"), 2);
    assert_eq!(
        t.service_calls("observations/save"),
        0,
        "delivery is asynchronous"
    );
    assert_eq!(llm.finish().await.unwrap(), 3);
}

/// A search filters to the host's read ceiling, records its exposure once and replays exactly;
/// a replay whose recorded exposure no longer fits the current ceiling refuses (E3).
#[tokio::test]
async fn memory_access_level_agent_memory_search_is_ceilinged_recorded_once_and_replayed() {
    let t = Tools::new(AccessLevel::L5).await;
    let company = t.ready(NamespaceOwner::Company).await;
    let low = t.seed(company, 2, "Discount floor", "AX-4401 discount floor 90");
    let high = t.seed(company, 5, "Discount floor", "AX-4410 discount floor 60");
    t.seed(company, 6, "Discount floor", "AX-4499 discount floor 10");
    let call = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"discount floor","scopes":["company"],"limit":10}),
        )
        .await;
    let result = t.call(copy(&call)).await;
    assert_eq!(ids(&result), vec![low.to_string(), high.to_string()]);
    assert!(
        result["items"].as_array().unwrap().iter().all(|item| {
            item["scope"] == "company" && item["access_level"].as_i64().unwrap() <= 5
        })
    );
    assert_eq!(t.invocation_result(&call).await, Some(result.clone()));
    let committed = t.committed().await;
    assert_eq!(committed.level, 5);
    assert_eq!(committed.consumptions.len(), 1);
    assert_eq!(t.service_calls("observations/search"), 1);
    // Replay: the committed result, no second service call and no second record.
    assert_eq!(t.call(copy(&call)).await, result);
    assert_eq!(t.service_calls("observations/search"), 1);
    assert_eq!(t.committed().await.consumptions.len(), 1);
    // The actor's grant drops below what the call exposed: the replay refuses.
    sqlx::query("UPDATE company_members SET access_level=3 WHERE company_id=$1 AND user_id=$2")
        .bind(t.company().as_uuid())
        .bind(t.actor().await)
        .execute(t.p().pool())
        .await
        .unwrap();
    let refused = t
        .runtime()
        .call_memory_tool(t.s.claim.fence, copy(&call))
        .await;
    assert!(
        matches!(refused, Err(AppError::KnowledgeRelease { .. })),
        "{refused:?}"
    );
    assert_eq!(t.service_calls("observations/search"), 1);
}

/// Details return whole bodies in the requested order, leave unreadable ids out without an
/// oracle, and record each body's hash; a timeline shows the readable neighbours in time order.
#[tokio::test]
async fn memory_integration_agent_memory_details_and_timeline_stay_within_the_ceiling() {
    let t = Tools::new(AccessLevel::L5).await;
    let company = t.ready(NamespaceOwner::Company).await;
    let agent = t.ready(NamespaceOwner::Agent(t.s.agent)).await;
    let first = t.seed(company, 0, "First", "FIRST-BODY");
    let secret = t.seed(company, 7, "Secret", "SECRET-BODY");
    let second = t.seed(company, 4, "Second", "SECOND-BODY");
    let own = t.seed(agent, 1, "Own", "OWN-BODY");
    let details = t
        .prepare(
            MemoryTool::Details,
            json!({"items":[{"scope":"agent","id":own},{"scope":"company","id":secret},
                {"scope":"company","id":second}]}),
        )
        .await;
    let result = t.call(details).await;
    assert_eq!(ids(&result), vec![own.to_string(), second.to_string()]);
    assert_eq!(result["items"][0]["text"], "OWN-BODY");
    assert_eq!(result["items"][1]["text"], "SECOND-BODY");
    assert_eq!(result["omitted"], 0);
    assert!(!result.to_string().contains(&secret.to_string()));
    let committed = t.committed().await;
    assert_eq!(committed.level, 4);
    let items = &committed.consumptions[0]["items"];
    assert_eq!(items[1]["content_sha256"], format!("{:064}", 4));
    assert_eq!(items[0]["scope"], "agent");
    let timeline = t
        .prepare(
            MemoryTool::Timeline,
            json!({"scope":"company","id":first,"before":0,"after":5}),
        )
        .await;
    let result = t.call(timeline).await;
    assert_eq!(ids(&result), vec![first.to_string(), second.to_string()]);
    assert_eq!(t.committed().await.consumptions.len(), 2);
    // An anchor the reader may not see is not found, and a refusal raises nothing.
    let hidden = t
        .prepare(MemoryTool::Timeline, json!({"scope":"company","id":secret}))
        .await;
    let result = t.call(copy(&hidden)).await;
    assert_eq!(error_code(&result), "memory.not_found");
    let committed = t.committed().await;
    assert_eq!((committed.level, committed.consumptions.len()), (4, 2));
    assert_eq!(t.invocation_result(&hidden).await, Some(result));
}

/// The user scope is a person's own manual run only (E2): eligible there, and refused for a
/// root admitted from a schedule even though every admission binds an unattached session. The
/// eligible run is of a workflow another person published (U4: allowed and documented).
#[tokio::test]
async fn memory_scope_user_memory_requires_the_actors_own_manual_session() {
    let t = Tools::new(AccessLevel::L5).await;
    let actor = t.actor().await;
    let publisher: Uuid = sqlx::query_scalar(
        "SELECT version.actor_id FROM workflow_runs AS run JOIN workflow_versions AS version \
         ON version.company_id=run.company_id AND version.id=run.version_id \
         WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(t.company().as_uuid())
    .bind(t.s.claim.fence.scope.run.as_uuid())
    .fetch_one(t.p().pool())
    .await
    .unwrap();
    assert_ne!(publisher, actor, "a foreign-authored manual workflow (U4)");
    let user = t.ready(NamespaceOwner::User(actor)).await;
    t.seed(user, 2, "Prefers mornings", "MY-PRIVATE prefers mornings");
    let search = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"prefers mornings","scopes":["user"]}),
        )
        .await;
    let result = t.call(search).await;
    assert_eq!(result["items"][0]["scope"], "user");
    let save = t
        .prepare(
            MemoryTool::Save,
            json!({"facts":["Prefers mornings."],"scope":"user"}),
        )
        .await;
    assert_eq!(t.call(save).await["status"], "accepted");
    let committed = t.committed().await;
    assert_eq!(committed.deliveries.len(), 1);
    assert_eq!(committed.deliveries[0]["scope"], "user");
    assert_eq!(committed.deliveries[0]["user"], json!(actor));

    let t = Tools::new(AccessLevel::L5).await;
    let actor = t.actor().await;
    let scope = t.s.claim.fence.scope;
    // Stand-in for a schedule's root (fixture SQL): the same run, admitted from a schedule
    // occurrence. The admission identity is immutable, so the row is replaced.
    let mut tx = t.p().pool().begin().await.unwrap();
    sqlx::query("WITH gone AS (DELETE FROM workflow_admissions WHERE company_id=$1 AND run_id=$2 RETURNING binding_id) INSERT INTO workflow_admissions (company_id,binding_id,source_key,run_id) SELECT $1,binding_id,$3,$2 FROM gone")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(format!(
            "v1:{}",
            json!(["schedule", Uuid::new_v4(), Uuid::new_v4()])
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let search = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"prefers mornings","scopes":["user"]}),
        )
        .await;
    assert_eq!(
        error_code(&t.call(search).await),
        "memory.scope_unavailable"
    );
    let save = t
        .prepare(
            MemoryTool::Save,
            json!({"facts":["Prefers mornings."],"scope":"user"}),
        )
        .await;
    assert_eq!(error_code(&t.call(save).await), "memory.scope_unavailable");
    assert!(t.committed().await.deliveries.is_empty());
    assert!(
        t.namespace(NamespaceOwner::User(actor)).await.is_none(),
        "an ineligible scope is never provisioned"
    );
    assert!(
        t.fake
            .calls()
            .iter()
            .all(|call| !call.starts_with("observations/"))
    );
}

/// A scope whose namespace is not provisioned is requested in its own transaction and answers
/// `memory.not_ready`, with no service read and nothing raised.
#[tokio::test]
async fn memory_scope_unprovisioned_scope_is_requested_and_refuses_not_ready() {
    let t = Tools::new(AccessLevel::L5).await;
    let search = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"anything","scopes":["agent"]}),
        )
        .await;
    assert_eq!(error_code(&t.call(search).await), "memory.not_ready");
    let (_, state) = t
        .namespace(NamespaceOwner::Agent(t.s.agent))
        .await
        .expect("agent namespace requested");
    assert_ne!(state, "ready");
    assert!(
        t.namespace(NamespaceOwner::Company).await.is_none(),
        "only the requested scope"
    );
    let committed = t.committed().await;
    assert_eq!(committed.level, 0);
    assert!(committed.consumptions.is_empty());
    assert!(
        t.fake
            .calls()
            .iter()
            .all(|call| !call.starts_with("observations/"))
    );
}

/// Revocation applies at runtime (06-D5): a call published while memory was enabled answers
/// `memory.not_enabled` once it is disabled, contacts nothing and replays that answer.
#[tokio::test]
async fn memory_scope_agent_memory_tools_refuse_once_memory_is_disabled() {
    let t = Tools::new(AccessLevel::L5).await;
    let company = t.ready(NamespaceOwner::Company).await;
    t.seed(
        company,
        0,
        "Concise replies",
        "The customer prefers concise replies.",
    );
    let search = t
        .prepare(MemoryTool::Search, json!({"query":"concise replies"}))
        .await;
    t.p()
        .disable_memory(t.company(), t.s.fixture.target.actor, 1)
        .await
        .unwrap();
    let result = t.call(copy(&search)).await;
    assert_eq!(error_code(&result), "memory.not_enabled");
    assert_eq!(t.call(copy(&search)).await, result, "replayed");
    let save = t
        .prepare(MemoryTool::Save, json!({"facts":["Too late."]}))
        .await;
    assert_eq!(error_code(&t.call(save).await), "memory.not_enabled");
    let committed = t.committed().await;
    assert_eq!(committed.level, 0);
    assert!(committed.consumptions.is_empty() && committed.deliveries.is_empty());
    assert!(
        t.fake
            .calls()
            .iter()
            .all(|call| !call.starts_with("observations/"))
    );
}

/// An agent version with memory tools publishes only while the company has memory enabled; the
/// same draft publishes once it is. (Admission and calls never re-check: see the disable test.)
#[tokio::test]
async fn memory_scope_memory_tools_publish_only_while_memory_is_enabled() {
    let f = Fixture::new().await;
    let draft = AgentPersistence::create(
        &f.persistence,
        f.target.company.as_uuid(),
        AgentWrite {
            name: "Rememberer".into(),
            slug: "rememberer".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            tool_bindings: Some(vec![
                crate::entities::agent_capability::AgentToolBinding::builtin(
                    AgentBuiltinTool::SearchMemory,
                ),
            ]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    CompanyPersistence::replace_model_connections_for_user(
        &f.persistence,
        f.target.actor.user_id(),
        f.target.company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let refused = f.try_publish_agent(draft.id).await.err();
    assert!(
        matches!(&refused, Some(AppError::NotFound(message)) if message.contains("Memory is not enabled")),
        "{refused:?}"
    );
    f.persistence
        .enable_memory(f.target.company, f.target.actor, 0, PROVISIONER)
        .await
        .unwrap();
    f.try_publish_agent(draft.id).await.unwrap();
}

/// E6 on the tool save's production entry (`save_memory`): it freezes the level committed when
/// it holds the run lock. Rise first: the save waits on the in-flight rise 4 → 6 and freezes 6.
/// Save first: held after taking the run lock (a third transaction holds the tenant row), the
/// rise waits on it and the intent keeps 4. An inexact run freezes `unknown` either way.
#[tokio::test]
async fn memory_access_level_competing_rise_and_tool_save_freeze_the_committed_level() {
    use super::super::memory_park_race::{pid, raise, waiter_of};
    for exact in [true, false] {
        for rise_first in [true, false] {
            let t = Tools::new(AccessLevel::L9).await;
            t.ready(NamespaceOwner::Agent(t.s.agent)).await;
            let pool = t.p().pool().clone();
            let scope = t.s.claim.fence.scope;
            if exact {
                knowledge::exactness_fixture::mark_exact(&pool, scope.company, scope.run).await;
            } else {
                let mut tx = pool.begin().await.unwrap();
                knowledge::lower_content_exact_on(&mut tx, scope.company, scope.run)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
            }
            raise(&pool, scope, AccessLevel::L4)
                .await
                .commit()
                .await
                .unwrap();
            let call = t
                .prepare(MemoryTool::Save, json!({"facts":["Prefers mornings."]}))
                .await;
            let p = t.p().clone();
            let fence = t.s.claim.fence;
            let spawn_save = move || {
                tokio::spawn(async move {
                    p.save_memory(
                        fence,
                        &call,
                        MemoryToolScope::Agent,
                        vec!["Prefers mornings.".into()],
                    )
                    .await
                })
            };
            let (save, expected) = if rise_first {
                let mut rise = raise(&pool, scope, AccessLevel::L6).await;
                let holder = pid(&mut rise).await;
                let save = spawn_save();
                waiter_of(&pool, holder).await;
                rise.commit().await.unwrap();
                (save, 6)
            } else {
                let mut tenant = pool.begin().await.unwrap();
                sqlx::query("SELECT 1 FROM memory_tenants WHERE company_id=$1 FOR UPDATE")
                    .bind(scope.company.as_uuid())
                    .execute(&mut *tenant)
                    .await
                    .unwrap();
                let tenant_holder = pid(&mut tenant).await;
                let save = spawn_save();
                let saver = waiter_of(&pool, tenant_holder).await;
                let rising = tokio::spawn({
                    let pool = pool.clone();
                    async move {
                        raise(&pool, scope, AccessLevel::L6)
                            .await
                            .commit()
                            .await
                            .unwrap()
                    }
                });
                // The rise waits on the run row the save holds.
                waiter_of(&pool, saver).await;
                tenant.commit().await.unwrap();
                rising.await.unwrap();
                (save, 4)
            };
            let outcome = save.await.unwrap().unwrap();
            assert!(
                matches!(
                    outcome,
                    crate::application::workflow::memory_tools::MemorySaveOutcome::Accepted(_)
                ),
                "exact={exact} rise_first={rise_first}"
            );
            let committed = t.committed().await;
            let want = if exact {
                json!({"kind":"exact","level":expected})
            } else {
                json!({"kind":"unknown"})
            };
            assert_eq!(
                committed.deliveries[0]["context"]["consumed"], want,
                "exact={exact} rise_first={rise_first}"
            );
            assert_eq!(committed.level, 6, "the rise committed in both orders");
        }
    }
}

/// E6 for tool reads: the answer reaches the model only after its rise and consumption record
/// commit. With the run row held once the service has answered, the call cannot return; on
/// release the result arrives with the run already raised.
#[tokio::test]
async fn memory_access_level_tool_read_commits_its_rise_before_the_model_sees_it() {
    use super::super::memory_park_race::{pid, waiter_of};
    let t = Tools::new(AccessLevel::L9).await;
    let company = t.ready(NamespaceOwner::Company).await;
    t.seed(company, 6, "Escalation", "ESCALATION-SIX contact");
    let search = t
        .prepare(
            MemoryTool::Search,
            json!({"query":"escalation","scopes":["company"]}),
        )
        .await;
    t.fake
        .state
        .lock()
        .unwrap()
        .delays
        .insert("observations/search".into(), Duration::from_millis(500));
    let runtime = t.runtime();
    let fence = t.s.claim.fence;
    let call = tokio::spawn(async move { runtime.call_memory_tool(fence, search).await });
    // The service has the request: the authorization transaction is over.
    tokio::time::timeout(Duration::from_secs(5), async {
        while t
            .fake
            .state
            .lock()
            .unwrap()
            .delays
            .contains_key("observations/search")
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the search reaches the service");
    let pool = t.p().pool().clone();
    let scope = t.s.claim.fence.scope;
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .execute(&mut *holder)
        .await
        .unwrap();
    let holder_pid = pid(&mut holder).await;
    waiter_of(&pool, holder_pid).await;
    assert!(
        !call.is_finished(),
        "no result before the observation commits"
    );
    assert_eq!(t.committed().await.level, 0);
    holder.commit().await.unwrap();
    let AgentToolObservation::Completed(result) = call.await.unwrap().unwrap() else {
        panic!("a completed read")
    };
    assert!(result.to_string().contains("ESCALATION-SIX"));
    let committed = t.committed().await;
    assert_eq!(committed.level, 6);
    assert_eq!(committed.consumptions.len(), 1);
}

/// The binding between a model call and its saved invocation (`require_memory_invocation`) and
/// the run's fence, on every production entry point: a forged fence, changed arguments, a call
/// naming another memory tool than the prepared one or one the agent did not bind, and a
/// tampered prepared tool all refuse before any service call and commit nothing.
#[tokio::test]
async fn memory_integration_forged_calls_and_fences_refuse_before_the_service() {
    use crate::application::workflow::memory_tools::MemoryReadObservation;
    let t = Box::pin(Tools::with_tools(
        AccessLevel::L5,
        &[AgentBuiltinTool::SearchMemory, AgentBuiltinTool::SaveMemory],
    ))
    .await;
    let company = t.ready(NamespaceOwner::Company).await;
    t.ready(NamespaceOwner::Agent(t.s.agent)).await;
    t.seed(
        company,
        0,
        "Concise replies",
        "The customer prefers concise replies.",
    );
    let call = t
        .prepare(MemoryTool::Search, json!({"query":"concise replies"}))
        .await;
    let before = t.committed().await;
    let original = t.s.claim.fence;
    let mut fences = Vec::new();
    for forge in 0..7 {
        let mut fence = original;
        match forge {
            0 => fence.scope.company = CompanyId::new(Uuid::new_v4()),
            1 => fence.scope.run = RunId::new(Uuid::new_v4()),
            2 => fence.scope.execution = ExecutionId::new(Uuid::new_v4()),
            3 => fence.scope.job.0 = Uuid::new_v4(),
            4 => fence.worker.0 = Uuid::new_v4(),
            5 => fence.generation.0 = Uuid::new_v4(),
            _ => fence.attempt.0 += 1,
        }
        fences.push(fence);
    }
    let observation = || MemoryReadObservation {
        result: json!({"items":[]}),
        exposed: AccessLevel::PUBLIC,
        read_ceiling: AccessLevel::L5,
        items: Vec::new(),
    };
    for fence in fences {
        assert!(
            t.runtime()
                .call_memory_tool(fence, copy(&call))
                .await
                .is_err()
        );
        assert!(
            t.p()
                .observe_memory_read(fence, &call, observation())
                .await
                .is_err()
        );
        assert!(
            t.p()
                .refuse_memory_tool(fence, &call, json!({"error":{"code":"x","message":"y"}}))
                .await
                .is_err()
        );
        assert!(
            t.p()
                .save_memory(fence, &call, MemoryToolScope::Agent, vec!["x".into()])
                .await
                .is_err()
        );
    }
    let mut changed = copy(&call);
    changed.arguments = json!({"query":"something else"});
    assert!(
        t.runtime()
            .call_memory_tool(original, changed)
            .await
            .is_err()
    );
    for tool in [MemoryTool::Save, MemoryTool::Details, MemoryTool::Timeline] {
        // Save is bound but is not this invocation's tool; details and timeline are unbound.
        let mut other = copy(&call);
        other.tool = tool;
        assert!(
            t.runtime().call_memory_tool(original, other).await.is_err(),
            "{tool:?}"
        );
    }
    let mut other = copy(&call);
    other.tool = MemoryTool::Save;
    assert!(
        t.p()
            .save_memory(original, &other, MemoryToolScope::Agent, vec!["x".into()])
            .await
            .is_err(),
        "a save through a search invocation"
    );
    // A prepared tool cannot be widened afterwards: the database refuses the edit.
    let scope = original.scope;
    let tampered = sqlx::query("UPDATE task_harness_runs SET workflow_prepared=jsonb_set(workflow_prepared,'{tools,0,input_schema}','{\"type\":\"object\"}') WHERE company_id=$1 AND workflow_run_id=$2 AND workflow_execution_id=$3 AND task_id=$4")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0)
        .execute(t.p().pool()).await;
    assert!(
        tampered
            .unwrap_err()
            .to_string()
            .contains("prepared inputs are immutable")
    );
    // Nothing was sent or committed; the genuine call still works.
    assert!(
        t.fake
            .calls()
            .iter()
            .all(|call| !call.starts_with("observations/"))
    );
    let after = t.committed().await;
    assert_eq!(
        (
            after.level,
            after.consumptions.len(),
            after.deliveries.len()
        ),
        (before.level, 0, 0)
    );
    assert_eq!(t.invocation_result(&call).await, None);
    assert_eq!(ids(&t.call(copy(&call)).await).len(), 1);
}

/// Lost ownership: after a replacement claim the stale owner can neither save nor read the
/// committed receipt; the new owner replays it, and a second save through the same invocation
/// returns the same receipt with no second intent (one logical save per tool call).
#[tokio::test]
async fn memory_integration_a_replacement_claim_fences_the_stale_saver_and_replays_one_intent() {
    use crate::application::workflow::lease::LeaseReleaseCause;
    let t = Tools::new(AccessLevel::L5).await;
    t.ready(NamespaceOwner::Agent(t.s.agent)).await;
    let call = t
        .prepare(MemoryTool::Save, json!({"facts":["Prefers mornings."]}))
        .await;
    let stale = t.s.claim.fence;
    let first = t.call(copy(&call)).await;
    assert_eq!(first["status"], "accepted");
    assert!(
        t.p()
            .release_io(stale, lease(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 microsecond' WHERE id=$1",
    )
    .bind(stale.scope.job.0)
    .execute(t.p().pool())
    .await
    .unwrap();
    let current = t
        .p()
        .claim_io(stale.scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(current.fence.generation, stale.generation);
    assert!(
        t.p()
            .save_memory(stale, &call, MemoryToolScope::Agent, vec!["Again.".into()])
            .await
            .is_err(),
        "the stale owner freezes nothing"
    );
    assert!(
        t.runtime()
            .call_memory_tool(stale, copy(&call))
            .await
            .is_err()
    );
    let replayed = match t
        .runtime()
        .call_memory_tool(current.fence, copy(&call))
        .await
        .unwrap()
    {
        AgentToolObservation::Completed(result) => result,
        _ => panic!("a receipt"),
    };
    assert_eq!(
        replayed, first,
        "the new owner replays the committed receipt"
    );
    let again = t
        .p()
        .save_memory(
            current.fence,
            &call,
            MemoryToolScope::Agent,
            vec!["Changed.".into()],
        )
        .await
        .unwrap();
    assert!(matches!(
        again,
        crate::application::workflow::memory_tools::MemorySaveOutcome::Accepted(receipt) if receipt == first
    ));
    let committed = t.committed().await;
    assert_eq!(committed.deliveries.len(), 1, "one intent");
    assert_eq!(committed.deliveries[0]["id"], first["intent"]);
    assert_eq!(
        committed.deliveries[0]["items"][0]["body"],
        "Prefers mornings."
    );
}

/// A service that refuses the credential (401) or the request (403) is a controlled tool error:
/// the invocation records it, nothing is raised or recorded as consumed, and it replays.
#[tokio::test]
async fn memory_integration_service_auth_refusals_are_controlled_and_raise_nothing() {
    let t = Tools::new(AccessLevel::L5).await;
    let company = t.ready(NamespaceOwner::Company).await;
    t.seed(
        company,
        3,
        "Concise replies",
        "The customer prefers concise replies.",
    );
    for status in [401_u16, 403] {
        t.fake.fault("observations/search", &[status]);
        let call = t
            .prepare(
                MemoryTool::Search,
                json!({"query":"concise replies","scopes":["company"]}),
            )
            .await;
        let result = t.call(copy(&call)).await;
        assert_eq!(error_code(&result), "memory.refused", "{status}");
        assert_eq!(t.invocation_result(&call).await, Some(result.clone()));
        assert_eq!(t.call(copy(&call)).await, result, "replayed");
        let committed = t.committed().await;
        assert_eq!(
            (committed.level, committed.consumptions.len()),
            (0, 0),
            "{status}"
        );
    }
    assert_eq!(
        t.service_calls("observations/search"),
        2,
        "one refused request each, never retried"
    );
}

/// The editor's built-in catalog, read through the production authoring read, lists the four
/// memory tools while the company has memory enabled and none once it is disabled.
#[tokio::test]
async fn memory_scope_authoring_catalog_lists_memory_tools_only_while_enabled() {
    use crate::application::agent::{
        AgentAuthoringReads, AgentPublicationScope, AgentToolKind, AgentToolPageRequest,
    };
    let t = Tools::new(AccessLevel::L5).await;
    let request = || AgentToolPageRequest {
        scope: AgentPublicationScope {
            company: t.company(),
            actor: t.s.fixture.target.actor,
            agent: crate::entities::agent_capability::AgentId::new(t.s.agent),
        },
        kind: AgentToolKind::Builtin,
        connection: None,
        after: None,
    };
    let names = |page: crate::application::agent::AgentToolPage| -> Vec<String> {
        page.items
            .into_iter()
            .map(|item| item.name.as_str().to_owned())
            .collect()
    };
    let enabled = names(t.p().tool_options(request()).await.unwrap());
    for tool in MemoryTool::ALL {
        assert!(
            enabled.iter().any(|name| name == tool.name()),
            "{}",
            tool.name()
        );
    }
    t.p()
        .disable_memory(t.company(), t.s.fixture.target.actor, 1)
        .await
        .unwrap();
    let disabled = names(t.p().tool_options(request()).await.unwrap());
    assert!(
        MemoryTool::ALL
            .iter()
            .all(|tool| !disabled.iter().any(|name| name == tool.name()))
    );
    assert_eq!(disabled.len() + 4, enabled.len());
}

impl Tools {
    /// The pausing runtime revalidating through the fixture's client, without fresh recall.
    fn pausing_tool_reads(&self) -> WorkflowAgentRuntime<PostgresPersistence, PreparedOnly> {
        let mut runtime = pausing(self);
        runtime.set_recall(Arc::new(MemoryRecall::new(
            Arc::new(ToolReadsOnly(self.p().clone())),
            self.client.clone(),
        )));
        runtime
    }
}

/// The production recall persistence with automatic recall switched off. The tool tests measure
/// tool reads alone (their consumptions, levels and service calls), while each later
/// preparation still revalidates what an earlier attempt's tool reads showed the model (E7).
/// Stand-in: in production a search agent also recalls at preparation (`recall` tests).
struct ToolReadsOnly(PostgresPersistence);

#[async_trait::async_trait]
impl crate::application::workflow::memory_recall::MemoryRecallPersistence for ToolReadsOnly {
    async fn recall_plan(
        &self,
        _claim: &ClaimedWorkflow,
        _policy: LeasePolicy,
    ) -> crate::app_error::AppResult<crate::application::workflow::memory_recall::RecallPlan> {
        Ok(crate::application::workflow::memory_recall::RecallPlan::NotApplicable)
    }

    async fn provision_recall_scope(
        &self,
        fence: crate::application::workflow::lease::WorkflowFence,
        scope: MemoryToolScope,
    ) -> crate::app_error::AppResult<()> {
        self.0.provision_recall_scope(fence, scope).await
    }

    async fn capture_targets(
        &self,
        _claim: &ClaimedWorkflow,
        _policy: LeasePolicy,
    ) -> crate::app_error::AppResult<Vec<MemoryToolScope>> {
        Ok(Vec::new())
    }

    async fn revalidation(
        &self,
        claim: &ClaimedWorkflow,
        policy: LeasePolicy,
    ) -> crate::app_error::AppResult<crate::application::workflow::memory_recall::Revalidation>
    {
        self.0.revalidation(claim, policy).await
    }
}

/// Automatic recall over the fixture's client and persistence (E7).
fn recall(t: &Tools) -> Arc<MemoryRecall> {
    Arc::new(MemoryRecall::new(Arc::new(t.p().clone()), t.client.clone()))
}

/// The agent runtime whose tools pause at the first model tool call (the attempt ends there),
/// with recall composed.
fn pausing(t: &Tools) -> WorkflowAgentRuntime<PostgresPersistence, PreparedOnly> {
    let mut runtime = WorkflowAgentRuntime::new(
        ProviderRegistry::standard().unwrap(),
        t.p().clone(),
        PreparedOnly,
        Some(notices(t.p())),
        lease(),
    );
    runtime.set_recall(recall(t));
    runtime
}
