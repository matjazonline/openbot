use std::time::Duration;

use async_trait::async_trait;
use futures::future::join_all;
use reqwest::{StatusCode, header};
use secrecy::SecretString;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    adapters::memory::http::{
        MemoryHttpClient, classify_transport, validate_identifier, validate_recall_bounds,
    },
    entities::memory::{
        MAX_MEMORY_CHUNK_CHARS, MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES, MAX_MEMORY_RETURNED_ROWS,
        MAX_MEMORY_TARGET_COLLECTIONS, MemoryChunk, MemoryProviderError, MemoryRecallMode,
        ResolvedMemoryScope, truncate_memory_text,
    },
    services::{
        memory_provider::{
            MemoryAdditionalContext, MemoryConversation, MemoryPersistenceTarget, MemoryProvider,
            MemoryRecallQuery,
        },
        runtime_metrics::MemoryProviderActivity,
    },
};

pub struct HydraDbProvider {
    http: MemoryHttpClient,
}

impl HydraDbProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: SecretString,
        fast_timeout: Duration,
        thinking_timeout: Duration,
    ) -> Result<Self, MemoryProviderError> {
        Ok(Self {
            http: MemoryHttpClient::new(base_url, api_key, fast_timeout, thinking_timeout)?,
        })
    }

    /// Report every call into the runtime metric sample this machine writes each ten seconds.
    pub fn with_activity(mut self, activity: MemoryProviderActivity) -> Self {
        self.http = self.http.with_activity(activity);
        self
    }

    /// HydraDB pins its wire contract with a version header the shared client knows nothing about.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, path).header("API-Version", "2")
    }

    /// Unwrap v2's `{success, data, error, meta}` envelope.
    ///
    /// Every v2 route answers in this shape, so the payload lives under `data` rather than at the
    /// top level. Nothing from `error` reaches the caller: the variant carries the meaning and the
    /// message could name a database or a credential.
    fn v2_data(body: Value) -> Result<Value, MemoryProviderError> {
        Self::warn_on_deprecation(&body);
        if body.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(MemoryProviderError::RejectedItem);
        }
        body.get("data")
            .cloned()
            .ok_or(MemoryProviderError::MalformedResponse)
    }

    /// `meta.deprecation` is HydraDB telling us we sent a legacy field name — `tenant_id` for
    /// `database`, `sub_tenant_id` for `collection`. That is a mistake in this file, not a runtime
    /// condition, so it is logged for the operator and never turned into a caller-visible error.
    fn warn_on_deprecation(body: &Value) {
        let Some(notices) = body.pointer("/meta/deprecation").and_then(Value::as_array) else {
            return;
        };
        let deprecated: Vec<&str> = notices
            .iter()
            .filter_map(|notice| notice.get("deprecated_field").and_then(Value::as_str))
            .collect();
        if !deprecated.is_empty() {
            warn!(
                fields = ?deprecated,
                "HydraDB reported deprecated request fields; this adapter is behind the v2 contract"
            );
        }
    }

    fn multipart_body(fields: &[(&str, &str)]) -> Result<(String, Vec<u8>), MemoryProviderError> {
        let boundary = format!("mail-agents-{}", uuid::Uuid::new_v4().simple());
        let body = Self::multipart_body_with_boundary(fields, &boundary)?;
        Ok((boundary, body))
    }

    fn multipart_body_with_boundary(
        fields: &[(&str, &str)],
        boundary: &str,
    ) -> Result<Vec<u8>, MemoryProviderError> {
        if fields.iter().any(|(_, value)| value.contains(boundary)) {
            return Err(MemoryProviderError::RequestTooLarge);
        }
        let mut length = format!("--{boundary}--\r\n").len();
        for (name, value) in fields {
            length = length
                .saturating_add(format!("--{boundary}\r\n").len())
                .saturating_add(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").len(),
                )
                .saturating_add(value.len())
                .saturating_add(2);
        }
        if length > MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES {
            return Err(MemoryProviderError::RequestTooLarge);
        }

        let mut body = Vec::with_capacity(length);
        for (name, value) in fields {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            );
            body.extend_from_slice(value.as_bytes());
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        debug_assert_eq!(body.len(), length);
        Ok(body)
    }

    async fn persist_collection(
        &self,
        database_id: &str,
        target: &MemoryPersistenceTarget,
        conversation: &MemoryConversation,
    ) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                validate_identifier(database_id)?;
                validate_identifier(&target.collection)?;
                // v2 shape: the item kind is the form-level `type`, and a memory item carries the
                // pair list rather than a `user`/`assistant` pair of its own. `custom_instructions`
                // is per item here, not per request.
                let mut item = json!({
                    "id": conversation.id,
                    "infer": true,
                    "user_assistant_pairs": [{
                        "user": conversation.user(),
                        "assistant": conversation.assistant(),
                    }],
                });
                if let Some(instructions) = target.custom_instructions {
                    item["custom_instructions"] = json!(instructions);
                }
                let memories = serde_json::to_string(&[item])
                    .map_err(|_| MemoryProviderError::MalformedResponse)?;
                let fields = vec![
                    ("database", database_id),
                    ("collection", target.collection.as_str()),
                    ("type", "memory"),
                    ("memories", memories.as_str()),
                ];
                let (boundary, body) = Self::multipart_body(&fields)?;
                let response = self
                    .request(reqwest::Method::POST, "/context/ingest")
                    .timeout(self.http.fast_timeout())
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(body)
                    .send()
                    .await
                    .map_err(classify_transport)?;
                let data = Self::v2_data(MemoryHttpClient::json_response(response).await?)?;
                let results = data
                    .get("results")
                    .and_then(Value::as_array)
                    .ok_or(MemoryProviderError::MalformedResponse)?;
                if results.len() != 1 {
                    return Err(MemoryProviderError::MalformedResponse);
                }
                // Ingestion is accepted (202) and indexed asynchronously, so `queued` and
                // `processing` are successes here. Only `failed` — or a failure count the batch
                // summary reports — is a rejection.
                let rejected = data
                    .get("failed_count")
                    .and_then(Value::as_u64)
                    .is_some_and(|failed| failed > 0)
                    || results[0]
                        .get("error")
                        .is_some_and(|value| !value.is_null())
                    || results[0]
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|status| matches!(status, "failed" | "rejected"));
                if rejected {
                    Err(MemoryProviderError::RejectedItem)
                } else {
                    Ok(())
                }
            })
            .await
    }
}

