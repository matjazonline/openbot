use super::*;

pub const CHECKPOINT_SCHEMA_VERSION: u16 = 1;
pub const MAX_CHECKPOINT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 65_536;
pub const MAX_ARGUMENT_BYTES: usize = 65_536;
pub const MAX_CONTINUATIONS: u16 = 64;
pub const MAX_CLAIM_MS: u64 = 300_000;
pub const MAX_ACTIVE_EXECUTION_MS: u64 = 3_600_000;

/// Byte-per-token fallback deliberately overestimates ordinary text. Callers must include
/// tool schemas, metadata and the complete restored history in the input reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigExecutionPolicy {
    pub model_calls: u16,
    pub tool_calls: u16,
    pub request_input_tokens: u64,
    pub request_output_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}
impl Default for RigExecutionPolicy {
    fn default() -> Self {
        Self {
            model_calls: 8,
            tool_calls: 64,
            request_input_tokens: 131_072,
            request_output_tokens: 16_384,
            total_input_tokens: 1_048_576,
            total_output_tokens: 262_144,
        }
    }
}
impl RigExecutionPolicy {
    pub fn validate(self) -> AppResult<()> {
        let ceiling = Self {
            model_calls: 16,
            ..Self::default()
        };
        if self.model_calls == 0
            || self.model_calls > ceiling.model_calls
            || self.tool_calls == 0
            || self.tool_calls > ceiling.tool_calls
            || self.request_input_tokens == 0
            || self.request_input_tokens > ceiling.request_input_tokens
            || self.request_output_tokens == 0
            || self.request_output_tokens > ceiling.request_output_tokens
            || self.total_input_tokens == 0
            || self.total_input_tokens > ceiling.total_input_tokens
            || self.total_output_tokens == 0
            || self.total_output_tokens > ceiling.total_output_tokens
        {
            return Err(invalid("Rig execution policy exceeds server ceilings"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Active,
    InvalidOutput,
    Waiting,
    Completed,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCheckpoint {
    pub schema_version: u16,
    pub run_id: RunId,
    pub identity: RunIdentity,
    pub revision: CheckpointRevision,
    pub state: RunState,
    pub policy: RigExecutionPolicy,
    pub messages: Vec<ConversationMessage>,
    /// Unknown requests remain here and remain charged across recovery.
    pub reservations: Vec<ModelReservation>,
    pub turns: Vec<SavedModelTurn>,
    pub invocations: Vec<SavedInvocation>,
    pub final_output: Option<AgentExecutionOutput>,
    pub wait: Option<ApprovalWait>,
    pub outreach_wait: Option<OutreachWait>,
    pub executions: Vec<ExecutionReservation>,
    pub tool_catalogue_fingerprint: Option<String>,
    #[serde(default)]
    pub contract_fingerprint: Option<crate::entities::response_contract::ContractFingerprint>,
}

pub(crate) enum Mutation {
    InvalidOutput,
    Execution(ExecutionReservation),
    BindTools(String),
    BeginRemote(InvocationId, SavedMcpInvocation),
    Reserve(ModelReservation),
    Model(SavedModelTurn),
    Prepare(InvocationId),
    Result(InvocationId, Value),
    Final(AgentExecutionOutput),
}

impl RunCheckpoint {
    pub fn new(
        identity: RunIdentity,
        prompt: String,
        policy: RigExecutionPolicy,
    ) -> AppResult<Self> {
        let checkpoint = Self {
            contract_fingerprint: identity
                .response_contract
                .as_ref()
                .map(|contract| contract.fingerprint()),
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            run_id: RunId(Uuid::new_v4()),
            identity,
            revision: CheckpointRevision(0),
            state: RunState::Active,
            policy,
            messages: vec![ConversationMessage::User { text: prompt }],
            reservations: vec![],
            turns: vec![],
            invocations: vec![],
            final_output: None,
            wait: None,
            outreach_wait: None,
            executions: vec![],
            tool_catalogue_fingerprint: None,
        };
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn validate(&self) -> AppResult<()> {
        self.policy.validate()?;
        if self.contract_fingerprint
            != self
                .identity
                .response_contract
                .as_ref()
                .map(|contract| contract.fingerprint())
        {
            return Err(invalid("Contract snapshot fingerprint mismatch"));
        }
        let mut generations = std::collections::HashSet::new();
        if self.executions.len() > usize::from(MAX_CONTINUATIONS)
            || self.executions.iter().any(|claim| {
                claim.allowance_ms == 0
                    || claim.allowance_ms > MAX_CLAIM_MS
                    || !generations.insert(claim.generation)
            })
            || self
                .executions
                .iter()
                .try_fold(0u64, |sum, claim| sum.checked_add(claim.allowance_ms))
                .is_none_or(|sum| sum > MAX_ACTIVE_EXECUTION_MS)
        {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::Budget,
            ));
        }
        if self.schema_version != CHECKPOINT_SCHEMA_VERSION
            || self.identity.provider.is_empty()
            || self.identity.provider.len() > 64
            || self.identity.model.is_empty()
            || self.identity.model.len() > 256
            || self.identity.capability_fingerprint.len() != 64
            || !self
                .identity
                .capability_fingerprint
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || self.reservations.len() > usize::from(self.policy.model_calls)
            || self.turns.len() > self.reservations.len()
            || self.invocations.len() > usize::from(self.policy.tool_calls)
            || serde_json::to_vec_pretty(self)
                .map_err(|_| invalid("Invalid checkpoint"))?
                .len()
                > MAX_CHECKPOINT_BYTES
        {
            return Err(invalid(
                "Unsupported, malformed or oversized harness checkpoint",
            ));
        }
        if self.repair_count() > crate::services::response_contract::MAX_REPAIR_CALLS {
            return Err(crate::services::response_contract::invalid_output());
        }
        self.validate_response_ledger()?;
        self.validate_ledger()?;
        self.validate_conversation()?;
        Ok(())
    }

    fn validate_response_ledger(&self) -> AppResult<()> {
        let mut repairing = false;
        for (index, reservation) in self.reservations.iter().enumerate() {
            if let Some(repair) = &reservation.repair {
                repairing = true;
                let candidate = self
                    .turns
                    .iter()
                    .find(|turn| turn.request_id == repair.candidate);
                if self.identity.response_contract.is_none()
                    || !self.reservations[..index]
                        .iter()
                        .any(|request| request.request_id == repair.candidate)
                    || candidate.is_none_or(|turn| {
                        turn.invalid_response != Some(repair.reason) || !turn.calls.is_empty()
                    })
                {
                    return Err(invalid("Invalid repair reservation ledger"));
                }
                if self
                    .turns
                    .iter()
                    .find(|turn| turn.request_id == reservation.request_id)
                    .is_some_and(|turn| !turn.calls.is_empty())
                {
                    return Err(invalid("Repair response contains executable calls"));
                }
            } else if repairing {
                return Err(invalid("Ordinary generation follows repair"));
            }
        }
        if self.turns.iter().any(|turn| {
            turn.invalid_response.is_some()
                && (self.identity.response_contract.is_none() || !turn.calls.is_empty())
        }) {
            return Err(invalid("Invalid candidate assessment"));
        }
        if self.state == RunState::InvalidOutput
            && (self.repair_count() != crate::services::response_contract::MAX_REPAIR_CALLS
                || self
                    .turns
                    .last()
                    .is_none_or(|turn| turn.invalid_response.is_none()))
        {
            return Err(invalid("Invalid terminal response ledger"));
        }
        if let Some(output) = &self.final_output {
            match (&self.identity.response_contract, &output.structured) {
                (None, None) => {}
                (Some(contract), Some(response))
                    if response.contract() == contract && response.body() == output.content => {}
                _ => return Err(crate::services::response_contract::invalid_output()),
            }
        }
        Ok(())
    }

    fn validate_ledger(&self) -> AppResult<()> {
        let input = self.reservations.iter().try_fold(0u64, |sum, reservation| {
            sum.checked_add(reservation.input_tokens)
        });
        let output = self.reservations.iter().try_fold(0u64, |sum, reservation| {
            sum.checked_add(reservation.output_tokens)
        });
        if input.is_none_or(|n| n > self.policy.total_input_tokens)
            || output.is_none_or(|n| n > self.policy.total_output_tokens)
            || (self.state == RunState::Waiting)
                != (self.wait.is_some() || self.outreach_wait.is_some())
            || (self.wait.is_some() && self.outreach_wait.is_some())
            || (self.state == RunState::Completed && self.final_output.is_none())
            || (self.final_output.is_some()
                && !matches!(self.state, RunState::Completed | RunState::Superseded))
        {
            return Err(invalid("Inconsistent checkpoint state or budget ledger"));
        }
        let mut requests = std::collections::HashSet::new();
        for reservation in &self.reservations {
            if !requests.insert(reservation.request_id.0)
                || reservation.input_tokens == 0
                || reservation.output_tokens == 0
                || reservation.input_tokens > self.policy.request_input_tokens
                || reservation.output_tokens > self.policy.request_output_tokens
            {
                return Err(invalid("Invalid saved model reservation"));
            }
        }
        let mut turns = std::collections::HashSet::new();
        let mut calls = std::collections::HashSet::new();
        let mut call_count = 0;
        for turn in &self.turns {
            let Some(reservation) = self
                .reservations
                .iter()
                .find(|r| r.request_id == turn.request_id)
            else {
                return Err(invalid("Unreserved saved model response"));
            };
            if !turns.insert(turn.request_id.0)
                || turn.input_tokens > reservation.input_tokens
                || turn.output_tokens > reservation.output_tokens
            {
                return Err(invalid("Invalid saved response usage"));
            }
            call_count += turn.calls.len();
            for call in &turn.calls {
                if !calls.insert(call.invocation_id.0)
                    || call.call_id.is_empty()
                    || call.call_id.len() > 256
                    || !call.arguments.is_object()
                    || call.arguments.to_string().len() > MAX_ARGUMENT_BYTES
                {
                    return Err(invalid("Invalid saved invocation identity or arguments"));
                }
            }
        }
        if call_count > usize::from(self.policy.tool_calls) {
            return Err(invalid("Saved tool budget exhausted"));
        }
        let mut receipts = std::collections::HashSet::new();
        for invocation in &self.invocations {
            let call = self
                .turns
                .get(usize::from(invocation.turn))
                .and_then(|turn| turn.calls.get(usize::from(invocation.ordinal)));
            if call != Some(&invocation.call)
                || !receipts.insert(invocation.call.invocation_id.0)
                || (invocation.state == InvocationState::Completed) != invocation.result.is_some()
                || invocation
                    .result
                    .as_ref()
                    .is_some_and(|result| result.to_string().len() > MAX_RESULT_BYTES)
            {
                return Err(invalid("Inconsistent saved invocation receipt"));
            }
        }
        Ok(())
    }

    fn validate_conversation(&self) -> AppResult<()> {
        let Some(ConversationMessage::User { text }) = self.messages.first() else {
            return Err(invalid("Missing initial prompt"));
        };
        let mut expected = vec![ConversationMessage::User { text: text.clone() }];
        let mut unfinished = false;
        for turn in &self.turns {
            if unfinished {
                return Err(invalid("Model response precedes tool results"));
            }
            expected.push(ConversationMessage::Assistant {
                text: turn.text.clone(),
                calls: turn.calls.clone(),
                continuation: turn.continuation.clone(),
            });
            for call in &turn.calls {
                let result = self
                    .invocations
                    .iter()
                    .find(|inv| inv.call.invocation_id == call.invocation_id)
                    .and_then(|inv| inv.result.as_ref());
                if let Some(result) = result {
                    if unfinished {
                        return Err(invalid("Tool receipts are out of order"));
                    }
                    expected.push(ConversationMessage::Tool {
                        invocation_id: call.invocation_id,
                        call_id: call.call_id.clone(),
                        item_id: call.item_id.clone(),
                        result: result.clone(),
                    });
                } else {
                    unfinished = true;
                }
            }
        }
        if self.messages != expected {
            return Err(invalid("Conversation differs from its committed ledger"));
        }
        Ok(())
    }

    pub fn pending_call(&self) -> Option<&SavedToolCall> {
        self.turns.last()?.calls.iter().find(|call| {
            !self.invocations.iter().any(|inv| {
                inv.call.invocation_id == call.invocation_id
                    && inv.state == InvocationState::Completed
            })
        })
    }

    /// Mutate a clone so a failed validation cannot partially change the caller's checkpoint.
    pub(crate) fn apply(&self, mutation: Mutation) -> AppResult<(Self, bool)> {
        let mut next = self.clone();
        let changed = match mutation {
            Mutation::InvalidOutput => {
                if next.state == RunState::InvalidOutput {
                    false
                } else {
                    next.require_active()?;
                    next.state = RunState::InvalidOutput;
                    true
                }
            }
            Mutation::Execution(mut reservation) => {
                next.require_active()?;
                if next
                    .executions
                    .iter()
                    .any(|claim| claim.generation == reservation.generation)
                {
                    false
                } else {
                    reservation.replayed_invocations = next
                        .invocations
                        .iter()
                        .filter(|inv| inv.state == InvocationState::Completed)
                        .count() as u16;
                    next.executions.push(reservation);
                    true
                }
            }
            Mutation::BindTools(fingerprint) => next.bind_tools(fingerprint)?,
            Mutation::BeginRemote(id, identity) => next.begin_remote(id, identity)?,
            Mutation::Reserve(value) => next.reserve(value)?,
            Mutation::Model(value) => next.commit_turn(value)?,
            Mutation::Prepare(id) => next.prepare(id)?,
            Mutation::Result(id, value) => next.result(id, value)?,
            Mutation::Final(output) => next.finish(output)?,
        };
        if changed {
            next.revision = CheckpointRevision(
                self.revision
                    .0
                    .checked_add(1)
                    .ok_or_else(|| invalid("Checkpoint revision exhausted"))?,
            );
        }
        next.validate()?;
        Ok((next, changed))
    }

    fn require_active(&self) -> AppResult<()> {
        if self.state != RunState::Active {
            return Err(invalid("Continuation is not active"));
        }
        Ok(())
    }

    fn reserve(&mut self, value: ModelReservation) -> AppResult<bool> {
        if let Some(saved) = self
            .reservations
            .iter()
            .find(|saved| saved.request_id == value.request_id)
        {
            return if saved == &value {
                Ok(false)
            } else {
                Err(invalid("Model reservation identity collision"))
            };
        }
        self.require_active()?;
        self.validate_repair_reservation(&value)?;
        let input = self
            .reservations
            .iter()
            .try_fold(value.input_tokens, |sum, item| {
                sum.checked_add(item.input_tokens)
            });
        let output = self
            .reservations
            .iter()
            .try_fold(value.output_tokens, |sum, item| {
                sum.checked_add(item.output_tokens)
            });
        if self.pending_call().is_some()
            || self.reservations.len() >= usize::from(self.policy.model_calls)
            || value.input_tokens == 0
            || value.input_tokens > self.policy.request_input_tokens
            || value.output_tokens == 0
            || value.output_tokens > self.policy.request_output_tokens
            || input.is_none_or(|n| n > self.policy.total_input_tokens)
            || output.is_none_or(|n| n > self.policy.total_output_tokens)
        {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::Budget,
            ));
        }
        self.reservations.push(value);
        Ok(true)
    }

    pub fn repair_count(&self) -> usize {
        self.reservations
            .iter()
            .filter(|reservation| reservation.repair.is_some())
            .count()
    }

    fn validate_repair_reservation(&self, reservation: &ModelReservation) -> AppResult<()> {
        let candidate = self
            .turns
            .last()
            .filter(|turn| turn.invalid_response.is_some());
        match (&reservation.repair, candidate) {
            (None, None) if self.repair_count() == 0 => Ok(()),
            (Some(repair), Some(candidate))
                if self.identity.response_contract.is_some()
                    && repair.candidate == candidate.request_id
                    && Some(repair.reason) == candidate.invalid_response
                    && self.repair_count()
                        < crate::services::response_contract::MAX_REPAIR_CALLS =>
            {
                Ok(())
            }
            _ => Err(crate::services::response_contract::invalid_output()),
        }
    }

    fn commit_turn(&mut self, value: SavedModelTurn) -> AppResult<bool> {
        if let Some(saved) = self
            .turns
            .iter()
            .find(|saved| saved.request_id == value.request_id)
        {
            return if saved == &value {
                Ok(false)
            } else {
                Err(invalid("Model response identity collision"))
            };
        }
        self.require_active()?;
        let reservation = self
            .reservations
            .last()
            .filter(|saved| saved.request_id == value.request_id)
            .ok_or_else(|| invalid("Model response has no current reservation"))?;
        if self.pending_call().is_some()
            || value.input_tokens > reservation.input_tokens
            || value.output_tokens > reservation.output_tokens
            || self.invocations.len() + value.calls.len() > usize::from(self.policy.tool_calls)
        {
            return Err(invalid("Invalid or over-budget model response"));
        }
        if reservation.repair.is_some() && !value.calls.is_empty() {
            return Err(crate::services::response_contract::invalid_output());
        }
        if value.invalid_response.is_some()
            && (self.identity.response_contract.is_none() || !value.calls.is_empty())
        {
            return Err(invalid("Invalid final candidate assessment"));
        }
        self.validate_turn(&value)?;
        self.messages.push(ConversationMessage::Assistant {
            text: value.text.clone(),
            calls: value.calls.clone(),
            continuation: value.continuation.clone(),
        });
        let exhausted = value.invalid_response.is_some()
            && self.repair_count() >= crate::services::response_contract::MAX_REPAIR_CALLS;
        self.turns.push(value);
        if exhausted {
            self.state = RunState::InvalidOutput;
        }
        Ok(true)
    }

    fn validate_turn(&self, turn: &SavedModelTurn) -> AppResult<()> {
        let mut calls = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for call in &turn.calls {
            if call.call_id.is_empty()
                || call.call_id.len() > 256
                || call
                    .item_id
                    .as_ref()
                    .is_some_and(|id| id.is_empty() || id.len() > 256)
                || call.tool_id.is_empty()
                || call.tool_id.len() > 256
                || !call.arguments.is_object()
                || call.arguments.to_string().len() > MAX_ARGUMENT_BYTES
                || !calls.insert(&call.call_id)
                || !ids.insert(call.invocation_id.0)
                || self
                    .turns
                    .iter()
                    .flat_map(|turn| &turn.calls)
                    .any(|old| old.invocation_id == call.invocation_id)
            {
                return Err(invalid("Invalid or duplicated provider tool call"));
            }
        }
        for block in &turn.continuation {
            if block.schema_version != 1
                || block.provider != self.identity.provider
                || block.kind.is_empty()
                || block.kind.len() > 64
                || block.data.to_string().len() > MAX_RESULT_BYTES
            {
                return Err(invalid("Unsupported provider continuation block"));
            }
        }
        Ok(())
    }

    fn prepare(&mut self, id: InvocationId) -> AppResult<bool> {
        if self
            .invocations
            .iter()
            .any(|saved| saved.call.invocation_id == id)
        {
            return Ok(false);
        }
        self.require_active()?;
        let call = self
            .pending_call()
            .filter(|call| call.invocation_id == id)
            .cloned()
            .ok_or_else(|| invalid("Invocation is not the next saved call"))?;
        let turn = self.turns.len() - 1;
        let ordinal = self.turns[turn]
            .calls
            .iter()
            .position(|item| item.invocation_id == id)
            .ok_or_else(|| invalid("Missing saved call"))?;
        self.invocations.push(SavedInvocation {
            call,
            turn: turn as u16,
            ordinal: ordinal as u16,
            state: InvocationState::Prepared,
            result: None,
            mcp: None,
        });
        Ok(true)
    }

    fn result(&mut self, id: InvocationId, result: Value) -> AppResult<bool> {
        if result.to_string().len() > MAX_RESULT_BYTES {
            return Err(invalid("Tool result exceeds checkpoint bound"));
        }
        if let Some(saved) = self.invocations.iter().find(|saved| {
            saved.call.invocation_id == id && saved.state == InvocationState::Completed
        }) {
            return if saved.result.as_ref() == Some(&result) {
                Ok(false)
            } else {
                Err(invalid("Invocation result identity collision"))
            };
        }
        self.require_active()?;
        if self
            .pending_call()
            .is_none_or(|call| call.invocation_id != id)
        {
            return Err(invalid("Result is not for the next saved call"));
        }
        let invocation = self
            .invocations
            .iter_mut()
            .find(|saved| {
                saved.call.invocation_id == id
                    && matches!(
                        saved.state,
                        InvocationState::Prepared
                            | InvocationState::Ready
                            | InvocationState::Indeterminate
                    )
            })
            .ok_or_else(|| invalid("Invocation was not prepared"))?;
        self.messages.push(ConversationMessage::Tool {
            invocation_id: id,
            call_id: invocation.call.call_id.clone(),
            item_id: invocation.call.item_id.clone(),
            result: result.clone(),
        });
        invocation.result = Some(result);
        invocation.state = InvocationState::Completed;
        Ok(true)
    }

    fn finish(&mut self, output: AgentExecutionOutput) -> AppResult<bool> {
        if let Some(saved) = &self.final_output {
            return if saved == &output {
                Ok(false)
            } else {
                Err(invalid("Final output identity collision"))
            };
        }
        self.require_active()?;
        if self.turns.is_empty()
            || self.pending_call().is_some()
            || output.disposition != super::super::AgentExecutionDisposition::Completed
            || serde_json::to_vec(&output)
                .map_err(|_| invalid("Invalid final output"))?
                .len()
                > MAX_RESULT_BYTES
        {
            return Err(invalid("Unfinished or oversized final output"));
        }
        match (&self.identity.response_contract, &output.structured) {
            (None, None) => {}
            (Some(contract), Some(structured))
                if structured.contract() == contract && structured.body() == output.content => {}
            _ => return Err(crate::services::response_contract::invalid_output()),
        }
        self.final_output = Some(output);
        self.state = RunState::Completed;
        Ok(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalWait {
    pub approval_id: Uuid,
    pub cycle_id: Uuid,
    pub invocation_id: InvocationId,
}

impl RunCheckpoint {
    pub(crate) fn park_approval(&self, wait: ApprovalWait) -> AppResult<Self> {
        if self.wait.as_ref() == Some(&wait) && self.state == RunState::Waiting {
            return Ok(self.clone());
        }
        self.require_active()?;
        if self
            .pending_call()
            .is_none_or(|call| call.invocation_id != wait.invocation_id)
        {
            return Err(invalid("Approval does not match the current invocation"));
        }
        let mut next = self.clone();
        let invocation = next
            .invocations
            .iter_mut()
            .find(|inv| {
                inv.call.invocation_id == wait.invocation_id
                    && matches!(
                        inv.state,
                        InvocationState::Prepared | InvocationState::Ready
                    )
            })
            .ok_or_else(|| invalid("Approval invocation is not prepared"))?;
        invocation.state = InvocationState::Waiting;
        next.state = RunState::Waiting;
        next.wait = Some(wait);
        next.revision.0 = next
            .revision
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Checkpoint revision exhausted"))?;
        next.validate()?;
        Ok(next)
    }

    pub(crate) fn resolve_approval(&self, wait: &ApprovalWait, approved: bool) -> AppResult<Self> {
        if self.wait.as_ref() != Some(wait) || self.state != RunState::Waiting {
            return Err(invalid(
                "Approval does not match the current continuation wait",
            ));
        }
        let mut next = self.clone();
        let invocation = next
            .invocations
            .iter_mut()
            .find(|inv| {
                inv.call.invocation_id == wait.invocation_id
                    && inv.state == InvocationState::Waiting
            })
            .ok_or_else(|| invalid("Missing waiting invocation"))?;
        let checkpoint = invocation.call.tool_id.as_str()
            == crate::entities::tool_catalogue::REQUEST_APPROVAL_TOOL_ID;
        invocation.state = if approved {
            InvocationState::Ready
        } else {
            InvocationState::Failed
        };
        next.wait = None;
        next.state = if approved {
            RunState::Active
        } else {
            RunState::Superseded
        };
        if approved && checkpoint {
            next.result(
                wait.invocation_id,
                serde_json::json!({"status":"approved", "approval_id":wait.approval_id}),
            )?;
        }
        next.revision.0 = next
            .revision
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Checkpoint revision exhausted"))?;
        next.validate()?;
        Ok(next)
    }
}

impl RunCheckpoint {
    pub(crate) fn supersede(&self) -> AppResult<Self> {
        let mut next = self.clone();
        next.state = RunState::Superseded;
        next.wait = None;
        next.outreach_wait = None;
        for invocation in &mut next.invocations {
            if matches!(
                invocation.state,
                InvocationState::Prepared | InvocationState::Ready | InvocationState::Waiting
            ) {
                invocation.state = InvocationState::Failed;
            }
        }
        next.revision.0 = next
            .revision
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Checkpoint revision exhausted"))?;
        next.validate()?;
        Ok(next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutreachWait {
    pub outreach_id: Uuid,
    pub invocation_id: InvocationId,
}

impl RunCheckpoint {
    pub(crate) fn park_outreach(&self, wait: OutreachWait) -> AppResult<Self> {
        self.require_active()?;
        if self
            .pending_call()
            .is_none_or(|call| call.invocation_id != wait.invocation_id)
        {
            return Err(invalid("Outreach is not the current invocation"));
        }
        let mut next = self.clone();
        let invocation = next
            .invocations
            .iter_mut()
            .find(|inv| {
                inv.call.invocation_id == wait.invocation_id
                    && matches!(
                        inv.state,
                        InvocationState::Prepared | InvocationState::Ready
                    )
            })
            .ok_or_else(|| invalid("Outreach invocation was not prepared"))?;
        invocation.state = InvocationState::Waiting;
        next.state = RunState::Waiting;
        next.outreach_wait = Some(wait);
        next.revision.0 = next
            .revision
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Checkpoint revision exhausted"))?;
        next.validate()?;
        Ok(next)
    }

    pub(crate) fn resolve_outreach(&self, wait: &OutreachWait, result: Value) -> AppResult<Self> {
        if self.outreach_wait.as_ref() != Some(wait) || self.state != RunState::Waiting {
            return Err(invalid("Outreach does not match the saved wait"));
        }
        let mut next = self.clone();
        let invocation = next
            .invocations
            .iter_mut()
            .find(|inv| {
                inv.call.invocation_id == wait.invocation_id
                    && inv.state == InvocationState::Waiting
            })
            .ok_or_else(|| invalid("Missing waiting outreach invocation"))?;
        invocation.state = InvocationState::Ready;
        next.state = RunState::Active;
        next.outreach_wait = None;
        next.result(wait.invocation_id, result)?;
        next.revision.0 = next
            .revision
            .0
            .checked_add(1)
            .ok_or_else(|| invalid("Checkpoint revision exhausted"))?;
        next.validate()?;
        Ok(next)
    }
}

impl RunCheckpoint {
    fn bind_tools(&mut self, fingerprint: String) -> AppResult<bool> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("Invalid tool catalogue fingerprint"));
        }
        if let Some(saved) = &self.tool_catalogue_fingerprint {
            return if saved == &fingerprint {
                Ok(false)
            } else {
                Err(invalid(
                    "Tool schemas or selection revisions changed during continuation",
                ))
            };
        }
        self.require_active()?;
        if !self.reservations.is_empty() {
            return Err(invalid("Cannot change tools after model work begins"));
        }
        self.tool_catalogue_fingerprint = Some(fingerprint);
        Ok(true)
    }

    fn begin_remote(&mut self, id: InvocationId, identity: SavedMcpInvocation) -> AppResult<bool> {
        self.require_active()?;
        let invocation = self
            .invocations
            .iter_mut()
            .find(|inv| inv.call.invocation_id == id)
            .ok_or_else(|| invalid("Remote invocation is not prepared"))?;
        if invocation.state == InvocationState::Indeterminate {
            return Err(AppError::Execution(
                crate::app_error::ExecutionFailure::IndeterminateEffect,
            ));
        }
        if invocation.state != InvocationState::Prepared || invocation.mcp.is_some() {
            return Err(invalid("Remote invocation is not dispatchable"));
        }
        invocation.mcp = Some(identity);
        // Mark uncertainty before crossing the remote boundary. A crash cannot authorize replay.
        invocation.state = InvocationState::Indeterminate;
        Ok(true)
    }
}
