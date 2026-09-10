use super::{RunCheckpoint, TokenUsageSource};
use crate::entities::task::TokenUsage;

/// One contribution per reserved request. Unknown outcomes retain the reserved upper estimate;
/// a committed response replaces that estimate, including after a process restart.
pub struct RunUsage {
    pub tokens: TokenUsage,
    pub source: TokenUsageSource,
    pub unreported_calls: usize,
    pub unresolved_calls: usize,
}
impl From<&RunCheckpoint> for RunUsage {
    fn from(run: &RunCheckpoint) -> Self {
        let mut input = 0u64;
        let mut output = 0u64;
        let mut reported = false;
        let mut estimated = false;
        let mut unreported_calls = 0;
        let mut unresolved_calls = 0;
        for reservation in &run.reservations {
            let turn = run
                .turns
                .iter()
                .find(|turn| turn.request_id == reservation.request_id);
            let source = turn.map_or(TokenUsageSource::Estimated, |turn| turn.token_usage_source);
            input = input
                .saturating_add(turn.map_or(reservation.input_tokens, |turn| turn.input_tokens));
            output = output
                .saturating_add(turn.map_or(reservation.output_tokens, |turn| turn.output_tokens));
            reported |= source != TokenUsageSource::Estimated;
            estimated |= source != TokenUsageSource::Reported;
            unreported_calls += usize::from(source != TokenUsageSource::Reported);
            unresolved_calls += usize::from(turn.is_none());
        }
        Self {
            tokens: TokenUsage::new(input as usize, output as usize),
            source: match (reported, estimated) {
                (true, true) => TokenUsageSource::Mixed,
                (true, false) => TokenUsageSource::Reported,
                _ => TokenUsageSource::Estimated,
            },
            unreported_calls,
            unresolved_calls,
        }
    }
}