#[async_trait]
impl MemoryProvider for HydraDbProvider {
    async fn provision(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                validate_identifier(database_id)?;
                let body = MemoryHttpClient::bounded_json(&json!({"database": database_id}))?;
                let response = self
                    .request(reqwest::Method::POST, "/databases")
                    .timeout(self.http.fast_timeout())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .send()
                    .await
                    .map_err(classify_transport)?;
                if response.status() == StatusCode::CONFLICT {
                    return Ok(());
                }
                Self::v2_data(MemoryHttpClient::json_response(response).await?).map(|_| ())
            })
            .await
    }

    async fn is_ready(&self, database_id: &str) -> Result<bool, MemoryProviderError> {
        self.http
            .measured(async {
                validate_identifier(database_id)?;
                let response = self
                    .request(reqwest::Method::GET, "/databases/status")
                    .timeout(self.http.fast_timeout())
                    .query(&[("database", database_id)])
                    .send()
                    .await
                    .map_err(classify_transport)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(false);
                }
                let data = Self::v2_data(MemoryHttpClient::json_response(response).await?)?;
                // v2 reports readiness as a boolean inside an infrastructure block, not as a
                // top-level status string.
                Ok(data
                    .pointer("/infra/ready_for_ingestion")
                    .and_then(Value::as_bool)
                    == Some(true))
            })
            .await
    }

    async fn recall(
        &self,
        database_id: &str,
        query: &MemoryRecallQuery,
        scopes: &[ResolvedMemoryScope],
        mode: MemoryRecallMode,
        max_results: u8,
        additional_context: Option<&MemoryAdditionalContext>,
    ) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
        self.http
            .measured(async {
                validate_identifier(database_id)?;
                validate_recall_bounds(scopes.len(), max_results)?;
                for scope in scopes {
                    validate_identifier(&scope.collection)?;
                }
                let collections: serde_json::Map<String, Value> = scopes
                    .iter()
                    .map(|scope| (scope.collection.clone(), json!(scope.weight)))
                    .collect();
                let timeout = self.http.timeout_for(mode);
                let request_body = MemoryHttpClient::bounded_json(&json!({
                    "database": database_id,
                    "type": "memory",
                    "query": query.as_str(),
                    "query_by": "hybrid",
                    "collections": collections,
                    "mode": mode.as_str(),
                    "max_results": max_results,
                    "additional_context": additional_context.map(MemoryAdditionalContext::as_str),
                }))?;
                let response = self
                    .request(reqwest::Method::POST, "/query")
                    .timeout(timeout)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(request_body)
                    .send()
                    .await
                    .map_err(classify_transport)?;
                let data = Self::v2_data(MemoryHttpClient::json_response(response).await?)?;
                let rows = data
                    .get("chunks")
                    .and_then(Value::as_array)
                    .ok_or(MemoryProviderError::MalformedResponse)?;
                if rows.len() > max_results as usize || rows.len() > MAX_MEMORY_RETURNED_ROWS {
                    return Err(MemoryProviderError::TooManyResults);
                }
                rows.iter()
                    .map(|row| {
                        let collection = row
                            .get("collection")
                            .and_then(Value::as_str)
                            .ok_or(MemoryProviderError::MalformedResponse)?;
                        let source_scope = scopes
                            .iter()
                            .find(|scope| scope.collection == collection)
                            .map(|scope| scope.scope)
                            .ok_or(MemoryProviderError::MalformedResponse)?;
                        let content = row
                            .get("chunk_content")
                            .and_then(Value::as_str)
                            .ok_or(MemoryProviderError::MalformedResponse)?;
                        let (content, truncated) =
                            truncate_memory_text(content, MAX_MEMORY_CHUNK_CHARS);
                        Ok(MemoryChunk {
                            // The chunk uuid identifies the returned row; `id` is the source it
                            // came from, and several chunks can share one.
                            source_chunk_id: row
                                .get("chunk_uuid")
                                .or_else(|| row.get("id"))
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            content,
                            source_scope,
                            truncated,
                        })
                    })
                    .collect()
            })
            .await
    }

    async fn persist(
        &self,
        database_id: &str,
        targets: &[MemoryPersistenceTarget],
        conversation: &MemoryConversation,
    ) -> Vec<Result<(), MemoryProviderError>> {
        if targets.len() > MAX_MEMORY_TARGET_COLLECTIONS {
            return targets
                .iter()
                .map(|_| Err(MemoryProviderError::TooManyTargets))
                .collect();
        }
        join_all(
            targets
                .iter()
                .map(|target| self.persist_collection(database_id, target, conversation)),
        )
        .await
    }

    async fn delete(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                validate_identifier(database_id)?;
                // v2 takes the database as a query parameter here, not as a JSON body.
                let response = self
                    .request(reqwest::Method::DELETE, "/databases")
                    .timeout(self.http.fast_timeout())
                    .query(&[("database", database_id)])
                    .send()
                    .await
                    .map_err(classify_transport)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(());
                }
                Self::v2_data(MemoryHttpClient::json_response(response).await?).map(|_| ())
            })
            .await
    }
}

#[cfg(test)]
#[path = "hydradb_tests.rs"]
mod tests;
