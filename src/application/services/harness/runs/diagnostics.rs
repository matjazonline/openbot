use super::*;

/// Safe operational projection. Never include arguments, transcripts, tokens or remote content.
#[derive(Debug, Clone, Serialize)]
pub struct RunDiagnostics {
    pub token_usage_source: TokenUsageSource,
    pub token_usage: crate::entities::task::TokenUsage,
    pub unreported_usage_calls: usize,
    pub unresolved_model_calls: usize,
    pub repair_call_count: usize,
    pub terminal_invalid_output_reason: Option<&'static str>,
    pub run_id: RunId,
    pub revision: CheckpointRevision,
    pub state: RunState,
    pub pending_invocation_id: Option<InvocationId>,
    pub approval_id: Option<Uuid>,
    pub outreach_id: Option<Uuid>,
    pub model_calls: usize,
    pub model_call_limit: u16,
    pub tool_calls: usize,
    pub tool_call_limit: u16,
    pub input_reserved: u64,
    pub input_limit: u64,
    pub output_reserved: u64,
    pub output_limit: u64,
    pub active_ms_reserved: u64,
    pub active_ms_limit: u64,
    pub continuations: usize,
    pub replayed_invocations: u64,
    pub indeterminate_effect: bool,
}
impl From<&RunCheckpoint> for RunDiagnostics {
    fn from(run: &RunCheckpoint) -> Self {
        let usage = RunUsage::from(run);
        Self {
            token_usage_source: usage.source,
            token_usage: usage.tokens,
            unreported_usage_calls: usage.unreported_calls,
            unresolved_model_calls: usage.unresolved_calls,
            repair_call_count: run.repair_count(),
            terminal_invalid_output_reason: (run.state == RunState::InvalidOutput)
                .then(|| {
                    run.turns
                        .last()
                        .and_then(|turn| turn.invalid_response)
                        .map(|reason| reason.code())
                })
                .flatten(),
            run_id: run.run_id,
            revision: run.revision,
            state: run.state.clone(),
            pending_invocation_id: run.pending_call().map(|call| call.invocation_id),
            approval_id: run.wait.as_ref().map(|wait| wait.approval_id),
            outreach_id: run.outreach_wait.as_ref().map(|wait| wait.outreach_id),
            model_calls: run.reservations.len(),
            model_call_limit: run.policy.model_calls,
            tool_calls: run.invocations.len(),
            tool_call_limit: run.policy.tool_calls,
            input_reserved: run.reservations.iter().map(|r| r.input_tokens).sum(),
            input_limit: run.policy.total_input_tokens,
            output_reserved: run.reservations.iter().map(|r| r.output_tokens).sum(),
            output_limit: run.policy.total_output_tokens,
            active_ms_reserved: run.executions.iter().map(|r| r.allowance_ms).sum(),
            active_ms_limit: MAX_ACTIVE_EXECUTION_MS,
            continuations: run.executions.len(),
            replayed_invocations: run
                .executions
                .iter()
                .map(|r| u64::from(r.replayed_invocations))
                .sum(),
            indeterminate_effect: run
                .invocations
                .iter()
                .any(|inv| inv.state == InvocationState::Indeterminate),
        }
    }
}
