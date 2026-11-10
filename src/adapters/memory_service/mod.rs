//! HTTP client of the standalone memory service (phase 05, decision E1). It speaks only the
//! service's published contract (`services/memory/openapi.json`): no shared crate, no SQL pool.
//! No redirects, no proxy, no retries, bounded bodies, and tokens only in sensitive headers.
mod limits;

pub use limits::{MemoryPool, MemoryPools};

use crate::application::memory::*;
use crate::infra::memory_config::MemoryServiceConfig;
use async_trait::async_trait;
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;

/// The service's largest body (4 MiB on save) plus framing; nothing larger is read (D6).
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024 + 64 * 1024;

pub struct MemoryServiceHttp {
    client: reqwest::Client,
    base_url: String,
    provisioner: SecretString,
    request_timeout: Duration,
    recall_deadline: Duration,
    pools: MemoryPools,
}

impl MemoryServiceHttp {
    pub fn new(config: &MemoryServiceConfig) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5).min(config.request_timeout))
                .timeout(config.request_timeout)
                .pool_max_idle_per_host(8)
                .pool_idle_timeout(Duration::from_secs(30))
                .build()?,
            base_url: config.base_url.clone(),
            provisioner: config.provisioner.secret().clone(),
            request_timeout: config.request_timeout,
            recall_deadline: config.recall_deadline,
            pools: MemoryPools::new(config.interactive_in_flight, config.background_in_flight),
        })
    }

    /// One admin call on the background pool, bounded by the configured timeout.
    async fn admin<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        company: Uuid,
        operation: &'static str,
        body: &Req,
    ) -> Result<Resp, MemoryServiceError> {
        let deadline = Instant::now() + self.request_timeout;
        let _permit = self
            .pools
            .acquire(MemoryPool::Background, company, deadline)
            .await?;
        let url = format!("{}/admin/v1/{operation}", self.base_url);
        let value = tokio::time::timeout_at(deadline, self.post(&url, &self.provisioner, body))
            .await
            .map_err(|_| MemoryServiceError::Ambiguous)??;
        serde_json::from_value(value).map_err(|_| MemoryServiceError::Protocol)
    }

    async fn post<Req: Serialize>(
        &self,
        url: &str,
        token: &SecretString,
        body: &Req,
    ) -> Result<Value, MemoryServiceError> {
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
                .map_err(|_| MemoryServiceError::Protocol)?;
        authorization.set_sensitive(true);
        let mut request = self
            .client
            .post(url)
            .header(reqwest::header::AUTHORIZATION, authorization);
        // p06 A2-2: the scope's correlation id, which the service logs and audits.
        if let Some(correlation) = current_correlation() {
            request = request.header("x-correlation-id", correlation);
        }
        let response = request.json(body).send().await.map_err(transport)?;
        let status = response.status().as_u16();
        let retry_after = retry_after(response.headers());
        let body = read_bounded(response).await?;
        if status == 200 {
            return serde_json::from_slice(&body).map_err(|_| MemoryServiceError::Protocol);
        }
        Err(classify(status, &body, retry_after))
    }
}

