//! The durable model loop a workflow model step drives. The lower-level completion API
//! provides the transaction boundary an agent framework cannot: capture and commit the *whole*
//! assistant turn before the first tool is invoked.
use crate::adapters::response_schema::JsonResponseValidator;
use crate::services::response_contract::StructuredResponse;
use crate::{
    app_error::{AppError, AppResult},
    services::harness::{AgentExecutionOutput, runs::*, sanitize_text},
};
use rig::{agent::ModelHandle, completion::CompletionRequest};
use sha2::{Digest, Sha256};

pub(super) fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
pub(super) fn applied(outcome: WriteOutcome<RunCheckpoint>) -> AppResult<RunCheckpoint> {
    match outcome {
        WriteOutcome::Applied(run) | WriteOutcome::AlreadyApplied(run) => Ok(run),
        WriteOutcome::OwnershipLost => Err(AppError::Execution(
            crate::app_error::ExecutionFailure::OwnershipLost,
        )),
        WriteOutcome::RevisionConflict => Err(AppError::Conflict(
            "Rig checkpoint writer superseded".into(),
        )),
    }
}

/// The durable owner of one checkpoint: the workflow model step. It supplies the authority,
/// checkpoint writes and tool dispatch; the drive loop below is the shared mechanism.
pub(super) type Binding<'a> = Box<super::workflow::WorkflowBinding<'a>>;

/// A durable admission either enters the provider or parks the execution before I/O.
pub(super) enum Admitted {
    Enter(Box<RunCheckpoint>),
    Suspend(crate::domain::workflow::WaitId),
}

/// The next state after a durable write: keep driving, or stop because the owner parked.
pub(super) enum Progress {
    Continue(RunCheckpoint),
    Suspend(RunCheckpoint, crate::domain::workflow::WaitId),
}

pub(super) enum Driven {
    Completed(AgentExecutionOutput),
    Suspended(crate::domain::workflow::WaitId),
}

/// Canonical provider request identity: provider, model and the complete completion
/// request (frozen preamble, saved history, tool declarations, repair, limits). The
/// model handle carries credentials; they never enter this digest.
pub(super) fn request_fingerprint(
    identity: &RunIdentity,
    request: &CompletionRequest,
) -> AppResult<[u8; 32]> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "provider": identity.provider,
        "model": identity.model,
        "request": request,
    }))
    .map_err(|_| invalid("Invalid Rig request"))?;
    Ok(Sha256::digest(canonical).into())
}

