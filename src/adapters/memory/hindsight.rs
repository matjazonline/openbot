//! Hindsight (https://hindsight.vectorize.io) as a long-term memory backend, over its HTTP API.
//!
//! Hindsight partitions only by *bank*: there is no collection concept inside one. Each of our
//! memory scopes therefore gets its own bank, `{database_id}--{collection}`, so the isolation
//! `resolve_scopes` guarantees stays structural rather than depending on tag filters. Two
//! consequences run through everything below:
//!
//! * recall fans out one call per scope and merges client-side — Hindsight's recall takes a token
//!   budget, not a row limit, and its rows carry no collection field, so scope attribution comes
//!   from *which call* returned a row;
//! * only the company bank is provisioned. Agent and user bank ids are not knowable until a
//!   message names them, so those banks are created on the write path.

use std::time::Duration;

use async_trait::async_trait;
use futures::future::join_all;
use reqwest::{Method, StatusCode, header};
use secrecy::SecretString;
use serde_json::{Value, json};

use crate::{
    adapters::memory::http::{
        MemoryHttpClient, ScopeRecallResults, ScoredMemoryChunk, classify_transport,
        merge_scope_results, validate_identifier, validate_recall_bounds,
    },
    entities::memory::{
        HINDSIGHT_RECALL_MAX_TOKENS, MAX_HINDSIGHT_BANK_ID_BYTES, MAX_MEMORY_CHUNK_CHARS,
        MAX_MEMORY_RETURNED_ROWS, MAX_MEMORY_TARGET_COLLECTIONS, MemoryChunk, MemoryProviderError,
        MemoryRecallMode, ResolvedMemoryScope, truncate_memory_text,
    },
    services::{
        memory_provider::{
            MemoryAdditionalContext, MemoryConversation, MemoryPersistenceTarget, MemoryProvider,
            MemoryRecallQuery,
        },
        runtime_metrics::MemoryProviderActivity,
    },
};

/// The scope whose bank stands in for the company as a whole: the one `provision` creates and
/// `is_ready` probes, because it is the only bank id knowable from the company alone.
const ANCHOR_COLLECTION: &str = "company";
/// Separates the company's namespace from the scope inside it. Two characters neither
/// `remote_memory_database_id` nor any collection name produces, so the prefix is unambiguous
/// when `delete` enumerates a company's banks.
const NAMESPACE_SEPARATOR: &str = "--";
/// One page of `GET /banks`, and the ceiling on how many pages `delete` will walk. A company has
/// at most one bank per company, agent and sender, so a listing longer than this is a sign the
/// server ignored the filter rather than a company we should keep deleting through.
const BANK_PAGE_SIZE: usize = 100;
const MAX_DELETED_BANKS: usize = 1_000;

pub struct HindsightProvider {
    http: MemoryHttpClient,
}

impl HindsightProvider {
    /// `base_url` carries the API version and organization path segment
    /// (`https://api.hindsight.vectorize.io/v1/default`), so the same code serves Hindsight Cloud,
    /// a self-hosted instance and a non-default organization.
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

    /// The bank id for one scope of one company.
    ///
    /// The composed id goes into a URL path segment, so `validate_identifier`'s charset check is
    /// what stops a future collection scheme from escaping into the path. Hindsight publishes no
    /// length limit; the bound here is ours, and the worst case we generate — the User scope, at
    /// 123 bytes — is pinned by a test.
    fn bank_id(database_id: &str, collection: &str) -> Result<String, MemoryProviderError> {
        validate_identifier(database_id)?;
        validate_identifier(collection)?;
        let bank_id = format!("{database_id}{NAMESPACE_SEPARATOR}{collection}");
        if bank_id.len() > MAX_HINDSIGHT_BANK_ID_BYTES {
            return Err(MemoryProviderError::RequestTooLarge);
        }
        Ok(bank_id)
    }

    /// The prefix every bank of one company shares.
    fn namespace_prefix(database_id: &str) -> Result<String, MemoryProviderError> {
        validate_identifier(database_id)?;
        Ok(format!("{database_id}{NAMESPACE_SEPARATOR}"))
    }

    /// Create or update a bank. Idempotent, and the body is deliberately minimal: bank defaults
    /// are Hindsight's business, and re-sending our own would overwrite anything an operator has
    /// tuned in its console.
    async fn put_bank(&self, bank_id: &str) -> Result<(), MemoryProviderError> {
        let body = MemoryHttpClient::bounded_json(&json!({"name": bank_id}))?;
        let response = self
            .http
            .request(Method::PUT, &format!("/banks/{bank_id}"))
            .timeout(self.http.fast_timeout())
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(classify_transport)?;
        MemoryHttpClient::json_response(response).await.map(|_| ())
    }

