//! Progressive recall and context packing (03B, D6, D8): timeline, context packs and their
//! validation. Each request is validated against the D6 bounds and authorized against the
//! credential's current grants, whatever an earlier search returned; the store applies the D4
//! predicate and provenance filter. Packs are structured data for an arbitrary agent host: the
//! service builds no prompt text, roles or instructions, and stored text stays content.
use super::{
    contract::*,
    embedding::EmbeddingPool,
    ports::{ClientRead, PackCandidate, Store, TimelineQuery},
    search::{denied, namespaces, query_text, search_query},
    service::ClientPrincipal,
};
use crate::domain::{
    access_level::AccessLevel,
    bounds::{MAX_CONTEXT_BYTES, MAX_CONTEXT_ITEMS, MAX_CONTEXT_TOKENS, MAX_TIMELINE_SIDE},
    error::ServiceError,
    grant::{Operation, authorize},
    packing::{self, Budget, Candidate, DEFAULT_CONTEXT_BYTES, DEFAULT_CONTEXT_TOKENS},
};
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

pub const DEFAULT_TIMELINE_SIDE: u32 = 5;

fn decode<T: DeserializeOwned>(input: Value) -> Result<T, ServiceError> {
    serde_json::from_value(input)
        .map_err(|error| ServiceError::invalid(format!("Invalid request: {error}")))
}

fn encode<T: Serialize>(value: &T) -> Result<Value, ServiceError> {
    serde_json::to_value(value).map_err(|_| ServiceError::unavailable("Cannot encode the result"))
}

