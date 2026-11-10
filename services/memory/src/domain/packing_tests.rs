//! `memory_context_` packing rules (D8): exact budget boundaries, greedy skip-and-continue,
//! count-only omission, the exposed level of included items only, and determinism.
use super::*;
use chrono::TimeZone;

fn level(value: i64) -> AccessLevel {
    AccessLevel::new(value).unwrap()
}

fn candidate(bytes: u64, exposed: i64) -> Candidate {
    Candidate {
        bytes,
        exposed_level: level(exposed),
        expires_at: None,
    }
}

fn budget(max_tokens: u64, max_bytes: u64) -> Budget {
    Budget {
        max_tokens,
        max_bytes,
    }
}

#[test]
fn memory_context_token_budget_boundary_is_inclusive() {
    // One 100-byte item costs 64 + 200 tokens.
    let one = [candidate(100, 0)];
    let fits = select(&one, budget(264, 1 << 20));
    assert_eq!(
        (fits.included.clone(), fits.tokens, fits.bytes),
        (vec![0], 264, 100)
    );
    let over = select(&one, budget(263, 1 << 20));
    assert_eq!(
        (over.included.len(), over.omitted, over.tokens, over.bytes),
        (0, 1, 0, 0)
    );
    // Two items: the second fits exactly at 64 + 2 × 150.
    let two = [candidate(100, 0), candidate(50, 0)];
    assert_eq!(select(&two, budget(364, 1 << 20)).included, vec![0, 1]);
    assert_eq!(select(&two, budget(363, 1 << 20)).included, vec![0]);
}

#[test]
fn memory_context_byte_budget_boundary_is_inclusive_and_binds_below_half_the_tokens() {
    let items = [candidate(100, 0), candidate(50, 0)];
    // A generous token budget: bytes bind, and equality fits.
    assert_eq!(select(&items, budget(65_536, 150)).included, vec![0, 1]);
    let short = select(&items, budget(65_536, 149));
    assert_eq!(
        (short.included, short.omitted, short.bytes),
        (vec![0], 1, 100)
    );
    // max_bytes < max_tokens / 2: the byte cap is what stops the pack.
    let capped = select(&items, budget(1_000, 120));
    assert_eq!((capped.included, capped.tokens), (vec![0], 264));
}

#[test]
fn memory_context_greedy_selection_skips_what_does_not_fit_and_continues() {
    let items = [
        candidate(40, 1),
        candidate(500, 9),
        candidate(40, 2),
        candidate(1_000, 8),
        candidate(10, 3),
    ];
    let selection = select(&items, budget(64 + 2 * 90, 1 << 20));
    assert_eq!(selection.included, vec![0, 2, 4]);
    assert_eq!(
        selection.omitted, 2,
        "omission is a count of skipped candidates"
    );
    // The exposed level is the maximum of what was included, never of skipped candidates.
    assert_eq!(selection.exposed_level, level(3));
}

#[test]
fn memory_context_empty_and_tiny_budgets_expose_nothing() {
    assert_eq!(
        select(&[], budget(8_192, 65_536)),
        Selection {
            included: vec![],
            omitted: 0,
            bytes: 0,
            tokens: 0,
            exposed_level: AccessLevel::PUBLIC,
            valid_until: None,
        }
    );
    // Below the fixed overhead nothing fits; the counts stay zero.
    let tiny = select(&[candidate(1, 7), candidate(1, 7)], budget(64, 65_536));
    assert_eq!((tiny.included.len(), tiny.omitted, tiny.tokens), (0, 2, 0));
    assert_eq!(tiny.exposed_level, AccessLevel::PUBLIC);
}

#[test]
fn memory_context_selection_is_deterministic_and_valid_until_is_the_earliest_expiry() {
    let at = |hour| Some(Utc.with_ymd_and_hms(2026, 11, 9, hour, 0, 0).unwrap());
    let items = [
        Candidate {
            expires_at: at(12),
            ..candidate(10, 0)
        },
        Candidate {
            expires_at: None,
            ..candidate(10, 0)
        },
        Candidate {
            expires_at: at(3),
            ..candidate(10_000, 0)
        },
        Candidate {
            expires_at: at(6),
            ..candidate(10, 0)
        },
    ];
    let first = select(&items, budget(8_192, 65_536));
    assert_eq!(first, select(&items, budget(8_192, 65_536)));
    assert_eq!(first.included, vec![0, 1, 3]);
    // The skipped item's earlier expiry does not count.
    assert_eq!(first.valid_until, at(6));
}
