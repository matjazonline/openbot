use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use crate::{
    app_error::{AppError, AppResult},
    entities::memory::{MemoryChunk, MemoryRecallMode, ResolvedMemoryScope},
    services::memory_provider::{MemoryConversation, MemoryProvider},
};

pub struct HydraDbProvider {
    client: Client,
    base_url: String,
    api_key: SecretString,
    fast_timeout: Duration,
    thinking_timeout: Duration,
}

impl HydraDbProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: SecretString,
        fast_timeout: Duration,
        thinking_timeout: Duration,
    ) -> AppResult<Self> {
        Ok(Self {
            client: Client::builder()
                .build()
                .map_err(|e| AppError::Internal(e.to_string()))?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key,
            fast_timeout,
            thinking_timeout,
        })
    }
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .header("API-Version", "2")
            .bearer_auth(self.api_key.expose_secret())
    }
    async fn accepted(response: reqwest::Response) -> AppResult<Value> {
        let status = response.status();
        let body = response.text().await.map_err(provider_error)?;
        if status == StatusCode::NOT_FOUND {
            return Ok(json!({"absent": true}));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "HydraDB request failed with status {status}"
            )));
        }
        serde_json::from_str(&body)
            .map_err(|_| AppError::Internal("HydraDB returned a malformed v2 response".into()))
    }
}

fn provider_error(error: reqwest::Error) -> AppError {
    AppError::Internal(format!("HydraDB request failed: {error}"))
}

#[async_trait]
impl MemoryProvider for HydraDbProvider {
    async fn provision(&self, database_id: &str) -> AppResult<()> {
        let response = self
            .request(reqwest::Method::POST, "/databases")
            .json(&json!({"database_id": database_id, "name": database_id}))
            .send()
            .await
            .map_err(provider_error)?;
        Self::accepted(response).await.map(|_| ())
    }
    async fn is_ready(&self, database_id: &str) -> AppResult<bool> {
        let response = self
            .request(reqwest::Method::GET, "/databases/status")
            .query(&[("database_id", database_id)])
            .send()
            .await
            .map_err(provider_error)?;
        let body = Self::accepted(response).await?;
        Ok(body.pointer("/status").and_then(Value::as_str) == Some("ready_for_ingestion"))
    }
    async fn recall(
        &self,
        database_id: &str,
        query: &str,
        scopes: &[ResolvedMemoryScope],
        mode: MemoryRecallMode,
        max_results: u8,
        additional_context: Option<&str>,
    ) -> AppResult<Vec<MemoryChunk>> {
        let collections: serde_json::Map<String, Value> = scopes
            .iter()
            .map(|s| (s.collection.clone(), json!(s.weight)))
            .collect();
        let timeout = if mode == MemoryRecallMode::Thinking {
            self.thinking_timeout
        } else {
            self.fast_timeout
        };
        let response = self.request(reqwest::Method::POST, "/query").timeout(timeout).json(&json!({"database_id": database_id, "type":"memory", "query":query, "query_by":"hybrid", "collections":collections, "mode":mode.as_str(), "max_results":max_results, "additional_context":additional_context})).send().await.map_err(provider_error)?;
        let body = Self::accepted(response).await?;
        let rows = body
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        rows.into_iter()
            .map(|row| {
                Ok(MemoryChunk {
                    source_chunk_id: row
                        .get("chunk_id")
                        .or_else(|| row.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    content: row
                        .get("content")
                        .and_then(Value::as_str)
                        .ok_or_else(|| AppError::Internal("HydraDB result omitted content".into()))?
                        .to_owned(),
                })
            })
            .collect()
    }
    async fn persist(
        &self,
        database_id: &str,
        collections: &[String],
        conversation: &MemoryConversation,
    ) -> Vec<AppResult<()>> {
        let mut results = Vec::with_capacity(collections.len());
        for collection in collections {
            let item = json!({"id":conversation.id,"type":"user_assistant_pairs","infer":true,"user":conversation.user,"assistant":conversation.assistant});
            let response = self
                .request(reqwest::Method::POST, "/context/ingest")
                .json(&json!({"database_id":database_id,"collection":collection,"items":[item]}))
                .send()
                .await
                .map_err(provider_error);
            results.push(match response {
                Ok(response) => Self::accepted(response).await.and_then(|body| {
                    if body.pointer("/results/0/error").is_some() {
                        Err(AppError::Internal("HydraDB rejected a memory item".into()))
                    } else {
                        Ok(())
                    }
                }),
                Err(error) => Err(error),
            });
        }
        results
    }
    async fn delete(&self, database_id: &str) -> AppResult<()> {
        let response = self
            .request(reqwest::Method::DELETE, "/databases")
            .json(&json!({"database_id":database_id}))
            .send()
            .await
            .map_err(provider_error)?;
        Self::accepted(response).await.map(|_| ())
    }
}
