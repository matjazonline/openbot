//! Client memory use cases (02B): request validation against the D6 bounds, authorization of the
//! current grants, and the canonical idempotency identity (D5). Storage, the D13 parent locks,
//! label derivation over resolved sources and the idempotency record run in the store.
use super::{
    contract::*,
    ports::{ClientRead, ClientWrite, Store},
    service::{ClientPrincipal, RequestIds},
};
use crate::domain::{
    access_level::AccessLevel,
    bounds::*,
    error::{ErrorCode, ServiceError, valid_external_id},
    generation::{holds_secret, redact},
    grant::{Denial, Operation, authorize},
};
use chrono::{DateTime, Duration, SubsecRound, Utc};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

/// A write request may be retried for at most this long (D5).
pub const MAX_REQUEST_LIFETIME_DAYS: i64 = 7;
/// Fetch returns at most this many response bytes of details; the rest is `omitted_for_budget`.
pub const MAX_FETCH_BYTES: usize = 2 * 1024 * 1024;
const MAX_TITLE_BYTES: usize = 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_SOURCE_CONTENT_BYTES: usize = 256 * 1024;

fn decode<T: DeserializeOwned>(input: Value) -> Result<T, ServiceError> {
    serde_json::from_value(input)
        .map_err(|error| ServiceError::invalid(format!("Invalid request: {error}")))
}

fn denied(denial: Denial) -> ServiceError {
    match denial {
        Denial::NoNamespace | Denial::TooManyNamespaces | Denial::MissingReadCeiling => {
            ServiceError::invalid(denial.to_string())
        }
        // Ungranted, unknown and deleted namespaces, missing operations, a ceiling above the
        // grant and a grant without write authority are one indistinguishable 403 (D3).
        _ => ServiceError::forbidden(),
    }
}

fn id(value: &str, what: &str) -> Result<(), ServiceError> {
    if valid_external_id(value) {
        Ok(())
    } else {
        Err(ServiceError::invalid(format!(
            "{what} must be 1-128 bytes of [A-Za-z0-9._:-]"
        )))
    }
}

fn text(
    value: &str,
    what: &str,
    min: usize,
    chars: usize,
    bytes: usize,
) -> Result<(), ServiceError> {
    let count = value.chars().count();
    if value.contains('\0') {
        // PostgreSQL text cannot hold NUL; refuse it instead of failing at the database.
        return Err(ServiceError::invalid(format!(
            "{what} must not contain NUL"
        )));
    }
    if count < min || count > chars || value.len() > bytes {
        Err(ServiceError::invalid(format!(
            "{what} must be {min}-{chars} characters and at most {bytes} bytes"
        )))
    } else {
        Ok(())
    }
}

fn count<T>(items: &[T], what: &str, min: usize, max: usize) -> Result<(), ServiceError> {
    if items.len() < min || items.len() > max {
        Err(ServiceError::invalid(format!(
            "{what} must hold {min}-{max} entries"
        )))
    } else {
        Ok(())
    }
}

/// PostgreSQL stores timestamps in microseconds. Request timestamps are truncated to that
/// precision before validation, hashing and storage, so an identical re-record or retry whose
/// timestamps differ only below a microsecond names the same rows and the same request.
pub(super) fn micros(at: DateTime<Utc>) -> DateTime<Utc> {
    at.trunc_subsecs(6)
}

fn request_window(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> Result<(), ServiceError> {
    if expires_at > now + Duration::days(MAX_REQUEST_LIFETIME_DAYS) {
        Err(ServiceError::invalid(
            "expires_at may be at most 7 days ahead",
        ))
    } else {
        Ok(())
    }
}

fn future(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    what: &str,
) -> Result<(), ServiceError> {
    match expires_at {
        Some(at) if at <= now => Err(ServiceError::invalid(format!(
            "{what} must be in the future"
        ))),
        _ => Ok(()),
    }
}

/// SHA-256 over the versioned canonical request: the validated, typed request (defaults
/// applied) without the key. The deadline is part of it: a retry must resend the original
/// `expires_at`, so the record, kept until that deadline has passed, outlives every attempt
/// that could still execute (D5). Field order is the struct's; the JSON has
/// no maps with unordered keys. Text is hashed byte for byte.
pub fn canonical_sha256<T: Serialize>(kind: &str, request: &T) -> Result<[u8; 32], ServiceError> {
    let mut value = serde_json::to_value(request)
        .map_err(|_| ServiceError::unavailable("Cannot encode the request"))?;
    if let Value::Object(object) = &mut value {
        object.remove("idempotency_key");
    }
    let canonical = serde_json::json!({ "v": 1, "kind": kind, "request": value });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|_| ServiceError::unavailable("Cannot encode the request"))?;
    Ok(Sha256::digest(bytes).into())
}

