use super::*;

#[test]
fn memory_preparation_namespace_metadata_stays_below_its_budget_at_the_scope_limit() {
    let mut memory = FrozenMemory::without_items(RecallState::Included, AccessLevel::L9);
    memory.namespaces = (0..crate::application::workflow::scopes::MAX_RUN_READ_NAMESPACES)
        .map(|_| NamespaceExternalId(uuid::Uuid::new_v4()))
        .collect();
    memory.digests = vec!["a".repeat(64); memory.namespaces.len()];
    assert!(
        serde_json::to_vec(&memory).unwrap().len() < (MAX_RECALL_BYTES as usize * 4 / 5),
        "scope metadata must fail early before it crowds out the frozen recall budget"
    );
}

/// Variable unit sets share one fixed recall budget; no per-unit multiplication.
#[test]
fn memory_preparation_recall_budget_shares() {
    assert_eq!(share(1, 2048), 2048);
    assert_eq!(share(2, 2048), 1024);
    assert_eq!(share(4, 2048), 512);
    for count in 1..=258 {
        assert!(share(count, 8192) * count as u32 <= 8192);
    }
    assert_eq!(
        share(3, 2),
        0,
        "the caller skips a budget too small for every namespace"
    );
}

/// E7: transport, capacity, deadline and not-ready leave recall optional; authorization and
/// contract failures fail closed.
#[test]
fn memory_preparation_optional_failures_are_distinct_from_refusals() {
    for (error, code) in [
        (MemoryServiceError::Unavailable, Some("unavailable")),
        (MemoryServiceError::Ambiguous, Some("deadline")),
        (
            MemoryServiceError::Capacity {
                retry_after_secs: Some(1),
            },
            Some("capacity"),
        ),
        (MemoryServiceError::NotFound, Some("not_ready")),
        (MemoryServiceError::Unauthorized, None),
        (
            MemoryServiceError::Forbidden {
                code: "forbidden".into(),
            },
            None,
        ),
        (MemoryServiceError::Protocol, None),
    ] {
        assert_eq!(optional_failure(&error), code, "{error:?}");
    }
}
