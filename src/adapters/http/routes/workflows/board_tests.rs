//! The board's slow-read decision: the four phases together against the threshold.
use super::*;

#[test]
fn a_board_read_is_slow_only_when_its_phases_together_pass_the_threshold() {
    let quarter = RUN_BOARD_QUERY_WARN_THRESHOLD / 4;
    let at_threshold = BoardReadTrace {
        counts: quarter,
        visibility: quarter,
        cards: quarter,
        rollup: RUN_BOARD_QUERY_WARN_THRESHOLD - 3 * quarter,
        ..Default::default()
    };
    assert_eq!(at_threshold.elapsed(), RUN_BOARD_QUERY_WARN_THRESHOLD);
    assert!(!at_threshold.is_slow());
    let past = BoardReadTrace {
        rollup: at_threshold.rollup + Duration::from_millis(1),
        ..at_threshold
    };
    assert!(past.is_slow(), "no phase alone is slow, the read is");
    assert!(!BoardReadTrace::default().is_slow());
}
