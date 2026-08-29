use reqwest::StatusCode;

use super::*;
use crate::entities::memory::MemoryScope;

fn chunk(content: &str) -> MemoryChunk {
    MemoryChunk {
        source_chunk_id: Some(content.to_owned()),
        content: content.to_owned(),
        source_scope: MemoryScope::Company,
        truncated: false,
    }
}

fn scored(content: &str, score: Option<f64>) -> ScoredMemoryChunk {
    ScoredMemoryChunk {
        chunk: chunk(content),
        score,
    }
}

#[test]
fn identifiers_separate_a_size_bound_from_a_malformed_one() {
    assert_eq!(
        validate_identifier("mail-agents-company-abc--user_9f"),
        Ok(())
    );
    assert_eq!(
        validate_identifier(""),
        Err(MemoryProviderError::RequestTooLarge)
    );
    assert_eq!(
        validate_identifier(&"a".repeat(MAX_MEMORY_PROVIDER_IDENTIFIER_BYTES + 1)),
        Err(MemoryProviderError::RequestTooLarge)
    );
    // A path segment is the destination for some providers, so these are not merely unusual.
    for hostile in ["company/../other", "company?q=1", "company#frag", "compañy"] {
        assert_eq!(
            validate_identifier(hostile),
            Err(MemoryProviderError::InvalidIdentifier),
            "{hostile}"
        );
    }
}

#[test]
fn status_classification_keeps_account_failures_out_of_the_retry_set() {
    assert_eq!(
        classify_status(StatusCode::UNAUTHORIZED),
        MemoryProviderError::Authentication
    );
    assert_eq!(
        classify_status(StatusCode::PAYMENT_REQUIRED),
        MemoryProviderError::Authentication
    );
    assert_eq!(
        classify_status(StatusCode::TOO_MANY_REQUESTS),
        MemoryProviderError::RateLimited
    );
    assert_eq!(
        classify_status(StatusCode::UNPROCESSABLE_ENTITY),
        MemoryProviderError::RejectedItem
    );
    assert!(!classify_status(StatusCode::PAYMENT_REQUIRED).retryable());
    assert!(classify_status(StatusCode::BAD_GATEWAY).retryable());
}

#[test]
fn recall_bounds_reject_over_wide_and_over_deep_requests() {
    assert_eq!(
        validate_recall_bounds(MAX_MEMORY_TARGET_COLLECTIONS, 5),
        Ok(())
    );
    assert_eq!(
        validate_recall_bounds(MAX_MEMORY_TARGET_COLLECTIONS + 1, 5),
        Err(MemoryProviderError::TooManyTargets)
    );
    assert_eq!(
        validate_recall_bounds(1, 0),
        Err(MemoryProviderError::TooManyResults)
    );
    assert_eq!(
        validate_recall_bounds(1, MAX_MEMORY_RETURNED_ROWS as u8 + 1),
        Err(MemoryProviderError::TooManyResults)
    );
}

#[test]
fn merged_scopes_rank_by_provider_score_times_scope_weight() {
    // The company row scores higher on its own, but the user scope outweighs it three to one —
    // which is the whole point of carrying `weight` into a client-side merge.
    let merged = merge_scope_results(
        vec![
            ScopeRecallResults {
                weight: 1.0,
                rows: vec![scored("company-strong", Some(0.9))],
            },
            ScopeRecallResults {
                weight: 3.0,
                rows: vec![scored("user-weak", Some(0.5))],
            },
        ],
        5,
    )
    .unwrap();
    assert_eq!(
        merged
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>(),
        ["user-weak", "company-strong"]
    );
}

#[test]
fn merged_scopes_truncate_to_the_requested_row_count() {
    let merged = merge_scope_results(
        vec![ScopeRecallResults {
            weight: 1.0,
            rows: (0..6)
                .map(|index| scored(&format!("row-{index}"), Some(1.0 - f64::from(index) / 10.0)))
                .collect(),
        }],
        2,
    )
    .unwrap();
    assert_eq!(
        merged
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>(),
        ["row-0", "row-1"]
    );
}

#[test]
fn a_scope_without_scores_keeps_the_order_the_provider_returned() {
    let merged = merge_scope_results(
        vec![ScopeRecallResults {
            weight: 1.0,
            rows: vec![
                scored("first", None),
                scored("second", None),
                scored("third", None),
            ],
        }],
        3,
    )
    .unwrap();
    assert_eq!(
        merged
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
}

#[test]
fn a_scope_returning_more_rows_than_the_row_bound_is_rejected() {
    let over = merge_scope_results(
        vec![ScopeRecallResults {
            weight: 1.0,
            rows: (0..=MAX_MEMORY_RETURNED_ROWS)
                .map(|index| scored(&format!("row-{index}"), Some(0.5)))
                .collect(),
        }],
        5,
    );
    assert_eq!(over, Err(MemoryProviderError::TooManyResults));
}