fn context(context: &ProducingContext) -> Result<(), ServiceError> {
    count(&context.sources, "context.sources", 0, MAX_CONTEXT_SOURCES)?;
    count(
        &context.external_sources,
        "context.external_sources",
        0,
        MAX_CONTEXT_SOURCES,
    )?;
    for source in &context.sources {
        if let SourceRef::Source { revision, .. } = source {
            text(
                revision,
                "sources.revision",
                1,
                MAX_REVISION_BYTES,
                MAX_REVISION_BYTES,
            )?;
        }
    }
    for source in &context.external_sources {
        text(
            &source.reference,
            "external_sources.reference",
            1,
            MAX_REFERENCE_BYTES,
            MAX_REFERENCE_BYTES,
        )?;
    }
    Ok(())
}

pub(super) struct Write<'a> {
    pub client: &'a ClientPrincipal,
    pub kind: &'static str,
    pub operation: Operation,
    pub namespace: &'a str,
    pub read_ceiling: Option<AccessLevel>,
    pub key: &'a str,
    pub expires_at: DateTime<Utc>,
}

pub(super) fn authorize_write<T: Serialize>(
    write: Write<'_>,
    request: &T,
    now: DateTime<Utc>,
    ids: RequestIds,
) -> Result<ClientWrite, ServiceError> {
    id(write.key, "idempotency_key")?;
    id(write.namespace, "namespace")?;
    request_window(write.expires_at, now)?;
    let scope = authorize(
        &write.client.grants,
        write.operation,
        &[write.namespace.to_owned()],
        write.read_ceiling,
    )
    .map_err(denied)?;
    Ok(ClientWrite {
        tenant_id: write.client.tenant_id,
        credential_id: write.client.credential_id,
        namespace_id: scope.namespace_ids[0],
        write_max: scope.write_max.ok_or_else(ServiceError::forbidden)?,
        read_max: write
            .client
            .grants
            .iter()
            .find(|grant| grant.namespace_id == scope.namespace_ids[0])
            .and_then(|grant| grant.read_max),
        readable: write
            .client
            .grants
            .iter()
            // Citing a record is reading it: only namespaces granted with `fetch` (D3).
            .filter(|grant| grant.operations.contains(&Operation::Fetch))
            .filter_map(|grant| grant.read_max.map(|level| (grant.namespace_id, level)))
            .collect(),
        kind: write.kind,
        key: write.key.to_owned(),
        request_sha256: canonical_sha256(write.kind, request)?,
        expires_at: write.expires_at,
        now,
        request_id: ids.request,
        correlation: ids.correlation,
    })
}

