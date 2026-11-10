//! A loopback fake of the memory service's admin surface for adapter and reconciler tests.
//!
//! It is held to the published contract: every request body is validated against its operation's
//! request schema in `services/memory/openapi.json`, and every 200 response against the response
//! schema, so a drift between this client and the contract fails the test that exercised it.
//! Tenant semantics follow decisions D3: create is idempotent for the managing admin and a uniform
//! 409 otherwise; delete answers 200 for the caller's own tombstone and 404 for a foreign tenant.
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FakeTenantState {
    Active,
    Disabled,
    Deleted,
}

#[derive(Default)]
pub struct FakeState {
    /// tenant → (state, managing admin token)
    pub tenants: HashMap<String, (FakeTenantState, String)>,
    /// (tenant, namespace) → active
    pub namespaces: HashMap<(String, String), bool>,
    /// credential id → (tenant, label, revoked)
    pub credentials: HashMap<Uuid, (String, String, bool)>,
    pub calls: Vec<String>,
    /// Admin tokens accepted besides the one the fake started with.
    pub admin_tokens: Vec<String>,
    /// (operation, `x-correlation-id`) of every call, in order.
    pub correlations: Vec<(String, Option<String>)>,
    /// operation → statuses to answer before the normal behaviour.
    pub faults: HashMap<String, Vec<u16>>,
    /// Operations whose next call is applied and then answered by dropping the connection.
    pub drops: Vec<String>,
    /// Every contract violation seen; a test fails on any (see `Drop`).
    pub violations: Vec<String>,
    /// One-shot: the next call of the operation waits this long before it is handled; the entry
    /// is removed when the wait starts.
    pub delays: HashMap<String, std::time::Duration>,
    /// Every call of the operation waits this long before it is handled (not one-shot).
    pub holds: HashMap<String, std::time::Duration>,
    /// Calls of a held operation being held right now, and the most ever held at once.
    pub held_now: HashMap<String, usize>,
    pub held_peak: HashMap<String, usize>,
    /// Client bearer token → (tenant, namespace) it is granted.
    pub client_tokens: HashMap<String, (String, String)>,
    /// Save idempotency key → (request, first answer); a replay answers `replayed: true`.
    pub saves: HashMap<String, (Value, Value)>,
    /// Saved observations: (namespace, `ObservationDetail` without `namespace`).
    pub observations: Vec<(String, Value)>,
    /// `TenantView.generation_daily_token_cap` the tenant endpoints answer (0 by default).
    pub generation_cap: u64,
    /// Generation idempotency key → (namespace, request, first answer); a replay answers
    /// `replayed: true` and a changed body `idempotency_conflict`.
    pub generations: HashMap<String, (String, Value, Value)>,
}

