//! Hybrid search (03A, D7): validation against the D6 bounds, authorization of the current
//! grants, the query embedding through the bounded pool, then both legs and fusion in the
//! store under the D4 predicate. A query embedding failure is 503, never a keyword-only answer.
use super::{
    contract::{SearchRequest, SearchResponse, SemanticState},
    embedding::EmbeddingPool,
    ports::{ClientRead, SearchQuery, Store},
    service::ClientPrincipal,
};
use crate::domain::{
    access_level::AccessLevel,
    bounds::{MAX_QUERY_CHARS, MAX_SEARCH_LIMIT},
    error::{ServiceError, valid_external_id},
    grant::{Denial, Operation, authorize},
    retrieval::identifier_tokens,
};
use serde_json::Value;

pub const DEFAULT_SEARCH_LIMIT: u32 = 20;

pub(super) fn denied(denial: Denial) -> ServiceError {
    match denial {
        Denial::NoNamespace | Denial::TooManyNamespaces | Denial::MissingReadCeiling => {
            ServiceError::invalid(denial.to_string())
        }
        _ => ServiceError::forbidden(),
    }
}

pub(super) fn namespaces(namespaces: &[String]) -> Result<(), ServiceError> {
    for namespace in namespaces {
        if !valid_external_id(namespace) {
            return Err(ServiceError::invalid(
                "namespace must be 1-128 bytes of [A-Za-z0-9._:-]",
            ));
        }
    }
    Ok(())
}

pub(super) fn query_text(query: &str) -> Result<(), ServiceError> {
    if query.contains('\0') || query.chars().count() > MAX_QUERY_CHARS {
        return Err(ServiceError::invalid(format!(
            "query must be at most {MAX_QUERY_CHARS} characters without NUL"
        )));
    }
    Ok(())
}

/// The authorized query: identifier tokens for the keyword leg and the query embedding from
/// the bounded pool (a failure is 503, a query over the model limit 400).
pub(super) async fn search_query(
    embeddings: &EmbeddingPool,
    text: String,
    limit: usize,
) -> Result<SearchQuery, ServiceError> {
    let vector = embeddings.embed_query(&text).await?;
    Ok(SearchQuery {
        identifiers: identifier_tokens(&text),
        text,
        vector,
        model_id: embeddings.model_id().to_owned(),
        limit,
    })
}

pub async fn search(
    store: &dyn Store,
    embeddings: &EmbeddingPool,
    client: &ClientPrincipal,
    input: Value,
) -> Result<Value, ServiceError> {
    let request: SearchRequest = serde_json::from_value(input)
        .map_err(|error| ServiceError::invalid(format!("Invalid request: {error}")))?;
    namespaces(&request.namespaces)?;
    query_text(&request.query)?;
    let limit = request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(ServiceError::invalid(format!(
            "limit must be 1-{MAX_SEARCH_LIMIT}"
        )));
    }
    let scope = authorize(
        &client.grants,
        Operation::Search,
        &request.namespaces,
        Some(request.read_ceiling),
    )
    .map_err(denied)?;
    let encode = |response: SearchResponse| {
        serde_json::to_value(response)
            .map_err(|_| ServiceError::unavailable("Cannot encode the result"))
    };
    if request.query.trim().is_empty() {
        return encode(SearchResponse {
            items: Vec::new(),
            exposed_level: AccessLevel::PUBLIC,
            semantic: SemanticState::Complete,
        });
    }
    let read = ClientRead {
        tenant_id: client.tenant_id,
        namespace_ids: scope.namespace_ids,
        read_ceiling: request.read_ceiling,
    };
    let query = search_query(embeddings, request.query, limit as usize).await?;
    encode(store.search_observations(&read, &query).await?)
}