pub(super) struct Execution<'a> {
    pub(super) inputs: super::diagnostics::InputCounts,
    pub(super) binding: Binding<'a>,
    pub(super) checkpoint: RunCheckpoint,
    pub(super) model: ModelHandle,
    pub(super) deadline: tokio::time::Instant,
    pub(super) secret: &'a str,
}
impl Execution<'_> {
    pub(super) async fn drive(&mut self) -> AppResult<Driven> {
        loop {
            if tokio::time::Instant::now() >= self.deadline {
                return Err(AppError::Timeout("Rig deadline exceeded".into()));
            }
            if let Some(output) = &self.checkpoint.final_output {
                return Ok(Driven::Completed(output.clone()));
            }
            if self.checkpoint.state == RunState::BudgetExceeded {
                return Err(AppError::Execution(
                    crate::app_error::ExecutionFailure::Budget,
                ));
            }
            if self.checkpoint.state == RunState::InvalidOutput {
                return Err(crate::services::response_contract::invalid_output());
            }
            if self.checkpoint.state != RunState::Active {
                return Err(invalid("Rig continuation is not executable"));
            }
            if let Some(call) = self.checkpoint.pending_call().cloned() {
                if self.checkpoint.invocations.iter().any(|inv| {
                    inv.call.invocation_id == call.invocation_id
                        && inv.state == InvocationState::Indeterminate
                }) {
                    return Err(AppError::Execution(
                        crate::app_error::ExecutionFailure::IndeterminateEffect,
                    ));
                }
                match self.binding.tool(&self.checkpoint, call).await? {
                    Progress::Continue(checkpoint) => self.checkpoint = checkpoint,
                    Progress::Suspend(checkpoint, wait) => {
                        self.checkpoint = checkpoint;
                        return Ok(Driven::Suspended(wait));
                    }
                }
                continue;
            }
            if let Some(turn) = self.checkpoint.turns.last()
                && turn.calls.is_empty()
            {
                if turn.invalid_response.is_some() {
                    if self.checkpoint.repair_count()
                        >= crate::services::response_contract::MAX_REPAIR_CALLS
                    {
                        self.checkpoint = self.binding.invalid_output(&self.checkpoint).await?;
                        return Err(crate::services::response_contract::invalid_output());
                    }
                    if let Some(stop) = self.model_turn().await? {
                        return Ok(stop);
                    }
                    continue;
                }
                let output = self.final_candidate(&turn.text)?;
                self.checkpoint = self
                    .binding
                    .final_output(&self.checkpoint, output.clone())
                    .await?;
                return Ok(Driven::Completed(output));
            }
            if let Some(stop) = self.model_turn().await? {
                return Ok(stop);
            }
        }
    }

    fn final_candidate(&self, text: &str) -> AppResult<AgentExecutionOutput> {
        let mut output = self.output(sanitize_text(text, Some(self.secret)));
        if let Some(contract) = &self.checkpoint.identity.response_contract {
            let structured = StructuredResponse::validate(
                contract,
                text,
                Some(self.secret),
                &JsonResponseValidator,
            )?
            .map_err(|_| crate::services::response_contract::invalid_output())?;
            output = self.output(structured.body().into());
            output.structured = Some(structured);
        }
        Ok(output)
    }

    /// Some when the owner parked instead of entering the provider, or instead of
    /// progressing past a settled response.
    async fn model_turn(&mut self) -> AppResult<Option<Driven>> {
        let repair = super::final_response::repair_reservation(&self.checkpoint)?;
        let request = CompletionRequest {
            model: None,
            preamble: Some(super::final_response::response_preamble(
                &self.checkpoint,
                self.binding.preamble(),
                repair.as_ref(),
            )?),
            chat_history: super::protocol::history(&self.checkpoint)?,
            documents: vec![],
            tools: if repair.is_some() {
                vec![]
            } else {
                self.binding.definitions()
            },
            temperature: None,
            max_tokens: Some(self.checkpoint.policy.request_output_tokens),
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
        };
        request
            .validate_message_content()
            .map_err(|_| invalid("Invalid Rig model history"))?;
        // One UTF-8 byte per token is a conservative fallback, covering schemas and metadata too.
        let input_tokens = serde_json::to_vec(&request)
            .map_err(|_| invalid("Invalid Rig request"))?
            .len() as u64;
        let fingerprint = request_fingerprint(&self.checkpoint.identity, &request)?;
        let reservation = self.binding.reservation(ModelReservation {
            repair,
            request_id: ModelRequestId(uuid::Uuid::new_v4()),
            input_tokens,
            output_tokens: self.checkpoint.policy.request_output_tokens,
        })?;
        match self
            .binding
            .admit(&self.checkpoint, reservation.clone(), fingerprint)
            .await?
        {
            Admitted::Enter(checkpoint) => self.checkpoint = *checkpoint,
            Admitted::Suspend(wait) => return Ok(Some(Driven::Suspended(wait))),
        }
        let response = super::providers::complete(&self.model, request).await?;
        let mut turn =
            super::protocol::capture(&self.checkpoint.identity.provider, &reservation, response)?;
        super::final_response::assess_candidate(
            &self.checkpoint,
            self.secret,
            &mut turn,
            reservation.repair.as_ref(),
            super::diagnostics::OutputContext {
                inputs: self.inputs,
                supported_tools: self.binding.supported_ids(),
            },
        )?;
        match self.binding.settle(&self.checkpoint, turn).await? {
            Progress::Continue(checkpoint) => {
                self.checkpoint = checkpoint;
                Ok(None)
            }
            Progress::Suspend(checkpoint, wait) => {
                self.checkpoint = checkpoint;
                Ok(Some(Driven::Suspended(wait)))
            }
        }
    }

    pub(super) fn output(&self, content: String) -> AgentExecutionOutput {
        super::diagnostics::output(
            &self.checkpoint,
            self.inputs,
            self.binding.supported_ids(),
            content,
        )
    }
}