pub struct ContractFake {
    pub base_url: String,
    pub state: Arc<Mutex<FakeState>>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for ContractFake {
    /// The server task cannot fail a test, so violations are recorded there and reported here, on
    /// the test thread, unless the test is already failing.
    fn drop(&mut self) {
        self.server.abort();
        if !std::thread::panicking() {
            self.assert_clean();
        }
    }
}

struct Shared {
    state: Arc<Mutex<FakeState>>,
    admin_token: String,
    contract: Value,
}

impl ContractFake {
    pub async fn start(admin_token: &str) -> Self {
        let contract: Value = serde_json::from_str(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("services/memory/openapi.json"),
            )
            .expect("the service contract is readable"),
        )
        .expect("the service contract is JSON");
        let state = Arc::new(Mutex::new(FakeState::default()));
        let shared = Arc::new(Shared {
            state: state.clone(),
            admin_token: admin_token.to_owned(),
            contract,
        });
        let app = Router::new()
            .route("/admin/v1/{group}/{verb}", post(admin))
            .route("/v1/{group}/{verb}", post(client))
            .with_state(shared);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            server,
        }
    }

    /// A client token granted `namespace` in `tenant`, as an issued credential would be.
    pub fn client_token(&self, tenant: Uuid, namespace: Uuid) -> String {
        let token = format!("msk_cli_{}", Uuid::new_v4().simple());
        self.state.lock().unwrap().client_tokens.insert(
            token.clone(),
            (
                tenant.hyphenated().to_string(),
                namespace.hyphenated().to_string(),
            ),
        );
        token
    }

    /// Forget an issued credential and its token, as a service restored from a backup taken
    /// before it was issued would (p06 A4-8, A4-9): its token then answers 401.
    pub fn forget_credential(&self, id: Uuid) {
        let mut state = self.state.lock().unwrap();
        state.credentials.remove(&id);
        let prefix = format!("msk_cli_{}_", id.simple());
        state
            .client_tokens
            .retain(|token, _| !token.starts_with(&prefix));
    }

    /// Accept another provisioner token (a rotated credential, p06 A3-4).
    pub fn accept_admin(&self, token: &str) {
        self.state
            .lock()
            .unwrap()
            .admin_tokens
            .push(token.to_owned());
    }

    /// The operator's `tenants/reassign`: every tenant `from` manages, in any state, moves to
    /// `to`.
    pub fn reassign(&self, from: &str, to: &str) -> usize {
        let mut state = self.state.lock().unwrap();
        let mut moved = 0;
        for (_, owner) in state.tenants.values_mut() {
            if owner == from {
                *owner = to.to_owned();
                moved += 1;
            }
        }
        moved
    }

    pub fn foreign_tenant(&self, tenant: Uuid) {
        self.state.lock().unwrap().tenants.insert(
            tenant.hyphenated().to_string(),
            (FakeTenantState::Active, "someone-else".into()),
        );
    }

    pub fn fault(&self, operation: &str, statuses: &[u16]) {
        self.state
            .lock()
            .unwrap()
            .faults
            .insert(operation.into(), statuses.to_vec());
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().unwrap().calls.clone()
    }

    /// (operation, `x-correlation-id`) of every call, in order.
    pub fn correlations(&self) -> Vec<(String, Option<String>)> {
        self.state.lock().unwrap().correlations.clone()
    }

    /// The next `operation` is applied, then its connection is dropped before any answer.
    pub fn drop_after_write(&self, operation: &str) {
        self.state.lock().unwrap().drops.push(operation.into());
    }

    pub fn assert_clean(&self) {
        // A poisoned lock means a handler panicked mid-request: that is a failure, not clean.
        let violations = match self.state.lock() {
            Ok(state) => state.violations.clone(),
            Err(_) => vec!["the fake's state lock was poisoned by a panicking handler".into()],
        };
        assert!(
            violations.is_empty(),
            "the client violated the contract: {violations:?}"
        );
    }
}

fn validator(contract: &Value, schema: &Value) -> jsonschema::Validator {
    let mut root = schema.clone();
    root["components"] = contract["components"].clone();
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(&root)
        .expect("contract schema compiles")
}

fn check(contract: &Value, schema: &Value, instance: &Value, what: &str) -> Result<(), String> {
    let errors: Vec<String> = validator(contract, schema)
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{what} violates the contract: {errors:?} {instance}"
        ))
    }
}

fn error(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(json!({"error":{"code":code,"message":code,"retryable":false},"request_id":"fake"})),
    )
        .into_response()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Admin,
    Client,
}