async fn read_bounded(response: reqwest::Response) -> Result<Vec<u8>, MemoryServiceError> {
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
    {
        return Err(MemoryServiceError::Protocol);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        // The status line arrived, so the request was received and may have been acted on.
        let chunk = chunk.map_err(|_| MemoryServiceError::Ambiguous)?;
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(MemoryServiceError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Only a connect failure (a connect timeout included) proves nothing was sent. reqwest reports
/// every hyper call error after that, a reset after the request was written included, as a
/// request error, so anything else after `send()` may have run.
fn transport(error: reqwest::Error) -> MemoryServiceError {
    if error.is_connect() {
        MemoryServiceError::Unavailable
    } else if error.is_builder() {
        MemoryServiceError::Protocol
    } else {
        MemoryServiceError::Ambiguous
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorPayload,
}
#[derive(Deserialize)]
struct ErrorPayload {
    code: String,
    #[serde(default)]
    details: Value,
}

/// The D6 error taxonomy, classified once. A status is definite only where the service itself
/// says so: a 503 is `Unavailable` only with the service's `unavailable` envelope (a proxy's 503
/// can follow an executed request), and 500, 502, a bare 503, 504 or any status outside the
/// taxonomy may have run, so they are `Ambiguous`. 400 and 413 are this client's own error.
pub(crate) fn classify(status: u16, body: &[u8], retry_after: Option<u64>) -> MemoryServiceError {
    let payload = serde_json::from_slice::<ErrorBody>(body)
        .ok()
        .map(|b| b.error);
    let code = || {
        payload
            .as_ref()
            .map_or_else(|| "unknown".to_owned(), |p| p.code.clone())
    };
    let code_is = |expected: &str| payload.as_ref().is_some_and(|p| p.code == expected);
    match status {
        401 => MemoryServiceError::Unauthorized,
        403 => MemoryServiceError::Forbidden { code: code() },
        404 => MemoryServiceError::NotFound,
        409 => MemoryServiceError::Conflict { code: code() },
        410 => MemoryServiceError::Expired,
        422 => MemoryServiceError::Refused {
            reason: payload
                .as_ref()
                .and_then(|p| p.details.get("reason"))
                .and_then(Value::as_str)
                .map_or_else(code, str::to_owned),
        },
        // A reservation above the whole cap is a terminal refusal, not back-pressure (D6).
        429 if code_is("spend_exhausted") => MemoryServiceError::Refused {
            reason: "spend_exhausted".into(),
        },
        429 => MemoryServiceError::Capacity {
            retry_after_secs: retry_after,
        },
        503 if code_is("unavailable") => MemoryServiceError::Unavailable,
        400 | 413 => MemoryServiceError::Protocol,
        _ => MemoryServiceError::Ambiguous,
    }
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.min(300))
}

/// Wire states stay adapter-private and are mapped into the port's enums, so the port never
/// derives its shape from the wire.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireTenantState {
    Active,
    Disabled,
    Deleted,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireNamespaceState {
    Active,
    Deleted,
}
#[derive(Deserialize)]
struct TenantView {
    state: WireTenantState,
    generation_daily_token_cap: u64,
}
impl From<TenantView> for RemoteTenant {
    fn from(view: TenantView) -> Self {
        Self {
            state: match view.state {
                WireTenantState::Active => RemoteTenantState::Active,
                WireTenantState::Disabled => RemoteTenantState::Disabled,
                WireTenantState::Deleted => RemoteTenantState::Deleted,
            },
            generation_daily_token_cap: view.generation_daily_token_cap,
        }
    }
}
#[derive(Deserialize)]
struct NamespaceView {
    state: WireNamespaceState,
}
impl NamespaceView {
    fn state(self) -> RemoteNamespaceState {
        match self.state {
            WireNamespaceState::Active => RemoteNamespaceState::Active,
            WireNamespaceState::Deleted => RemoteNamespaceState::Deleted,
        }
    }
}
#[derive(Deserialize)]
struct IssuedCredential {
    credential_id: Uuid,
    token: String,
}
#[derive(Deserialize)]
struct CredentialList {
    credentials: Vec<CredentialView>,
    #[serde(default)]
    next_after: Option<Uuid>,
}
#[derive(Deserialize)]
struct CredentialView {
    credential_id: Uuid,
    label: String,
    #[serde(default)]
    revoked_at: Option<String>,
}

impl MemoryServiceHttp {
    /// One client call through a namespace's credential on `pool`, bounded by the timeout.
    async fn client<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        pool: MemoryPool,
        credential: &ClientCredential,
        operation: &'static str,
        body: &Req,
    ) -> Result<Resp, MemoryServiceError> {
        let deadline = Instant::now() + self.request_timeout;
        let _permit = self
            .pools
            .acquire(pool, credential.tenant.0, deadline)
            .await?;
        self.send(credential, operation, body, deadline).await
    }

    /// One request under a permit the caller holds, bounded by `deadline`.
    async fn send<Req: Serialize, Resp: DeserializeOwned>(
        &self,
        credential: &ClientCredential,
        operation: &'static str,
        body: &Req,
        deadline: Instant,
    ) -> Result<Resp, MemoryServiceError> {
        let url = format!("{}/v1/{operation}", self.base_url);
        let value = tokio::time::timeout_at(deadline, self.post(&url, &credential.token, body))
            .await
            .map_err(|_| MemoryServiceError::Ambiguous)??;
        serde_json::from_value(value).map_err(|_| MemoryServiceError::Protocol)
    }

    #[cfg(test)]
    pub(crate) fn interactive_in_use(&self, company: Uuid) -> usize {
        self.pools.in_use(MemoryPool::Interactive, company)
    }
}

fn search_body(search: &MemorySearch) -> Value {
    json!({
        "namespaces": [search.namespace.0.hyphenated().to_string()],
        "read_ceiling": search.read_ceiling,
        "query": search.query,
        "limit": search.limit,
    })
}

#[derive(Deserialize)]
struct WireSaved {
    observation_id: Uuid,
    revision: i64,
    access_level: u8,
}
#[derive(Deserialize)]
struct WireSaveResponse {
    observations: Vec<WireSaved>,
    replayed: bool,
}

/// `GenerationStatus`; observations and outcome are read later through status, not here.
#[derive(Deserialize)]
struct WireGenerationStatus {
    operation_id: Uuid,
    state: String,
    replayed: bool,
}

#[derive(Deserialize)]
struct WireHit {
    observation_id: Uuid,
    kind: String,
    title: String,
    snippet: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    access_level: u8,
    revision: i64,
}
#[derive(Deserialize)]
struct WireSearchResponse {
    items: Vec<WireHit>,
    exposed_level: u8,
}

#[derive(Deserialize)]
struct WirePackItem {
    observation_id: Uuid,
    kind: String,
    title: String,
    text: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    access_level: u8,
    revision: i64,
    content_sha256: String,
    sources_sha256: String,
}
#[derive(Deserialize)]
struct WirePack {
    items: Vec<WirePackItem>,
    exposed_level: u8,
    pack_digest: String,
    token_count: u32,
    #[serde(default)]
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    omitted: WireOmitted,
}
#[derive(Deserialize, Default)]
struct WireOmitted {
    #[serde(default)]
    budget: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WirePackStatus {
    Valid,
    Changed,
    Expired,
    Unavailable,
}
#[derive(Deserialize)]
struct WireValidated {
    observation_id: Uuid,
    status: WirePackStatus,
}
#[derive(Deserialize)]
struct WireValidate {
    items: Vec<WireValidated>,
}

/// A pack, refused whole if anything is above the ceiling (as `hits`).
fn pack(answer: WirePack, ceiling: u8) -> Result<MemoryPack, MemoryServiceError> {
    if answer.items.iter().any(|item| item.access_level > ceiling) || answer.exposed_level > ceiling
    {
        return Err(MemoryServiceError::Protocol);
    }
    Ok(MemoryPack {
        items: answer
            .items
            .into_iter()
            .map(|item| MemoryPackItem {
                observation_id: item.observation_id,
                kind: item.kind,
                title: item.title,
                text: item.text,
                occurred_at: item.occurred_at,
                access_level: item.access_level,
                revision: item.revision,
                content_sha256: item.content_sha256,
                sources_sha256: item.sources_sha256,
            })
            .collect(),
        exposed_level: answer.exposed_level,
        pack_digest: answer.pack_digest,
        token_count: answer.token_count,
        valid_until: answer.valid_until,
        omitted: answer.omitted.budget,
    })
}

/// An index answer (search or timeline), refused whole if anything is above the ceiling.
fn hits(answer: WireSearchResponse, ceiling: u8) -> Result<MemorySearchResult, MemoryServiceError> {
    if answer.items.iter().any(|hit| hit.access_level > ceiling) || answer.exposed_level > ceiling {
        // The service never answers above the ceiling; if it did, nothing is trusted.
        return Err(MemoryServiceError::Protocol);
    }
    Ok(MemorySearchResult {
        items: answer
            .items
            .into_iter()
            .map(|hit| MemoryHit {
                observation_id: hit.observation_id,
                kind: hit.kind,
                title: hit.title,
                snippet: hit.snippet,
                occurred_at: hit.occurred_at,
                access_level: hit.access_level,
                revision: hit.revision,
            })
            .collect(),
        exposed_level: answer.exposed_level,
    })
}
#[derive(Deserialize)]
struct WireDetail {
    observation_id: Uuid,
    kind: String,
    title: String,
    body: String,
    occurred_at: chrono::DateTime<chrono::Utc>,
    access_level: u8,
    revision: i64,
    content_sha256: String,
}
#[derive(Deserialize)]
struct WireFetchResponse {
    items: Vec<WireDetail>,
    exposed_level: u8,
}

#[async_trait]
impl MemoryClient for MemoryServiceHttp {
    async fn search(
        &self,
        credential: &ClientCredential,
        search: &MemorySearch,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        let answer: WireSearchResponse = self
            .client(
                MemoryPool::Interactive,
                credential,
                "observations/search",
                &search_body(search),
            )
            .await?;
        hits(answer, search.read_ceiling)
    }

    async fn search_scopes(
        &self,
        searches: &[ScopedSearch<'_>],
    ) -> Result<Vec<MemorySearchResult>, MemoryServiceError> {
        let Some(first) = searches.first() else {
            return Ok(Vec::new());
        };
        if searches
            .iter()
            .any(|entry| entry.credential.tenant != first.credential.tenant)
        {
            return Err(MemoryServiceError::Protocol);
        }
        let deadline = Instant::now() + self.request_timeout;
        let _permit = self
            .pools
            .acquire(MemoryPool::Interactive, first.credential.tenant.0, deadline)
            .await?;
        let answers = futures::future::join_all(searches.iter().map(|entry| async move {
            let answer: WireSearchResponse = self
                .send(
                    entry.credential,
                    "observations/search",
                    &search_body(&entry.search),
                    deadline,
                )
                .await?;
            hits(answer, entry.search.read_ceiling)
        }))
        .await;
        answers.into_iter().collect()
    }

    async fn context_pack(
        &self,
        packs: &[ScopedPack<'_>],
    ) -> Result<Vec<MemoryPack>, MemoryServiceError> {
        let Some(first) = packs.first() else {
            return Ok(Vec::new());
        };
        if packs
            .iter()
            .any(|entry| entry.credential.tenant != first.credential.tenant)
        {
            return Err(MemoryServiceError::Protocol);
        }
        // The whole recall, permit wait included, stays within the recall deadline (E1).
        let deadline = Instant::now() + self.recall_deadline.min(self.request_timeout);
        let _permit = self
            .pools
            .acquire(MemoryPool::Interactive, first.credential.tenant.0, deadline)
            .await?;
        let answers = futures::future::join_all(packs.iter().map(|entry| async move {
            let body = json!({
                "namespaces": [entry.pack.namespace.0.hyphenated().to_string()],
                "read_ceiling": entry.pack.read_ceiling,
                "query": entry.pack.query,
                "max_tokens": entry.pack.max_tokens,
                "max_bytes": entry.pack.max_bytes,
                "detail": "compact",
            });
            let answer: WirePack = self
                .send(entry.credential, "context/pack", &body, deadline)
                .await?;
            pack(answer, entry.pack.read_ceiling)
        }))
        .await;
        answers.into_iter().collect()
    }

    async fn context_validate(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        items: &[PackedRevision],
    ) -> Result<Vec<PackItemStatus>, MemoryServiceError> {
        let body = json!({
            "namespaces": [namespace.0.hyphenated().to_string()],
            "read_ceiling": read_ceiling,
            "items": items.iter().map(|item| json!({"observation_id": item.observation_id,
                "revision": item.revision, "sources_sha256": item.sources_sha256}))
                .collect::<Vec<_>>(),
        });
        let answer: WireValidate = self
            .client(
                MemoryPool::Interactive,
                credential,
                "context/validate",
                &body,
            )
            .await?;
        if answer.items.len() != items.len()
            || answer
                .items
                .iter()
                .zip(items)
                .any(|(status, item)| status.observation_id != item.observation_id)
        {
            return Err(MemoryServiceError::Protocol);
        }
        Ok(answer
            .items
            .into_iter()
            .map(|item| match item.status {
                WirePackStatus::Valid => PackItemStatus::Valid,
                WirePackStatus::Changed => PackItemStatus::Changed,
                WirePackStatus::Expired => PackItemStatus::Expired,
                WirePackStatus::Unavailable => PackItemStatus::Unavailable,
            })
            .collect())
    }

    async fn timeline(
        &self,
        credential: &ClientCredential,
        timeline: &MemoryTimeline,
    ) -> Result<MemorySearchResult, MemoryServiceError> {
        let body = json!({
            "namespaces": [timeline.namespace.0.hyphenated().to_string()],
            "read_ceiling": timeline.read_ceiling,
            "anchor": timeline.anchor,
            "before": timeline.before,
            "after": timeline.after,
        });
        let answer: WireSearchResponse = self
            .client(
                MemoryPool::Interactive,
                credential,
                "observations/timeline",
                &body,
            )
            .await?;
        hits(answer, timeline.read_ceiling)
    }

    async fn fetch(
        &self,
        credential: &ClientCredential,
        namespace: NamespaceExternalId,
        read_ceiling: u8,
        ids: &[Uuid],
    ) -> Result<MemoryFetchResult, MemoryServiceError> {
        let body = json!({
            "namespaces": [namespace.0.hyphenated().to_string()],
            "read_ceiling": read_ceiling,
            "ids": ids,
        });
        let answer: WireFetchResponse = self
            .client(
                MemoryPool::Interactive,
                credential,
                "observations/fetch",
                &body,
            )
            .await?;
        if answer
            .items
            .iter()
            .any(|item| item.access_level > read_ceiling)
            || answer.exposed_level > read_ceiling
        {
            return Err(MemoryServiceError::Protocol);
        }
        Ok(MemoryFetchResult {
            items: answer
                .items
                .into_iter()
                .map(|item| MemoryDetail {
                    observation_id: item.observation_id,
                    kind: item.kind,
                    title: item.title,
                    body: item.body,
                    occurred_at: item.occurred_at,
                    access_level: item.access_level,
                    revision: item.revision,
                    content_sha256: item.content_sha256,
                })
                .collect(),
            exposed_level: answer.exposed_level,
        })
    }

    async fn save(
        &self,
        credential: &ClientCredential,
        request: &MemorySaveRequest,
    ) -> Result<MemorySaveReceipt, MemoryServiceError> {
        let answer: WireSaveResponse = self
            .client(
                MemoryPool::Delivery,
                credential,
                "observations/save",
                request,
            )
            .await?;
        Ok(MemorySaveReceipt {
            observations: answer
                .observations
                .into_iter()
                .map(|saved| SavedMemory {
                    observation_id: saved.observation_id,
                    revision: saved.revision,
                    access_level: saved.access_level,
                })
                .collect(),
            replayed: answer.replayed,
        })
    }

    async fn request_generation(
        &self,
        credential: &ClientCredential,
        request: &MemoryGenerationRequest,
    ) -> Result<MemoryGenerationReceipt, MemoryServiceError> {
        let answer: WireGenerationStatus = self
            .client(
                MemoryPool::Delivery,
                credential,
                "generations/request",
                request,
            )
            .await?;
        Ok(MemoryGenerationReceipt {
            operation_id: answer.operation_id,
            state: answer.state,
            replayed: answer.replayed,
        })
    }
}

fn tenant_ref(tenant: TenantExternalId) -> Value {
    json!({"tenant": tenant.0.hyphenated().to_string()})
}

#[async_trait]
impl MemoryProvisioner for MemoryServiceHttp {
    async fn create_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError> {
        let body = json!({"external_id": tenant.0.hyphenated().to_string()});
        self.admin::<_, TenantView>(tenant.0, "tenants/create", &body)
            .await
            .map(Into::into)
    }
    async fn disable_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError> {
        self.admin::<_, TenantView>(tenant.0, "tenants/disable", &tenant_ref(tenant))
            .await
            .map(Into::into)
    }
    async fn enable_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError> {
        self.admin::<_, TenantView>(tenant.0, "tenants/enable", &tenant_ref(tenant))
            .await
            .map(Into::into)
    }
    async fn delete_tenant(
        &self,
        tenant: TenantExternalId,
    ) -> Result<RemoteTenant, MemoryServiceError> {
        self.admin::<_, TenantView>(tenant.0, "tenants/delete", &tenant_ref(tenant))
            .await
            .map(Into::into)
    }
    async fn create_namespace(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Result<RemoteNamespaceState, MemoryServiceError> {
        let body = json!({"tenant": tenant.0.hyphenated().to_string(), "namespace": namespace.0.hyphenated().to_string()});
        self.admin::<_, NamespaceView>(tenant.0, "namespaces/create", &body)
            .await
            .map(NamespaceView::state)
    }
    async fn delete_namespace(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Result<RemoteNamespaceState, MemoryServiceError> {
        let body = json!({"tenant": tenant.0.hyphenated().to_string(), "namespace": namespace.0.hyphenated().to_string()});
        self.admin::<_, NamespaceView>(tenant.0, "namespaces/delete", &body)
            .await
            .map(NamespaceView::state)
    }
    async fn issue_client_credential(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
        label: &str,
    ) -> Result<IssuedClientCredential, MemoryServiceError> {
        let body = json!({
            "tenant": tenant.0.hyphenated().to_string(),
            "label": label,
            "grants": [{
                "namespace": namespace.0.hyphenated().to_string(),
                "operations": CLIENT_OPERATIONS,
                "read_max": 9,
                "write_max": 9,
            }],
        });
        let issued = self
            .admin::<_, IssuedCredential>(tenant.0, "credentials/issue", &body)
            .await?;
        Ok(IssuedClientCredential {
            credential_id: issued.credential_id,
            token: SecretString::from(issued.token),
        })
    }
    async fn list_client_credentials(
        &self,
        tenant: TenantExternalId,
        query: CredentialQuery<'_>,
    ) -> Result<CredentialPage, MemoryServiceError> {
        let body = json!({"tenant": tenant.0.hyphenated().to_string(), "label": query.label,
            "active_only": query.active_only, "after": query.after, "limit": query.limit});
        let list = self
            .admin::<_, CredentialList>(tenant.0, "credentials/list", &body)
            .await?;
        Ok(CredentialPage {
            credentials: list
                .credentials
                .into_iter()
                .map(|view| RemoteCredential {
                    credential_id: view.credential_id,
                    label: view.label,
                    revoked: view.revoked_at.is_some(),
                })
                .collect(),
            next_after: list.next_after,
        })
    }
    async fn revoke_client_credential(
        &self,
        tenant: TenantExternalId,
        credential: Uuid,
    ) -> Result<(), MemoryServiceError> {
        let body =
            json!({"tenant": tenant.0.hyphenated().to_string(), "credential_id": credential});
        self.admin::<_, Value>(tenant.0, "credentials/revoke", &body)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
#[path = "contract_fake.rs"]
pub(crate) mod contract_fake;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