/// Run one client memory operation; operations without a use case yet are `not_implemented`.
pub async fn execute(
    store: &dyn Store,
    spec_id: &'static str,
    client: &ClientPrincipal,
    input: Value,
    now: DateTime<Utc>,
    ids: RequestIds,
) -> Result<Value, ServiceError> {
    {
        match spec_id {
            "sources.record" => {
                let mut request: SourcesRecordRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                for source in &mut request.sources {
                    source.occurred_at = micros(source.occurred_at);
                    source.expires_at = source.expires_at.map(micros);
                }
                count(&request.sources, "sources", 1, MAX_WINDOW_EVENTS)?;
                for source in &request.sources {
                    text(
                        &source.external_ref,
                        "external_ref",
                        1,
                        MAX_REFERENCE_BYTES,
                        MAX_REFERENCE_BYTES,
                    )?;
                    text(
                        &source.revision,
                        "revision",
                        1,
                        MAX_REVISION_BYTES,
                        MAX_REVISION_BYTES,
                    )?;
                    text(
                        &source.content,
                        "content",
                        0,
                        MAX_SOURCE_CONTENT_CHARS,
                        MAX_SOURCE_CONTENT_BYTES,
                    )?;
                    future(source.expires_at, now, "A source's expires_at")?;
                }
                // Privacy before durable capture (D9, D5): identifiers that hold a secret are
                // refused, and content is redacted before the request is hashed, so neither the
                // stored evidence nor the stored request identity derives from removed text.
                if request.sources.iter().any(|source| {
                    holds_secret(&source.external_ref) || holds_secret(&source.revision)
                }) {
                    return Err(ServiceError::invalid(
                        "external_ref and revision must not hold secrets or private spans",
                    )
                    .with_details(serde_json::json!({"reason": "secret_in_identifier"})));
                }
                for source in &mut request.sources {
                    source.content = redact(&source.content).text;
                }
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Sources,
                        namespace: &request.namespace,
                        read_ceiling: None,
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                if request
                    .sources
                    .iter()
                    .any(|s| s.access_level > write.write_max)
                {
                    return Err(ServiceError::forbidden());
                }
                store.record_sources(&write, &request).await
            }
            "sessions.open" => {
                let mut request: SessionOpenRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                id(&request.external_id, "external_id")?;
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Sessions,
                        namespace: &request.namespace,
                        read_ceiling: None,
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                store.open_session(&write, &request).await
            }
            "sessions.close" => {
                let mut request: SessionCloseRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Sessions,
                        namespace: &request.namespace,
                        read_ceiling: None,
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                store.close_session(&write, &request).await
            }
            "observations.save" => {
                let mut request: SaveRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                for item in &mut request.items {
                    item.occurred_at = micros(item.occurred_at);
                    item.expires_at = item.expires_at.map(micros);
                }
                count(&request.items, "items", 1, MAX_SAVE_ITEMS)?;
                context(&request.context)?;
                for item in &request.items {
                    text(&item.title, "title", 1, MAX_TITLE_CHARS, MAX_TITLE_BYTES)?;
                    text(&item.body, "body", 0, MAX_BODY_CHARS, MAX_BODY_BYTES)?;
                    future(item.expires_at, now, "An item's expires_at")?;
                }
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Save,
                        namespace: &request.namespace,
                        read_ceiling: None,
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                store.save_observations(&write, &request).await
            }
            "observations.correct" => {
                let mut request: CorrectRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                context(&request.context)?;
                text(&request.title, "title", 1, MAX_TITLE_CHARS, MAX_TITLE_BYTES)?;
                text(&request.body, "body", 0, MAX_BODY_CHARS, MAX_BODY_BYTES)?;
                let cites_itself = request.context.sources.iter().any(|source| {
                    matches!(source, SourceRef::Observation { observation_id, .. }
                        if *observation_id == request.observation_id)
                });
                if cites_itself {
                    return Err(ServiceError::invalid(
                        "A correction cannot cite the observation it corrects",
                    ));
                }
                if request.expected_revision < 1 {
                    return Err(ServiceError::invalid("expected_revision starts at 1"));
                }
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Correct,
                        namespace: &request.namespace,
                        read_ceiling: Some(request.read_ceiling),
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                store.correct_observation(&write, &request).await
            }
            "observations.delete" => {
                let mut request: DeleteRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Delete,
                        namespace: &request.namespace,
                        read_ceiling: Some(request.read_ceiling),
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                )?;
                store.delete_observation(&write, &request).await
            }
            "namespaces.purge" => {
                let mut request: PurgeRequest = decode(input)?;
                request.expires_at = micros(request.expires_at);
                let write = authorize_write(
                    Write {
                        client,
                        kind: spec_id,
                        operation: Operation::Purge,
                        namespace: &request.namespace,
                        read_ceiling: None,
                        key: &request.idempotency_key,
                        expires_at: request.expires_at,
                    },
                    &request,
                    now,
                    ids,
                );
                match write {
                    Ok(write) => store.purge_namespace(&write, &request).await,
                    // The purge removed the caller's own grant, so its retry is refused here; it
                    // replays for the credential that executed this exact request, and only for
                    // it (06A A1-2).
                    Err(error) if error.code == ErrorCode::Forbidden => {
                        let sha = canonical_sha256(spec_id, &request)?;
                        store
                            .purge_replay(
                                client.tenant_id,
                                client.credential_id,
                                &request.idempotency_key,
                                sha,
                            )
                            .await?
                            .ok_or(error)
                    }
                    Err(error) => Err(error),
                }
            }
            "observations.fetch" => {
                let request: FetchRequest = decode(input)?;
                count(&request.ids, "ids", 1, MAX_FETCH_IDS)?;
                for namespace in &request.namespaces {
                    id(namespace, "namespace")?;
                }
                let scope = authorize(
                    &client.grants,
                    Operation::Fetch,
                    &request.namespaces,
                    Some(request.read_ceiling),
                )
                .map_err(denied)?;
                let mut seen = BTreeSet::new();
                let ids: Vec<Uuid> = request
                    .ids
                    .iter()
                    .copied()
                    .filter(|id| seen.insert(*id))
                    .collect();
                let read = ClientRead {
                    tenant_id: client.tenant_id,
                    namespace_ids: scope.namespace_ids,
                    read_ceiling: request.read_ceiling,
                };
                let response = store.fetch_observations(&read, &ids).await?;
                serde_json::to_value(response)
                    .map_err(|_| ServiceError::unavailable("Cannot encode the result"))
            }
            _ => Err(ServiceError::not_implemented(spec_id)),
        }
    }
}