async fn admin(
    State(shared): State<Arc<Shared>>,
    Path((group, verb)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    serve(
        shared,
        Surface::Admin,
        format!("{group}/{verb}"),
        headers,
        body,
    )
    .await
}

async fn client(
    State(shared): State<Arc<Shared>>,
    Path((group, verb)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    serve(
        shared,
        Surface::Client,
        format!("{group}/{verb}"),
        headers,
        body,
    )
    .await
}

async fn serve(
    shared: Arc<Shared>,
    surface: Surface,
    operation: String,
    headers: HeaderMap,
    body: Value,
) -> Response {
    let path = match surface {
        Surface::Admin => format!("/admin/v1/{operation}"),
        Surface::Client => format!("/v1/{operation}"),
    };
    let spec = &shared.contract["paths"][&path]["post"];
    let violation = |state: &mut FakeState, message: String| {
        state.violations.push(message);
        error(StatusCode::INTERNAL_SERVER_ERROR, "contract_violation")
    };
    // A one-shot delay, taken (and so observable as gone) before the request is handled.
    let delay = shared.state.lock().unwrap().delays.remove(&operation);
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
    let hold = {
        let mut state = shared.state.lock().unwrap();
        let hold = state.holds.get(&operation).copied();
        if hold.is_some() {
            let now = {
                let now = state.held_now.entry(operation.clone()).or_default();
                *now += 1;
                *now
            };
            let peak = state.held_peak.entry(operation.clone()).or_default();
            *peak = (*peak).max(now);
        }
        hold
    };
    if let Some(hold) = hold {
        tokio::time::sleep(hold).await;
        *shared
            .state
            .lock()
            .unwrap()
            .held_now
            .entry(operation.clone())
            .or_default() -= 1;
    }
    let mut state = shared.state.lock().unwrap();
    if !spec.is_object() {
        return violation(&mut state, format!("{path} is not in the contract"));
    }
    if let Err(message) = check(
        &shared.contract,
        &spec["requestBody"]["content"]["application/json"]["schema"],
        &body,
        &format!("{path} request"),
    ) {
        return violation(&mut state, message);
    }
    state.calls.push(operation.clone());
    // p06 A2-2: the correlation id each call carried.
    let correlation = headers
        .get("x-correlation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    state.correlations.push((operation.clone(), correlation));
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_owned();
    let granted = match surface {
        Surface::Admin => bearer == shared.admin_token || state.admin_tokens.contains(&bearer),
        Surface::Client => state.client_tokens.contains_key(&bearer),
    };
    if !granted {
        return error(StatusCode::UNAUTHORIZED, "unauthenticated");
    }
    if let Some(statuses) = state.faults.get_mut(&operation)
        && !statuses.is_empty()
    {
        let status = statuses.remove(0);
        let code = if status == 503 {
            "unavailable"
        } else {
            "injected"
        };
        return error(StatusCode::from_u16(status).unwrap(), code);
    }
    let answer = match surface {
        Surface::Admin => handle(&mut state, &operation, &body, &bearer),
        Surface::Client => handle_client(&mut state, &operation, &body, &bearer),
    };
    if let Some(position) = state.drops.iter().position(|drop| *drop == operation) {
        state.drops.remove(position);
        drop(state);
        // Unwinding without the panic hook ends this connection's task: the write is applied
        // and the client sees the connection close before any status line.
        std::panic::resume_unwind(Box::new("contract fake connection drop"));
    }
    match answer {
        Ok(value) => {
            if let Err(message) = check(
                &shared.contract,
                &spec["responses"]["200"]["content"]["application/json"]["schema"],
                &value,
                &format!("{path} response"),
            ) {
                return violation(&mut state, message);
            }
            (StatusCode::OK, Json(value)).into_response()
        }
        Err((status, code)) => error(status, code),
    }
}

fn tenant_view(tenant: &str, state: FakeTenantState, generation_cap: u64) -> Value {
    let state = match state {
        FakeTenantState::Active => "active",
        FakeTenantState::Disabled => "disabled",
        FakeTenantState::Deleted => "deleted",
    };
    json!({"tenant":tenant,"state":state,"generation_daily_token_cap":generation_cap,"cancelled_operations":0})
}

type Answer = Result<Value, (StatusCode, &'static str)>;

/// The client surface the adapter uses: an idempotent save (D5) labelled per D4.
fn handle_client(state: &mut FakeState, operation: &str, body: &Value, token: &str) -> Answer {
    let (tenant, namespace) = state.client_tokens[token].clone();
    let granted =
        body["namespace"] == json!(namespace) || body["namespaces"] == json!([namespace.clone()]);
    if !granted {
        return Err((StatusCode::FORBIDDEN, "forbidden"));
    }
    require_active(
        state
            .tenants
            .get(&tenant)
            .map(|(tenant, _)| *tenant)
            .unwrap_or(FakeTenantState::Active),
    )?;
    match operation {
        "observations/save" => {
            let key = body["idempotency_key"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if let Some((request, answer)) = state.saves.get(&key) {
                if request != body {
                    return Err((StatusCode::CONFLICT, "idempotency_conflict"));
                }
                let mut replay = answer.clone();
                replay["replayed"] = json!(true);
                return Ok(replay);
            }
            let context = &body["context"];
            let ceiling = context["producing_ceiling"].as_u64().unwrap_or(0);
            let consumed = match context["consumed"]["kind"].as_str() {
                Some("exact") => context["consumed"]["level"].as_u64().unwrap_or(0),
                _ => ceiling,
            };
            let level = context["external_sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|source| source["access_level"].as_u64())
                .fold(consumed, u64::max);
            if level > ceiling {
                return Err((StatusCode::UNPROCESSABLE_ENTITY, "classification_refused"));
            }
            let mut observations = Vec::new();
            for item in body["items"].as_array().into_iter().flatten() {
                let id = Uuid::new_v4();
                observations.push(json!({"observation_id":id,"revision":1,"access_level":level,"index_state":"pending"}));
                state.observations.push((
                    namespace.clone(),
                    json!({"observation_id":id,"kind":item["kind"],"title":item["title"],
                        "body":item["body"],"occurred_at":item["occurred_at"],"access_level":level,
                        "revision":1,"content_sha256":"0".repeat(64),"sources":[],"index_state":"pending"}),
                ));
            }
            let answer = json!({"observations":observations,"replayed":false});
            state.saves.insert(key, (body.clone(), answer.clone()));
            Ok(answer)
        }
        "observations/search" => Ok(search(state, &namespace, body)),
        "observations/fetch" => Ok(fetch(state, &namespace, body)),
        "generations/request" => {
            let key = body["idempotency_key"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if let Some((_, request, answer)) = state.generations.get(&key) {
                if request != body {
                    return Err((StatusCode::CONFLICT, "idempotency_conflict"));
                }
                let mut replay = answer.clone();
                replay["replayed"] = json!(true);
                return Ok(replay);
            }
            if state.generation_cap == 0 {
                return Err((StatusCode::FORBIDDEN, "generation_disabled"));
            }
            // The service's window rule: one (source_ref, revision) carries one text and level.
            let mut sources = std::collections::HashMap::new();
            for event in body["events"].as_array().into_iter().flatten() {
                let source = (event["source_ref"].clone(), event["revision"].clone());
                let content = (event["text"].clone(), event["access_level"].clone());
                if sources
                    .insert(source, content.clone())
                    .is_some_and(|seen| seen != content)
                {
                    return Err((StatusCode::CONFLICT, "conflict"));
                }
            }
            let ceiling = body["producing_ceiling"].as_u64().unwrap_or(0);
            let above = body["events"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|event| event["access_level"].as_u64().unwrap_or(0) > ceiling);
            if above {
                return Err((StatusCode::UNPROCESSABLE_ENTITY, "classification_refused"));
            }
            let answer = json!({"operation_id":Uuid::new_v4(),"state":"accepted","outcome":null,
                "observations":[],"replayed":false});
            state
                .generations
                .insert(key, (namespace.clone(), body.clone(), answer.clone()));
            Ok(answer)
        }
        "observations/timeline" => timeline(state, &namespace, body),
        "context/pack" => Ok(pack(state, &namespace, body)),
        "context/validate" => Ok(validate(state, &namespace, body)),
        _ => Err((StatusCode::NOT_FOUND, "not_found")),
    }
}

/// The service's `require_active`: a disabled tenant is 403, a deleted one 409.
fn require_active(state: FakeTenantState) -> Result<(), (StatusCode, &'static str)> {
    match state {
        FakeTenantState::Active => Ok(()),
        FakeTenantState::Disabled => Err((StatusCode::FORBIDDEN, "tenant_disabled")),
        FakeTenantState::Deleted => Err((StatusCode::CONFLICT, "conflict")),
    }
}

fn handle(state: &mut FakeState, operation: &str, body: &Value, admin: &str) -> Answer {
    let text = |key: &str| body[key].as_str().unwrap_or_default().to_owned();
    let managed =
        |state: &FakeState, tenant: &str| -> Result<FakeTenantState, (StatusCode, &'static str)> {
            match state.tenants.get(tenant) {
                Some((current, owner)) if owner == admin => Ok(*current),
                _ => Err((StatusCode::NOT_FOUND, "not_found")),
            }
        };
    match operation {
        "tenants/create" => {
            let tenant = text("external_id");
            match state.tenants.get(&tenant) {
                Some((current, owner))
                    if owner == admin && *current != FakeTenantState::Deleted =>
                {
                    Ok(tenant_view(&tenant, *current, state.generation_cap))
                }
                Some(_) => Err((StatusCode::CONFLICT, "conflict")),
                None => {
                    state
                        .tenants
                        .insert(tenant.clone(), (FakeTenantState::Active, admin.to_owned()));
                    Ok(tenant_view(
                        &tenant,
                        FakeTenantState::Active,
                        state.generation_cap,
                    ))
                }
            }
        }
        "tenants/disable" | "tenants/enable" | "tenants/delete" => {
            let tenant = text("tenant");
            let current = managed(state, &tenant)?;
            let next = match (operation, current) {
                ("tenants/delete", _) => FakeTenantState::Deleted,
                (_, FakeTenantState::Deleted) => return Err((StatusCode::CONFLICT, "conflict")),
                ("tenants/disable", _) => FakeTenantState::Disabled,
                _ => FakeTenantState::Active,
            };
            // As the service: a delete leaves credentials unrevoked; the tombstone makes them
            // unusable.
            state.tenants.get_mut(&tenant).unwrap().0 = next;
            Ok(tenant_view(&tenant, next, state.generation_cap))
        }
        "namespaces/create" => {
            let (tenant, namespace) = (text("tenant"), text("namespace"));
            require_active(managed(state, &tenant)?)?;
            let key = (tenant.clone(), namespace.clone());
            if state.namespaces.get(&key) == Some(&false) {
                return Err((StatusCode::CONFLICT, "namespace_deleted"));
            }
            state.namespaces.insert(key, true);
            Ok(json!({"tenant":tenant,"namespace":namespace,"state":"active"}))
        }
        "namespaces/delete" => {
            let (tenant, namespace) = (text("tenant"), text("namespace"));
            managed(state, &tenant)?;
            let key = (tenant.clone(), namespace.clone());
            // A namespace whose create never landed is not found and leaves no tombstone.
            let Some(active) = state.namespaces.get_mut(&key) else {
                return Err((StatusCode::NOT_FOUND, "not_found"));
            };
            *active = false;
            Ok(json!({"tenant":tenant,"namespace":namespace,"state":"deleted"}))
        }
        "credentials/issue" => {
            let tenant = text("tenant");
            require_active(managed(state, &tenant)?)?;
            let namespace = body["grants"][0]["namespace"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            if state.namespaces.get(&(tenant.clone(), namespace.clone())) != Some(&true) {
                return Err((StatusCode::BAD_REQUEST, "invalid_request"));
            }
            let id = Uuid::new_v4();
            state
                .credentials
                .insert(id, (tenant.clone(), text("label"), false));
            let token = format!("msk_cli_{}_{}", id.simple(), "B".repeat(43));
            // As the service: the issued token authenticates for its namespace.
            state
                .client_tokens
                .insert(token.clone(), (tenant, namespace));
            Ok(json!({"credential_id":id,"token":token}))
        }
        "credentials/list" => {
            // As the service (p06 A1-6): any state its manager manages, an exact label filter,
            // `active_only`, and pages in id order with `next_after` on a full page.
            let tenant = text("tenant");
            managed(state, &tenant)?;
            let label = body["label"].as_str();
            let active_only = body["active_only"].as_bool().unwrap_or(false);
            let after = body["after"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok());
            let limit = body["limit"].as_u64().unwrap_or(1000);
            if !(1..=1000).contains(&limit) {
                return Err((StatusCode::BAD_REQUEST, "invalid_request"));
            }
            let mut matching: Vec<(&Uuid, &(String, String, bool))> = state
                .credentials
                .iter()
                .filter(|(id, (owner, name, revoked))| {
                    *owner == tenant
                        && label.is_none_or(|label| label == name)
                        && !(active_only && *revoked)
                        && after.is_none_or(|after| **id > after)
                })
                .collect();
            matching.sort_by_key(|(id, _)| **id);
            matching.truncate(limit as usize);
            let next_after = (matching.len() == limit as usize)
                .then(|| matching.last().map(|(id, _)| **id))
                .flatten();
            let credentials: Vec<Value> = matching
                .into_iter()
                .map(|(id, (_, label, revoked))| {
                    json!({"tenant":tenant,"credential_id":id,"label":label,"grants":[],
                        "revoked_at": revoked.then(|| "2026-11-08T00:00:00Z")})
                })
                .collect();
            Ok(json!({"credentials":credentials,"next_after":next_after}))
        }
        "credentials/revoke" => {
            let tenant = text("tenant");
            managed(state, &tenant)?;
            let id = Uuid::parse_str(&text("credential_id"))
                .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_request"))?;
            let Some(credential) = state.credentials.get_mut(&id).filter(|c| c.0 == tenant) else {
                return Err((StatusCode::NOT_FOUND, "not_found"));
            };
            credential.2 = true;
            let label = credential.1.clone();
            let prefix = format!("msk_cli_{}_", id.simple());
            state
                .client_tokens
                .retain(|token, _| !token.starts_with(&prefix));
            Ok(
                json!({"tenant":tenant,"credential_id":id,"label":label,"grants":[],"revoked_at":"2026-11-08T00:00:00Z"}),
            )
        }
        _ => Err((StatusCode::NOT_FOUND, "not_found")),
    }
}

/// Keyword search within the namespace and ceiling: every query word in the title or body.
fn search(state: &FakeState, namespace: &str, body: &Value) -> Value {
    let ceiling = body["read_ceiling"].as_u64().unwrap_or(0);
    let words: Vec<String> = body["query"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let limit = body["limit"].as_u64().unwrap_or(20) as usize;
    let items: Vec<Value> = state
        .observations
        .iter()
        .filter(|(owner, detail)| {
            let text = format!("{} {}", detail["title"], detail["body"]).to_lowercase();
            owner == namespace
                && detail["access_level"].as_u64().unwrap_or(0) <= ceiling
                && !words.is_empty()
                && words.iter().all(|word| text.contains(word))
        })
        .take(limit)
        .map(|(owner, detail)| {
            let snippet: String = detail["body"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
            json!({"observation_id":detail["observation_id"],"namespace":owner,"kind":detail["kind"],
                "title":detail["title"],"snippet":snippet,"occurred_at":detail["occurred_at"],
                "access_level":detail["access_level"],"revision":detail["revision"],"sources":[],
                "index_state":"pending"})
        })
        .collect();
    let exposed = items
        .iter()
        .filter_map(|item| item["access_level"].as_u64())
        .max()
        .unwrap_or(0);
    json!({"items":items,"exposed_level":exposed,"semantic":"complete"})
}

fn sha256_hex(value: &Value) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(value.to_string().as_bytes()))
}

/// A compact pack: candidates sharing any word of three or more letters with the query (a
/// stand-in for the service's ranked hybrid search), or every visible item for recency; newest
/// first, each whole item included while it fits the byte and token budget (64 + 2 × bytes).
fn pack(state: &FakeState, namespace: &str, body: &Value) -> Value {
    let ceiling = body["read_ceiling"].as_u64().unwrap_or(0);
    let words: Vec<String> = body["query"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_owned)
        .collect();
    let max_tokens = body["max_tokens"].as_u64().unwrap_or(8192);
    let max_bytes = body["max_bytes"].as_u64().unwrap_or(65_536);
    let mut candidates: Vec<&Value> = state
        .observations
        .iter()
        .filter(|(owner, detail)| {
            let text = format!("{} {}", detail["title"], detail["body"]).to_lowercase();
            owner == namespace
                && detail["access_level"].as_u64().unwrap_or(0) <= ceiling
                && (words.is_empty() || words.iter().any(|word| text.contains(word)))
        })
        .map(|(_, detail)| detail)
        .collect();
    candidates.sort_by(|a, b| {
        b["occurred_at"]
            .as_str()
            .cmp(&a["occurred_at"].as_str())
            .then(
                a["observation_id"]
                    .as_str()
                    .cmp(&b["observation_id"].as_str()),
            )
    });
    let (mut items, mut bytes, mut omitted) = (Vec::new(), 0_u64, 0_u64);
    for detail in candidates.into_iter().take(200) {
        let text: String = detail["body"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(80)
            .collect();
        let item = json!({"observation_id":detail["observation_id"],"namespace":namespace,
            "kind":detail["kind"],"title":detail["title"],"text":text,
            "occurred_at":detail["occurred_at"],"access_level":detail["access_level"],
            "revision":detail["revision"],"content_sha256":detail["content_sha256"],
            "sources":[],"sources_sha256":sha256_hex(&json!([]))});
        let size = item.to_string().len() as u64;
        if bytes + size <= max_bytes && 64 + 2 * (bytes + size) <= max_tokens {
            bytes += size;
            items.push(item);
        } else {
            omitted += 1;
        }
    }
    let exposed = items
        .iter()
        .filter_map(|item| item["access_level"].as_u64())
        .max()
        .unwrap_or(0);
    let tokens = if items.is_empty() { 0 } else { 64 + 2 * bytes };
    json!({"items":items,"exposed_level":exposed,"token_count":tokens,"byte_count":bytes,
        "omitted":{"budget":omitted},"candidates_truncated":false,
        "pack_digest":sha256_hex(&Value::Array(items.clone())),
        "generated_at":chrono::Utc::now().to_rfc3339(),"semantic":"complete","valid_until":null})
}

/// Each frozen item's status: unavailable when gone or above the ceiling, expired when marked,
/// changed at another revision or provenance, else valid.
fn validate(state: &FakeState, namespace: &str, body: &Value) -> Value {
    let ceiling = body["read_ceiling"].as_u64().unwrap_or(0);
    let mut exposed = 0;
    let items: Vec<Value> = body["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let found = state.observations.iter().find(|(owner, detail)| {
                owner == namespace && detail["observation_id"] == item["observation_id"]
            });
            let status = match found {
                Some((_, detail)) if detail["access_level"].as_u64().unwrap_or(0) <= ceiling => {
                    exposed = exposed.max(detail["access_level"].as_u64().unwrap_or(0));
                    if detail["expired"] == json!(true) {
                        "expired"
                    } else if detail["revision"] != item["revision"]
                        || item["sources_sha256"] != json!(sha256_hex(&json!([])))
                    {
                        "changed"
                    } else {
                        "valid"
                    }
                }
                _ => "unavailable",
            };
            json!({"observation_id":item["observation_id"],"status":status})
        })
        .collect();
    json!({"items":items,"exposed_level":exposed,"checked_at":chrono::Utc::now().to_rfc3339()})
}

/// The anchor and its nearest visible neighbours in the namespace, oldest first by
/// (`occurred_at`, id); an anchor the reader may not see is 404 like a missing one.
fn timeline(state: &FakeState, namespace: &str, body: &Value) -> Answer {
    let ceiling = body["read_ceiling"].as_u64().unwrap_or(0);
    let side = |key: &str| body[key].as_u64().unwrap_or(5) as usize;
    let mut visible: Vec<&Value> = state
        .observations
        .iter()
        .filter(|(owner, detail)| {
            owner == namespace && detail["access_level"].as_u64().unwrap_or(0) <= ceiling
        })
        .map(|(_, detail)| detail)
        .collect();
    visible.sort_by_key(|detail| {
        (
            detail["occurred_at"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            detail["observation_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    });
    let Some(at) = visible
        .iter()
        .position(|detail| detail["observation_id"] == body["anchor"])
    else {
        return Err((StatusCode::NOT_FOUND, "not_found"));
    };
    let from = at.saturating_sub(side("before"));
    let to = (at + side("after") + 1).min(visible.len());
    let items: Vec<Value> = visible[from..to]
        .iter()
        .map(|detail| {
            let snippet: String = detail["body"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(80)
                .collect();
            json!({"observation_id":detail["observation_id"],"namespace":namespace,"kind":detail["kind"],
                "title":detail["title"],"snippet":snippet,"occurred_at":detail["occurred_at"],
                "access_level":detail["access_level"],"revision":detail["revision"],"sources":[],
                "index_state":"pending"})
        })
        .collect();
    let exposed = items
        .iter()
        .filter_map(|item| item["access_level"].as_u64())
        .max()
        .unwrap_or(0);
    Ok(json!({"items":items,"exposed_level":exposed}))
}

/// Whole bodies of the requested ids within the namespace and ceiling; the rest unavailable.
fn fetch(state: &FakeState, namespace: &str, body: &Value) -> Value {
    let ceiling = body["read_ceiling"].as_u64().unwrap_or(0);
    let mut items = Vec::new();
    let mut unavailable = Vec::new();
    for id in body["ids"].as_array().cloned().unwrap_or_default() {
        match state.observations.iter().find(|(owner, detail)| {
            owner == namespace
                && detail["observation_id"] == id
                && detail["access_level"].as_u64().unwrap_or(0) <= ceiling
        }) {
            Some((owner, detail)) => {
                let mut detail = detail.clone();
                detail["namespace"] = json!(owner);
                items.push(detail);
            }
            None => unavailable.push(id),
        }
    }
    let exposed = items
        .iter()
        .filter_map(|item| item["access_level"].as_u64())
        .max()
        .unwrap_or(0);
    json!({"items":items,"unavailable":unavailable,"omitted_for_budget":[],"exposed_level":exposed})
}