    /// Recall from one scope's bank. A bank that does not exist yet has nothing to contribute, so
    /// a 404 is an empty result rather than a failure — and the read path never creates one.
    async fn recall_scope(
        &self,
        database_id: &str,
        scope: &ResolvedMemoryScope,
        query: &str,
        mode: MemoryRecallMode,
    ) -> Result<ScopeRecallResults, MemoryProviderError> {
        let bank_id = Self::bank_id(database_id, &scope.collection)?;
        let body = MemoryHttpClient::bounded_json(&json!({
            "query": query,
            "budget": match mode {
                MemoryRecallMode::Fast => "low",
                MemoryRecallMode::Thinking => "high",
            },
            "max_tokens": HINDSIGHT_RECALL_MAX_TOKENS,
            // Consolidated observations say more per token than the raw facts behind them, and
            // the caller's budget is tokens.
            "prefer_observations": true,
        }))?;
        let response = self
            .http
            .request(Method::POST, &format!("/banks/{bank_id}/memories/recall"))
            .timeout(self.http.timeout_for(mode))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(classify_transport)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(ScopeRecallResults {
                weight: scope.weight,
                rows: Vec::new(),
            });
        }
        let body = MemoryHttpClient::json_response(response).await?;
        let results = body
            .get("results")
            .and_then(Value::as_array)
            .ok_or(MemoryProviderError::MalformedResponse)?;
        if results.len() > MAX_MEMORY_RETURNED_ROWS {
            return Err(MemoryProviderError::TooManyResults);
        }
        let rows = results
            .iter()
            .map(|row| {
                let text = row
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(MemoryProviderError::MalformedResponse)?;
                let (content, truncated) = truncate_memory_text(text, MAX_MEMORY_CHUNK_CHARS);
                Ok(ScoredMemoryChunk {
                    // `scores.final` is what Hindsight ranked by; it is absent when the caller
                    // did not ask for scoring detail, and `merge_scope_results` falls back to
                    // this row's position then.
                    score: row.pointer("/scores/final").and_then(Value::as_f64),
                    chunk: MemoryChunk {
                        source_chunk_id: row
                            .get("chunk_id")
                            .or_else(|| row.get("id"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        content,
                        source_scope: scope.scope,
                        truncated,
                    },
                })
            })
            .collect::<Result<Vec<_>, MemoryProviderError>>()?;
        Ok(ScopeRecallResults {
            weight: scope.weight,
            rows,
        })
    }

    /// Retain one conversation into one scope's bank.
    ///
    /// `async: true` because Hindsight's retain runs an LLM extraction pass and this is awaited on
    /// the message-handling path. A malformed request still fails here; extraction failures move
    /// behind the returned operation, which we deliberately do not poll — persistence is
    /// best-effort for every provider.
    async fn persist_scope(
        &self,
        database_id: &str,
        target: &MemoryPersistenceTarget,
        conversation: &MemoryConversation,
    ) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                let bank_id = Self::bank_id(database_id, &target.collection)?;
                let mut item = json!({
                    // Same `document_id` on a retry upserts rather than duplicating: the id is
                    // already stable across attempts for this task, channel and agent.
                    "document_id": conversation.id,
                    "content": format!("{}\n\n{}", conversation.user(), conversation.assistant()),
                    "tags": [target.scope.label()],
                });
                // Hindsight's own extraction instructions are bank-level, but the persistence mode
                // they express is a channel setting that can change between messages. Per-item
                // context is the request-scoped equivalent: guidance to the extractor rather than
                // the hard filter HydraDB applies.
                if let Some(instructions) = target.custom_instructions {
                    item["context"] = json!(instructions);
                }
                let body = MemoryHttpClient::bounded_json(&json!({
                    "items": [item],
                    "async": true,
                    "operation_id": conversation.id,
                }))?;

                let mut response = self.retain(&bank_id, &body).await?;
                if response.status() == StatusCode::NOT_FOUND {
                    // The agent and user banks are only nameable once a message arrives. Create
                    // on demand and retry exactly once — never a loop, because a bank that is
                    // still missing after a successful create is a server problem, not ours.
                    self.put_bank(&bank_id).await?;
                    response = self.retain(&bank_id, &body).await?;
                }
                let body = MemoryHttpClient::json_response(response).await?;
                let accepted = body.get("success").and_then(Value::as_bool) == Some(true)
                    && body.get("items_count").and_then(Value::as_u64) == Some(1);
                if accepted {
                    Ok(())
                } else {
                    Err(MemoryProviderError::RejectedItem)
                }
            })
            .await
    }

    async fn retain(
        &self,
        bank_id: &str,
        body: &[u8],
    ) -> Result<reqwest::Response, MemoryProviderError> {
        self.http
            .request(Method::POST, &format!("/banks/{bank_id}/memories"))
            .timeout(self.http.fast_timeout())
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(classify_transport)
    }

    /// Every bank belonging to one company.
    ///
    /// `q` is a case-insensitive *substring* filter, so it narrows the listing but does not decide
    /// membership. The prefix check below does, and it is what keeps `delete` from ever reaching
    /// another company's bank.
    async fn company_banks(&self, database_id: &str) -> Result<Vec<String>, MemoryProviderError> {
        let prefix = Self::namespace_prefix(database_id)?;
        let mut banks = Vec::new();
        let mut offset = 0usize;
        loop {
            let response = self
                .http
                .request(Method::GET, "/banks")
                .timeout(self.http.fast_timeout())
                .query(&[
                    ("q", prefix.as_str()),
                    ("limit", &BANK_PAGE_SIZE.to_string()),
                    ("offset", &offset.to_string()),
                ])
                .send()
                .await
                .map_err(classify_transport)?;
            let body = MemoryHttpClient::json_response(response).await?;
            let page = body
                .get("banks")
                .and_then(Value::as_array)
                .ok_or(MemoryProviderError::MalformedResponse)?;
            let returned = page.len();
            banks.extend(
                page.iter()
                    .filter_map(|bank| bank.get("bank_id").and_then(Value::as_str))
                    .filter(|bank_id| bank_id.starts_with(&prefix))
                    .map(str::to_owned),
            );
            if banks.len() > MAX_DELETED_BANKS {
                return Err(MemoryProviderError::ResponseTooLarge);
            }
            offset += returned;
            let total = body
                .get("total")
                .and_then(Value::as_u64)
                .ok_or(MemoryProviderError::MalformedResponse)?;
            if returned == 0 || offset as u64 >= total {
                return Ok(banks);
            }
        }
    }

    async fn delete_bank(&self, bank_id: &str) -> Result<(), MemoryProviderError> {
        let response = self
            .http
            .request(Method::DELETE, &format!("/banks/{bank_id}"))
            .timeout(self.http.fast_timeout())
            .send()
            .await
            .map_err(classify_transport)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        MemoryHttpClient::json_response(response).await.map(|_| ())
    }
}

