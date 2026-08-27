use std::{
    future::Future,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::future::join_all;
use reqwest::{Client, StatusCode, multipart};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::{
    entities::memory::{MemoryChunk, MemoryProviderError, MemoryRecallMode, ResolvedMemoryScope},
    services::{
        memory_provider::{MemoryConversation, MemoryProvider},
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
        if base_url.is_empty() {
            return Err(MemoryProviderError::Unavailable);
        }
        Ok(Self {
            client: Client::builder()
                .connect_timeout(fast_timeout)
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

    async fn json_response(response: reqwest::Response) -> Result<Value, MemoryProviderError> {
        let status = response.status();
        if !status.is_success() {
            return Err(classify_status(status));
        }
        response
            .json()
            .await
            .map_err(|_| MemoryProviderError::MalformedResponse)
    }

    async fn persist_collection(
        &self,
        database_id: &str,
        collection: &str,
        conversation: &MemoryConversation,
    ) -> Result<(), MemoryProviderError> {
        self.measured(async {
            let item = json!({
                "id": conversation.id,
                "type": "user_assistant_pairs",
                "infer": true,
                "user": conversation.user,
                "assistant": conversation.assistant,
            });
            let items = serde_json::to_string(&[item])
                .map_err(|_| MemoryProviderError::MalformedResponse)?;
            let form = multipart::Form::new()
                .text("database_id", database_id.to_owned())
                .text("collection", collection.to_owned())
                .text("items", items);
            let response = self
                .request(reqwest::Method::POST, "/context/ingest")
                .timeout(self.fast_timeout)
                .multipart(form)
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
            let response = self
                .request(reqwest::Method::POST, "/databases")
                .timeout(self.fast_timeout)
                .json(&json!({"database_id": database_id, "name": database_id}))
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
        query: &str,
        scopes: &[ResolvedMemoryScope],
        mode: MemoryRecallMode,
        max_results: u8,
        additional_context: Option<&str>,
    ) -> Result<Vec<MemoryChunk>, MemoryProviderError> {
        self.measured(async {
            let collections: serde_json::Map<String, Value> = scopes
                .iter()
                .map(|scope| (scope.collection.clone(), json!(scope.weight)))
                .collect();
            let timeout = match mode {
                MemoryRecallMode::Fast => self.fast_timeout,
                MemoryRecallMode::Thinking => self.thinking_timeout,
            };
            let response = self
                .request(reqwest::Method::POST, "/query")
                .timeout(timeout)
                .json(&json!({
                    "database_id": database_id,
                    "type": "memory",
                    "query": query,
                    "query_by": "hybrid",
                    "collections": collections,
                    "mode": mode.as_str(),
                    "max_results": max_results,
                    "additional_context": additional_context,
                }))
                .send()
                .await
                .map_err(classify_transport)?;
            let body = Self::json_response(response).await?;
            let rows = body
                .get("results")
                .and_then(Value::as_array)
                .ok_or(MemoryProviderError::MalformedResponse)?;
            rows.iter()
                .map(|row| {
                    let content = row
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or(MemoryProviderError::MalformedResponse)?;
                    Ok(MemoryChunk {
                        source_chunk_id: row
                            .get("chunk_id")
                            .or_else(|| row.get("id"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        content: content.to_owned(),
                    })
                })
                .collect()
        })
        .await
    }

    async fn persist(
        &self,
        database_id: &str,
        collections: &[String],
        conversation: &MemoryConversation,
    ) -> Vec<Result<(), MemoryProviderError>> {
        join_all(
            collections
                .iter()
                .map(|collection| self.persist_collection(database_id, collection, conversation)),
        )
        .await
    }

    async fn delete(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.measured(async {
            let response = self
                .request(reqwest::Method::DELETE, "/databases")
                .timeout(self.fast_timeout)
                .json(&json!({"database_id": database_id}))
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
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
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
                &["company".into()],
                &MemoryConversation {
                    id: "stable-id".into(),
                    user: "hello".into(),
                    assistant: "world".into(),
                },
            )
            .await;
        assert_eq!(results, vec![Err(MemoryProviderError::RejectedItem)]);
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
