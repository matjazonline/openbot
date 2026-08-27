use std::{
    future::Future,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::future::join_all;
use reqwest::{Client, StatusCode, header};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::{
    entities::memory::{
        MAX_MEMORY_CHUNK_CHARS, MAX_MEMORY_PROVIDER_BASE_URL_BYTES,
        MAX_MEMORY_PROVIDER_CONNECT_SECONDS, MAX_MEMORY_PROVIDER_CREDENTIAL_BYTES,
        MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES, MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES,
        MAX_MEMORY_PROVIDER_REQUEST_SECONDS, MAX_MEMORY_PROVIDER_RESPONSE_BYTES,
        MAX_MEMORY_RETURNED_ROWS, MAX_MEMORY_TARGET_COLLECTIONS, MemoryChunk, MemoryProviderError,
        MemoryRecallMode, ResolvedMemoryScope, truncate_memory_text,
    },
    services::{
        memory_provider::{
            MemoryAdditionalContext, MemoryConversation, MemoryPersistenceTarget, MemoryProvider,
            MemoryRecallQuery,
        },
        runtime_metrics::HydraDbActivity,
    },
};

pub struct HydraDbProvider {
    client: Client,
    base_url: String,
    api_key: SecretString,
    fast_timeout: Duration,
    thinking_timeout: Duration,
    activity: HydraDbActivity,
}

impl HydraDbProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: SecretString,
        fast_timeout: Duration,
        thinking_timeout: Duration,
    ) -> Result<Self, MemoryProviderError> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        if base_url.is_empty()
            || base_url.len() > MAX_MEMORY_PROVIDER_BASE_URL_BYTES
            || api_key.expose_secret().is_empty()
            || api_key.expose_secret().len() > MAX_MEMORY_PROVIDER_CREDENTIAL_BYTES
            || fast_timeout.is_zero()
            || thinking_timeout < fast_timeout
            || fast_timeout > Duration::from_secs(MAX_MEMORY_PROVIDER_REQUEST_SECONDS)
            || thinking_timeout > Duration::from_secs(MAX_MEMORY_PROVIDER_REQUEST_SECONDS)
        {
            return Err(MemoryProviderError::Unavailable);
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(
                    fast_timeout.min(Duration::from_secs(MAX_MEMORY_PROVIDER_CONNECT_SECONDS)),
                )
                .build()
                .map_err(|_| MemoryProviderError::Unavailable)?,
            base_url,
            api_key,
            fast_timeout,
            thinking_timeout,
            activity: HydraDbActivity::default(),
        })
    }

    /// Report every call into the runtime metric sample this machine writes each ten seconds.
    pub fn with_activity(mut self, activity: HydraDbActivity) -> Self {
        self.activity = activity;
        self
    }

    /// Time one provider call and tally its outcome. Wraps the whole exchange — connect, transfer
    /// and decode — because that is the latency the caller waits for.
    async fn measured<T>(
        &self,
        call: impl Future<Output = Result<T, MemoryProviderError>>,
    ) -> Result<T, MemoryProviderError> {
        let started = Instant::now();
        let outcome = call.await;
        self.activity.record(started.elapsed(), outcome.is_ok());
        outcome
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .header("API-Version", "2")
            .bearer_auth(self.api_key.expose_secret())
    }

    async fn json_response(mut response: reqwest::Response) -> Result<Value, MemoryProviderError> {
        let status = response.status();
        if !status.is_success() {
            return Err(classify_status(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_MEMORY_PROVIDER_RESPONSE_BYTES as u64)
        {
            return Err(MemoryProviderError::ResponseTooLarge);
        }

        let capacity = response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(MAX_MEMORY_PROVIDER_RESPONSE_BYTES);
        let mut bytes = Vec::with_capacity(capacity);
        while let Some(chunk) = response.chunk().await.map_err(classify_transport)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_MEMORY_PROVIDER_RESPONSE_BYTES {
                return Err(MemoryProviderError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| MemoryProviderError::MalformedResponse)
    }

    fn bounded_json(value: &Value) -> Result<Vec<u8>, MemoryProviderError> {
        let bytes =
            serde_json::to_vec(value).map_err(|_| MemoryProviderError::MalformedResponse)?;
        if bytes.len() > MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES {
            return Err(MemoryProviderError::RequestTooLarge);
        }
        Ok(bytes)
    }

    fn validate_identifier(value: &str) -> Result<(), MemoryProviderError> {
        if value.is_empty() || value.len() > MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES {
            Err(MemoryProviderError::RequestTooLarge)
        } else {
            Ok(())
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
        if fields.iter().any(|(_, value)| value.contains(&boundary)) {
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
        self.measured(async {
            Self::validate_identifier(database_id)?;
            Self::validate_identifier(&target.collection)?;
            let item = json!({
                "id": conversation.id,
                "type": "user_assistant_pairs",
                "infer": true,
                "user": conversation.user(),
                "assistant": conversation.assistant(),
            });
            let items = serde_json::to_string(&[item])
                .map_err(|_| MemoryProviderError::MalformedResponse)?;
            let mut fields = vec![
                ("database_id", database_id),
                ("collection", target.collection.as_str()),
                ("items", items.as_str()),
            ];
            if let Some(instructions) = target.custom_instructions {
                fields.push(("custom_instructions", instructions));
            }
            let (boundary, body) = Self::multipart_body(&fields)?;
            let response = self
                .request(reqwest::Method::POST, "/context/ingest")
                .timeout(self.fast_timeout)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body)
                .send()
                .await
                .map_err(classify_transport)?;
            let body = Self::json_response(response).await?;
            let results = body
                .get("results")
                .and_then(Value::as_array)
                .ok_or(MemoryProviderError::MalformedResponse)?;
            if results.len() != 1 {
                return Err(MemoryProviderError::MalformedResponse);
            }
            let rejected = results[0]
                .get("error")
                .is_some_and(|value| !value.is_null())
                || results[0]
                    .get("success")
                    .and_then(Value::as_bool)
                    .is_some_and(|success| !success)
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

fn classify_status(status: StatusCode) -> MemoryProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => MemoryProviderError::Authentication,
        StatusCode::TOO_MANY_REQUESTS => MemoryProviderError::RateLimited,
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => MemoryProviderError::Timeout,
        StatusCode::NOT_FOUND => MemoryProviderError::NotFound,
        status if status.is_server_error() => MemoryProviderError::Unavailable,
        _ => MemoryProviderError::RejectedItem,
    }
}

fn classify_transport(error: reqwest::Error) -> MemoryProviderError {
    if error.is_timeout() {
        MemoryProviderError::Timeout
    } else {
        MemoryProviderError::Unavailable
    }
}

#[async_trait]
impl MemoryProvider for HydraDbProvider {
    async fn provision(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.measured(async {
            Self::validate_identifier(database_id)?;
            let body =
                Self::bounded_json(&json!({"database_id": database_id, "name": database_id}))?;
            let response = self
                .request(reqwest::Method::POST, "/databases")
                .timeout(self.fast_timeout)
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .await
                .map_err(classify_transport)?;
            if response.status() == StatusCode::CONFLICT {
                return Ok(());
            }
            Self::json_response(response).await.map(|_| ())
        })
        .await
    }

    async fn is_ready(&self, database_id: &str) -> Result<bool, MemoryProviderError> {
        self.measured(async {
            Self::validate_identifier(database_id)?;
            let response = self
                .request(reqwest::Method::GET, "/databases/status")
                .timeout(self.fast_timeout)
                .query(&[("database_id", database_id)])
                .send()
                .await
                .map_err(classify_transport)?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(false);
            }
            let body = Self::json_response(response).await?;
            Ok(body.pointer("/status").and_then(Value::as_str) == Some("ready_for_ingestion"))
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
        self.measured(async {
            Self::validate_identifier(database_id)?;
            if scopes.len() > MAX_MEMORY_TARGET_COLLECTIONS {
                return Err(MemoryProviderError::TooManyTargets);
            }
            if !(1..=MAX_MEMORY_RETURNED_ROWS).contains(&(max_results as usize)) {
                return Err(MemoryProviderError::TooManyResults);
            }
            for scope in scopes {
                Self::validate_identifier(&scope.collection)?;
            }
            let collections: serde_json::Map<String, Value> = scopes
                .iter()
                .map(|scope| (scope.collection.clone(), json!(scope.weight)))
                .collect();
            let timeout = match mode {
                MemoryRecallMode::Fast => self.fast_timeout,
                MemoryRecallMode::Thinking => self.thinking_timeout,
            };
            let request_body = Self::bounded_json(&json!({
                "database_id": database_id,
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
            let body = Self::json_response(response).await?;
            let rows = body
                .get("results")
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
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or(MemoryProviderError::MalformedResponse)?;
                    let (content, truncated) =
                        truncate_memory_text(content, MAX_MEMORY_CHUNK_CHARS);
                    Ok(MemoryChunk {
                        source_chunk_id: row
                            .get("chunk_id")
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
        self.measured(async {
            Self::validate_identifier(database_id)?;
            let body = Self::bounded_json(&json!({"database_id": database_id}))?;
            let response = self
                .request(reqwest::Method::DELETE, "/databases")
                .timeout(self.fast_timeout)
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .await
                .map_err(classify_transport)?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(());
            }
            Self::json_response(response).await.map(|_| ())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::{Barrier, mpsc, oneshot},
    };

    async fn mock_server(status: u16, body: &'static str) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 16 * 1024];
            let read = stream.read(&mut request).await.unwrap();
            request.truncate(read);
            let _ = sender.send(String::from_utf8_lossy(&request).into_owned());
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{address}"), receiver)
    }

    async fn raw_response_server(response: Vec<u8>, hold_open: Option<Duration>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 16 * 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream.write_all(&response).await.unwrap();
            if let Some(duration) = hold_open {
                tokio::time::sleep(duration).await;
            }
        });
        format!("http://{address}")
    }

    fn test_provider(base_url: String, timeout: Duration) -> HydraDbProvider {
        HydraDbProvider::new(base_url, SecretString::from("test-key"), timeout, timeout).unwrap()
    }

    fn company_scope() -> ResolvedMemoryScope {
        ResolvedMemoryScope {
            scope: crate::entities::memory::MemoryScope::Company,
            collection: "company".into(),
            weight: 1.0,
        }
    }

    async fn concurrent_ingest_server() -> (String, mpsc::Receiver<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let barrier = Arc::new(Barrier::new(MAX_MEMORY_TARGET_COLLECTIONS + 1));
        let (sender, receiver) = mpsc::channel(MAX_MEMORY_TARGET_COLLECTIONS);
        tokio::spawn(async move {
            for _ in 0..MAX_MEMORY_TARGET_COLLECTIONS {
                let (mut stream, _) = listener.accept().await.unwrap();
                let barrier = barrier.clone();
                let sender = sender.clone();
                tokio::spawn(async move {
                    let mut request = vec![0; 16 * 1024];
                    let read = stream.read(&mut request).await.unwrap();
                    sender.send(read).await.unwrap();
                    barrier.wait().await;
                    let body = r#"{"results":[{"success":true}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                });
            }
            barrier.wait().await;
        });
        (format!("http://{address}"), receiver)
    }

    #[test]
    fn provider_errors_are_safe_and_classified() {
        assert_eq!(
            classify_status(StatusCode::UNAUTHORIZED),
            MemoryProviderError::Authentication
        );
        assert_eq!(
            classify_status(StatusCode::TOO_MANY_REQUESTS),
            MemoryProviderError::RateLimited
        );
        assert!(!MemoryProviderError::Authentication.retryable());
        assert!(MemoryProviderError::RateLimited.retryable());
        assert!(
            !MemoryProviderError::Authentication
                .to_string()
                .contains("secret")
        );
    }

    #[tokio::test]
    async fn provision_uses_v2_bearer_auth_and_treats_conflict_as_idempotent() {
        let (base_url, request) = mock_server(409, "{}").await;
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap();

        provider.provision("company-memory").await.unwrap();
        let request = request.await.unwrap();
        assert!(request.starts_with("POST /databases HTTP/1.1"));
        assert!(request.to_ascii_lowercase().contains("api-version: 2"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-key")
        );
        assert!(request.contains("company-memory"));
    }

    #[tokio::test]
    async fn activity_wraps_successful_calls_and_bounded_failures() {
        let (base_url, _) = mock_server(409, "{}").await;
        let activity = HydraDbActivity::default();
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap()
        .with_activity(activity.clone());

        provider.provision("company-memory").await.unwrap();
        assert_eq!(
            provider
                .provision(&"x".repeat(MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES + 1))
                .await,
            Err(MemoryProviderError::RequestTooLarge)
        );

        let interval = activity.drain();
        assert_eq!(interval.calls, 2);
        assert_eq!(interval.failures, 1);
    }

    #[tokio::test]
    async fn persist_surfaces_per_item_rejection_without_echoing_the_response() {
        let (base_url, _) = mock_server(200, r#"{"results":[{"success":false}]}"#).await;
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap();
        let results = provider
            .persist(
                "company-memory",
                &[MemoryPersistenceTarget {
                    scope: crate::entities::memory::MemoryScope::Company,
                    collection: "company".into(),
                    custom_instructions: None,
                }],
                &MemoryConversation::new("stable-id".into(), "hello", "world"),
            )
            .await;
        assert_eq!(results, vec![Err(MemoryProviderError::RejectedItem)]);
    }

    #[tokio::test]
    async fn persist_sends_scope_instructions_and_accepts_empty_extraction() {
        let (base_url, request) = mock_server(200, r#"{"results":[{"success":true}]}"#).await;
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap();
        let instructions = crate::entities::memory::MemoryScope::User.extraction_instructions();
        let results = provider
            .persist(
                "company-memory",
                &[MemoryPersistenceTarget {
                    scope: crate::entities::memory::MemoryScope::User,
                    collection: "user_hash".into(),
                    custom_instructions: Some(instructions),
                }],
                &MemoryConversation::new(
                    "stable-id".into(),
                    "transient request",
                    "no durable fact",
                ),
            )
            .await;

        assert_eq!(results, vec![Ok(())]);
        let request = request.await.unwrap();
        assert!(request.contains("name=\"custom_instructions\""));
        assert!(request.contains(instructions));
    }

    #[tokio::test]
    async fn recall_requires_expected_collection_attribution() {
        let scope = ResolvedMemoryScope {
            scope: crate::entities::memory::MemoryScope::Company,
            collection: "company".into(),
            weight: 1.0,
        };
        let (base_url, _) = mock_server(
            200,
            r#"{"results":[{"chunk_id":"one","content":"policy","collection":"company"}]}"#,
        )
        .await;
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap();
        let chunks = provider
            .recall(
                "company-memory",
                &MemoryRecallQuery::new("query"),
                std::slice::from_ref(&scope),
                MemoryRecallMode::Fast,
                5,
                None,
            )
            .await
            .unwrap();
        assert_eq!(chunks[0].source_scope, scope.scope);

        let (base_url, _) = mock_server(
            200,
            r#"{"results":[{"chunk_id":"one","content":"policy"}]}"#,
        )
        .await;
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from("test-key"),
            Duration::from_secs(2),
            Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(
            provider
                .recall(
                    "company-memory",
                    &MemoryRecallQuery::new("query"),
                    &[scope],
                    MemoryRecallMode::Fast,
                    5,
                    None,
                )
                .await,
            Err(MemoryProviderError::MalformedResponse)
        );
    }

    #[test]
    fn multipart_request_enforces_exact_byte_boundary_before_allocation() {
        let boundary = "fixed-boundary";
        let empty = HydraDbProvider::multipart_body_with_boundary(&[("items", "")], boundary)
            .unwrap()
            .len();
        let exact_payload = "x".repeat(MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES - empty);
        let exact = HydraDbProvider::multipart_body_with_boundary(
            &[("items", exact_payload.as_str())],
            boundary,
        )
        .unwrap();
        assert_eq!(exact.len(), MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES);

        let over_payload = "x".repeat(MAX_MEMORY_PROVIDER_REQUEST_BODY_BYTES - empty + 1);
        assert_eq!(
            HydraDbProvider::multipart_body_with_boundary(
                &[("items", over_payload.as_str())],
                boundary,
            ),
            Err(MemoryProviderError::RequestTooLarge)
        );
    }

    #[tokio::test]
    async fn response_accepts_absent_and_valid_content_length() {
        let absent = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\":\"ready_for_ingestion\"}".to_vec();
        let provider = test_provider(
            raw_response_server(absent, None).await,
            Duration::from_secs(2),
        );
        assert!(provider.is_ready("company-memory").await.unwrap());

        let (base_url, _) = mock_server(200, r#"{"status":"ready_for_ingestion"}"#).await;
        assert!(
            test_provider(base_url, Duration::from_secs(2))
                .is_ready("company-memory")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn oversized_content_length_is_rejected_without_reading_body() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{{}}",
            MAX_MEMORY_PROVIDER_RESPONSE_BYTES + 1
        )
        .into_bytes();
        let provider = test_provider(
            raw_response_server(response, None).await,
            Duration::from_secs(2),
        );
        assert_eq!(
            provider.is_ready("company-memory").await,
            Err(MemoryProviderError::ResponseTooLarge)
        );
    }

    #[tokio::test]
    async fn response_body_at_exact_byte_cap_is_accepted() {
        let mut body = b"{}".to_vec();
        body.resize(MAX_MEMORY_PROVIDER_RESPONSE_BYTES, b' ');
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        let provider = test_provider(
            raw_response_server(response, None).await,
            Duration::from_secs(2),
        );
        assert!(!provider.is_ready("company-memory").await.unwrap());
    }

    #[tokio::test]
    async fn chunked_response_crossing_byte_cap_stops_with_typed_error() {
        let payload = vec![b'x'; MAX_MEMORY_PROVIDER_RESPONSE_BYTES + 1];
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(format!("{:x}\r\n", payload.len()).as_bytes());
        response.extend_from_slice(&payload);
        response.extend_from_slice(b"\r\n0\r\n\r\n");
        let provider = test_provider(
            raw_response_server(response, None).await,
            Duration::from_secs(2),
        );
        assert_eq!(
            provider.is_ready("company-memory").await,
            Err(MemoryProviderError::ResponseTooLarge)
        );
    }

    #[tokio::test]
    async fn never_ending_response_body_obeys_request_timeout() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n".to_vec();
        let provider = test_provider(
            raw_response_server(response, Some(Duration::from_secs(1))).await,
            Duration::from_millis(50),
        );
        assert_eq!(
            provider.is_ready("company-memory").await,
            Err(MemoryProviderError::Timeout)
        );
    }

    #[tokio::test]
    async fn recall_rejects_excess_rows_and_caps_one_huge_unicode_chunk() {
        let scope = company_scope();
        let rows = (0..2)
            .map(|index| {
                json!({"chunk_id": index.to_string(), "content": "x", "collection": "company"})
            })
            .collect::<Vec<_>>();
        let body = serde_json::to_string(&json!({"results": rows})).unwrap();
        let leaked_body: &'static str = Box::leak(body.into_boxed_str());
        let (base_url, _) = mock_server(200, leaked_body).await;
        assert_eq!(
            test_provider(base_url, Duration::from_secs(2))
                .recall(
                    "company-memory",
                    &MemoryRecallQuery::new("query"),
                    std::slice::from_ref(&scope),
                    MemoryRecallMode::Fast,
                    1,
                    None,
                )
                .await,
            Err(MemoryProviderError::TooManyResults)
        );

        let content = "🦀".repeat(MAX_MEMORY_CHUNK_CHARS + 1);
        let body = serde_json::to_string(&json!({
            "results": [{"chunk_id": "one", "content": content, "collection": "company"}]
        }))
        .unwrap();
        let leaked_body: &'static str = Box::leak(body.into_boxed_str());
        let (base_url, _) = mock_server(200, leaked_body).await;
        let chunks = test_provider(base_url, Duration::from_secs(2))
            .recall(
                "company-memory",
                &MemoryRecallQuery::new("query"),
                &[scope],
                MemoryRecallMode::Fast,
                1,
                None,
            )
            .await
            .unwrap();
        assert_eq!(chunks[0].content.chars().count(), MAX_MEMORY_CHUNK_CHARS);
        assert!(chunks[0].truncated);
        assert!(
            chunks[0]
                .content
                .ends_with(crate::entities::memory::MEMORY_TRUNCATION_MARKER)
        );
    }

    #[tokio::test]
    async fn persistence_rejects_more_than_the_aggregate_target_budget() {
        let provider = test_provider("http://127.0.0.1:1".into(), Duration::from_millis(50));
        let targets = (0..=MAX_MEMORY_TARGET_COLLECTIONS)
            .map(|index| MemoryPersistenceTarget {
                scope: crate::entities::memory::MemoryScope::Company,
                collection: format!("collection-{index}"),
                custom_instructions: None,
            })
            .collect::<Vec<_>>();
        let results = provider
            .persist(
                "company-memory",
                &targets,
                &MemoryConversation::new("id".into(), "user", "assistant"),
            )
            .await;
        assert_eq!(results.len(), targets.len());
        assert!(
            results
                .iter()
                .all(|result| *result == Err(MemoryProviderError::TooManyTargets))
        );
    }

    #[tokio::test]
    async fn three_collection_persistence_is_concurrent_and_aggregate_bounded() {
        let (base_url, mut requests) = concurrent_ingest_server().await;
        let provider = test_provider(base_url, Duration::from_secs(2));
        let targets = (0..MAX_MEMORY_TARGET_COLLECTIONS)
            .map(|index| MemoryPersistenceTarget {
                scope: crate::entities::memory::MemoryScope::Company,
                collection: format!("collection-{index}"),
                custom_instructions: None,
            })
            .collect::<Vec<_>>();
        let results = provider
            .persist(
                "company-memory",
                &targets,
                &MemoryConversation::new("id".into(), "user", "assistant"),
            )
            .await;
        assert_eq!(results, vec![Ok(()); MAX_MEMORY_TARGET_COLLECTIONS]);

        let mut aggregate_request_bytes = 0usize;
        for _ in 0..MAX_MEMORY_TARGET_COLLECTIONS {
            aggregate_request_bytes += requests.recv().await.unwrap();
        }
        assert!(
            aggregate_request_bytes
                <= MAX_MEMORY_TARGET_COLLECTIONS
                    * crate::entities::memory::MAX_MEMORY_PROVIDER_REQUEST_BYTES
        );
    }

    #[tokio::test]
    #[ignore = "requires HYDRA_DB_* and HYDRA_DB_LIVE_DATABASE_ID"]
    async fn live_provisioning_smoke_test() {
        let Ok(base_url) = std::env::var("HYDRA_DB_BASE_URL") else {
            return;
        };
        let Ok(api_key) = std::env::var("HYDRA_DB_API_KEY") else {
            return;
        };
        let Ok(database_id) = std::env::var("HYDRA_DB_LIVE_DATABASE_ID") else {
            return;
        };
        let provider = HydraDbProvider::new(
            base_url,
            SecretString::from(api_key),
            Duration::from_secs(10),
            Duration::from_secs(60),
        )
        .unwrap();
        provider.provision(&database_id).await.unwrap();
    }
}