#[async_trait]
impl MemoryProvider for HindsightProvider {
    /// Creating the anchor bank proves the credential, the organization path segment and
    /// reachability in one call. There is no asynchronous remote build to wait for.
    async fn provision(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                let bank_id = Self::bank_id(database_id, ANCHOR_COLLECTION)?;
                self.put_bank(&bank_id).await
            })
            .await
    }

    async fn is_ready(&self, database_id: &str) -> Result<bool, MemoryProviderError> {
        self.http
            .measured(async {
                let bank_id = Self::bank_id(database_id, ANCHOR_COLLECTION)?;
                let response = self
                    .http
                    .request(Method::GET, &format!("/banks/{bank_id}/config"))
                    .timeout(self.http.fast_timeout())
                    .send()
                    .await
                    .map_err(classify_transport)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(false);
                }
                MemoryHttpClient::json_response(response)
                    .await
                    .map(|_| true)
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
                validate_recall_bounds(scopes.len(), max_results)?;
                // Recall takes no separate context field, and the value is already bounded to a
                // few hundred characters, so it rides along as a suffix on the query.
                let query = match additional_context {
                    Some(context) => format!("{}\n\n{}", query.as_str(), context.as_str()),
                    None => query.as_str().to_owned(),
                };
                let per_scope = join_all(
                    scopes
                        .iter()
                        .map(|scope| self.recall_scope(database_id, scope, &query, mode)),
                )
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
                merge_scope_results(per_scope, max_results)
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
                .map(|target| self.persist_scope(database_id, target, conversation)),
        )
        .await
    }

    async fn delete(&self, database_id: &str) -> Result<(), MemoryProviderError> {
        self.http
            .measured(async {
                for bank_id in self.company_banks(database_id).await? {
                    self.delete_bank(&bank_id).await?;
                }
                // The anchor bank is deleted unconditionally: a listing that missed it must not
                // leave the company's memory behind after the company is gone.
                let anchor = Self::bank_id(database_id, ANCHOR_COLLECTION)?;
                self.delete_bank(&anchor).await
            })
            .await
    }
}

#[cfg(test)]
#[path = "hindsight_tests.rs"]
mod tests;