fn json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, ServiceError> {
    serde_json::to_vec(value).map_err(|_| ServiceError::unavailable("Cannot encode the result"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_scope(
    client: &ClientPrincipal,
    operation: Operation,
    requested: &[String],
    read_ceiling: AccessLevel,
) -> Result<ClientRead, ServiceError> {
    namespaces(requested)?;
    let scope =
        authorize(&client.grants, operation, requested, Some(read_ceiling)).map_err(denied)?;
    Ok(ClientRead {
        tenant_id: client.tenant_id,
        namespace_ids: scope.namespace_ids,
        read_ceiling,
    })
}

fn bounded(
    value: Option<u32>,
    default: u32,
    min: u32,
    max: u32,
    what: &str,
) -> Result<u32, ServiceError> {
    let value = value.unwrap_or(default);
    if (min..=max).contains(&value) {
        Ok(value)
    } else {
        Err(ServiceError::invalid(format!("{what} must be {min}-{max}")))
    }
}

pub async fn timeline(
    store: &dyn Store,
    client: &ClientPrincipal,
    input: Value,
) -> Result<Value, ServiceError> {
    let request: TimelineRequest = decode(input)?;
    let before = bounded(
        request.before,
        DEFAULT_TIMELINE_SIDE,
        0,
        MAX_TIMELINE_SIDE,
        "before",
    )?;
    let after = bounded(
        request.after,
        DEFAULT_TIMELINE_SIDE,
        0,
        MAX_TIMELINE_SIDE,
        "after",
    )?;
    let read = read_scope(
        client,
        Operation::Timeline,
        &request.namespaces,
        request.read_ceiling,
    )?;
    let query = TimelineQuery {
        anchor: request.anchor,
        before,
        after,
        session_only: request.session_only,
    };
    encode(&store.timeline(&read, query).await?)
}

/// The digest of an item's visible provenance as served: lowercase hex SHA-256 of the compact
/// JSON of its `sources` list. `context.validate` compares it with the current one.
pub fn sources_sha256(sources: &[SourceRef]) -> String {
    hex(&Sha256::digest(
        serde_json::to_vec(sources).unwrap_or_default(),
    ))
}

/// The exact compact-JSON size of a candidate as it will be served. A `full` candidate's body
/// is not loaded yet: its size is the item with an empty text plus the body's JSON-string size.
fn served_bytes(candidate: &PackCandidate) -> Result<u64, ServiceError> {
    let bytes = json_bytes(&candidate.item)?.len() as u64;
    Ok(match candidate.body_json_bytes {
        Some(body) => bytes - 2 + body,
        None => bytes,
    })
}

fn sized(candidates: &[PackCandidate]) -> Result<Vec<Candidate>, ServiceError> {
    candidates
        .iter()
        .map(|candidate| {
            Ok(Candidate {
                bytes: served_bytes(candidate)?,
                exposed_level: candidate.exposed_level,
                expires_at: candidate.valid_until,
            })
        })
        .collect()
}

/// Pack visible candidates (D8): whole items in rank order under both budgets, omission as a
/// count only, and the highest level of what was included. `considered` is the number of
/// visible candidates before any pre-selection, so every candidate left out counts as omitted.
pub fn assemble(
    candidates: Vec<PackCandidate>,
    considered: usize,
    budget: Budget,
    candidates_truncated: bool,
    semantic: SemanticState,
    generated_at: DateTime<Utc>,
) -> Result<ContextPack, ServiceError> {
    let selection = packing::select(&sized(&candidates)?, budget);
    let mut candidates: Vec<Option<PackCandidate>> = candidates.into_iter().map(Some).collect();
    let items: Vec<PackItem> = selection
        .included
        .iter()
        .filter_map(|index| candidates[*index].take().map(|candidate| candidate.item))
        .collect();
    let pack_digest = hex(&Sha256::digest(json_bytes(&items)?));
    let count = |value: u64| {
        u32::try_from(value).map_err(|_| ServiceError::unavailable("The pack is too large"))
    };
    Ok(ContextPack {
        omitted: Omitted {
            budget: count(considered.saturating_sub(items.len()) as u64)?,
        },
        items,
        exposed_level: selection.exposed_level,
        token_count: count(selection.tokens)?,
        byte_count: count(selection.bytes)?,
        candidates_truncated,
        pack_digest,
        valid_until: selection.valid_until,
        generated_at,
        semantic,
    })
}

pub async fn pack(
    store: &dyn Store,
    embeddings: &EmbeddingPool,
    client: &ClientPrincipal,
    input: Value,
    now: DateTime<Utc>,
) -> Result<Value, ServiceError> {
    let request: ContextPackRequest = decode(input)?;
    let max_tokens = bounded(
        request.max_tokens,
        DEFAULT_CONTEXT_TOKENS,
        1,
        MAX_CONTEXT_TOKENS,
        "max_tokens",
    )?;
    let max_bytes = bounded(
        request.max_bytes,
        DEFAULT_CONTEXT_BYTES,
        1,
        MAX_CONTEXT_BYTES,
        "max_bytes",
    )?;
    if let Some(query) = &request.query {
        query_text(query)?;
    }
    let read = read_scope(
        client,
        Operation::Context,
        &request.namespaces,
        request.read_ceiling,
    )?;
    let detail = request.detail.unwrap_or(PackDetail::Compact);
    let query = match request.query {
        Some(text) if !text.trim().is_empty() => {
            Some(search_query(embeddings, text, MAX_CONTEXT_ITEMS).await?)
        }
        _ => None,
    };
    let found = store.pack_candidates(&read, query.as_ref(), detail).await?;
    let budget = Budget {
        max_tokens: u64::from(max_tokens),
        max_bytes: u64::from(max_bytes),
    };
    let considered = found.candidates.len();
    let mut candidates = found.candidates;
    if detail == PackDetail::Full {
        // Select on exact sizes first, then load only the selected bodies (at most the byte
        // budget). A body corrected or hidden in between is left out; selection is then rerun
        // on the loaded items, so both budgets hold whatever happened.
        let first = packing::select(&sized(&candidates)?, budget);
        let wanted: Vec<(Uuid, i64)> = first
            .included
            .iter()
            .map(|index| {
                let item = &candidates[*index].item;
                (item.observation_id, item.revision)
            })
            .collect();
        let mut bodies = if wanted.is_empty() {
            HashMap::new()
        } else {
            store.pack_bodies(&read, &wanted).await?
        };
        let mut slots: Vec<Option<PackCandidate>> = candidates.into_iter().map(Some).collect();
        candidates = first
            .included
            .iter()
            .filter_map(|index| {
                let mut candidate = slots[*index].take()?;
                candidate.item.text = bodies.remove(&candidate.item.observation_id)?;
                candidate.body_json_bytes = None;
                Some(candidate)
            })
            .collect();
    }
    let pack = assemble(
        candidates,
        considered,
        budget,
        found.truncated,
        found.semantic,
        now,
    )?;
    super::metrics::METRICS.inc("memory_context_packs_total", &[], 1);
    super::metrics::METRICS.inc(
        "memory_context_omitted_total",
        &[],
        u64::from(pack.omitted.budget),
    );
    encode(&pack)
}

pub async fn validate(
    store: &dyn Store,
    client: &ClientPrincipal,
    input: Value,
) -> Result<Value, ServiceError> {
    let request: ContextValidateRequest = decode(input)?;
    if request.items.is_empty() || request.items.len() > MAX_CONTEXT_ITEMS {
        return Err(ServiceError::invalid(format!(
            "items must hold 1-{MAX_CONTEXT_ITEMS} entries"
        )));
    }
    if request.items.iter().any(|item| item.revision < 1) {
        return Err(ServiceError::invalid("revisions start at 1"));
    }
    let read = read_scope(
        client,
        Operation::Context,
        &request.namespaces,
        request.read_ceiling,
    )?;
    encode(&store.validate_pack(&read, &request.items).await?)
}
