use super::*;

/// E7: the recall budget is split company 1/2, agent 1/4, user 1/4.
#[test]
fn memory_preparation_recall_budget_shares() {
    assert_eq!(share(MemoryToolScope::Company, 2048), 1024);
    assert_eq!(share(MemoryToolScope::Agent, 2048), 512);
    assert_eq!(share(MemoryToolScope::User, 2048), 512);
    assert_eq!(share(MemoryToolScope::Agent, 2), 1, "never zero");
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
