//! Non-durable runs share the candidate gate and ledger ceilings, without durable tools or waits.
use super::{
    compile::{CompileContext, CompiledRun},
    execution::{RigHarness, execution_policy},
    final_response, protocol,
    tools::ToolCorrelationId,
};
use crate::adapters::response_schema::JsonResponseValidator;
use crate::{
    app_error::{AppError, AppResult},
    entities::{harness::HarnessKind, transport::RecipientRole},
    services::{
        harness::{
            AgentExecutionDisposition, AgentExecutionOutput, AgentRun,
            context::{RuntimeContext, SystemClock},
            runs::*,
            sanitize_text,
        },
        response_contract::{ResponseContractValidator, StructuredResponse, invalid_output},
    },
};
use rig::{agent::ModelHandle, completion::CompletionRequest};
use std::sync::Arc;

pub(super) async fn execute(
    harness: &RigHarness,
    run: AgentRun<'_>,
) -> AppResult<AgentExecutionOutput> {
    let company_id = run.company_id.ok_or_else(|| {
        AppError::BadRequest("Direct Rig execution requires company context".into())
    })?;
    if let Some(contract) = &run.spec.response_contract {
        JsonResponseValidator.validate_contract(contract)?;
    }
    let policy = execution_policy(&run)?;
    let mut compiled = CompiledRun::compile(
        &run.spec,
        CompileContext {
            facts: RuntimeContext {
                company_id,
                agent_id: run.agent_id,
                recipient_role: run.recipient_role.unwrap_or(RecipientRole::To),
                timezone: chrono_tz::UTC,
            },
            clock: Arc::new(SystemClock),
            capabilities: harness.capabilities.clone(),
            token_budget: policy.total_input_tokens as usize,
            approvals: None,
            mcp: None,
        },
        run.tool_host.clone(),
    )?;
    if !compiled.tools.diagnostics.missing_context.is_empty() {
        return Err(AppError::BadRequest(
            "Direct Rig tools require unavailable context".into(),
        ));
    }
    let tools = Arc::get_mut(&mut compiled.tools)
        .ok_or_else(|| AppError::Internal("Rig tools shared before execution".into()))?;
    tools.restrict_deadline(run.deadline);
    tools.set_trace(run.trace.clone());
    let model = harness.model(&run)?;
    let checkpoint = RunCheckpoint::new(
        RunIdentity {
            company_id,
            task_id: uuid::Uuid::nil(),
            agent_id: run.agent_id,
            harness: HarnessKind::Rig,
            provider: run.spec.provider.clone(),
            model: run.spec.model.clone(),
            capability_fingerprint: "0".repeat(64),
            response_contract: run.spec.response_contract.clone(),
        },
        run.full_prompt.into(),
        policy,
    )?;
    Box::pin(
        Direct {
            inputs: super::diagnostics::InputCounts::from(&run),
            compiled,
            checkpoint,
            model,
            secret: run.api_key,
            deadline: run.deadline,
        }
        .drive(),
    )
    .await
}

struct Direct<'a> {
    inputs: super::diagnostics::InputCounts,
    compiled: CompiledRun,
    checkpoint: RunCheckpoint,
    model: ModelHandle,
    secret: &'a str,
    deadline: tokio::time::Instant,
}
impl Direct<'_> {
    async fn drive(mut self) -> AppResult<AgentExecutionOutput> {
        loop {
            if tokio::time::Instant::now() >= self.deadline {
                return Err(AppError::Timeout("Rig deadline exceeded".into()));
            }
            if let Some(call) = self.checkpoint.pending_call().cloned() {
                self.apply(Mutation::Prepare(call.invocation_id))?;
                let result = self
                    .compiled
                    .tools
                    .invoke(
                        &call.tool_id,
                        &ToolCorrelationId::parse(&call.invocation_id.0.to_string())?,
                        call.arguments,
                    )
                    .await?;
                if result.suspends_run() {
                    return Err(AppError::BadRequest(
                        "Direct execution cannot suspend".into(),
                    ));
                }
                self.apply(Mutation::Result(call.invocation_id, result.output))?;
                continue;
            }
            if let Some(turn) = self.checkpoint.turns.last()
                && turn.calls.is_empty()
                && turn.invalid_response.is_none()
            {
                let output = self.output(&turn.text)?;
                self.apply(Mutation::Final(output.clone()))?;
                return Ok(output);
            }
            self.model_turn().await?;
        }
    }
    fn apply(&mut self, mutation: Mutation) -> AppResult<()> {
        self.checkpoint = self.checkpoint.apply(mutation)?.0;
        Ok(())
    }
    async fn model_turn(&mut self) -> AppResult<()> {
        let repair = final_response::repair_reservation(&self.checkpoint)?;
        let request = CompletionRequest {
            model: None,
            preamble: Some(final_response::response_preamble(
                &self.checkpoint,
                &self.compiled,
                repair.as_ref(),
            )?),
            chat_history: protocol::history(&self.checkpoint)?,
            documents: vec![],
            tools: if repair.is_some() {
                vec![]
            } else {
                self.compiled.tools.definitions()
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
            .map_err(|_| invalid_output())?;
        let reservation = ModelReservation {
            repair,
            request_id: ModelRequestId(uuid::Uuid::new_v4()),
            input_tokens: serde_json::to_vec(&request)
                .map_err(|_| invalid_output())?
                .len() as u64,
            output_tokens: self.checkpoint.policy.request_output_tokens,
        };
        self.apply(Mutation::Reserve(reservation.clone()))?;
        let response = super::providers::complete(&self.model, request).await?;
        let mut turn =
            protocol::capture(&self.checkpoint.identity.provider, &reservation, response)?;
        final_response::assess_candidate(
            &self.checkpoint,
            self.secret,
            &mut turn,
            reservation.repair.as_ref(),
            super::diagnostics::OutputContext {
                inputs: self.inputs,
                supported_tools: self.compiled.tools.supported_ids(),
            },
        )?;
        self.apply(Mutation::Model(turn))
    }
    fn output(&self, candidate: &str) -> AppResult<AgentExecutionOutput> {
        let structured = self
            .checkpoint
            .identity
            .response_contract
            .as_ref()
            .map(|contract| {
                StructuredResponse::validate(
                    contract,
                    candidate,
                    Some(self.secret),
                    &JsonResponseValidator,
                )?
                .map_err(|_| invalid_output())
            })
            .transpose()?;
        let content = structured.as_ref().map_or_else(
            || sanitize_text(candidate, Some(self.secret)),
            |response| response.body().into(),
        );
        let mut output = super::diagnostics::output(
            &self.checkpoint,
            self.inputs,
            self.compiled.tools.supported_ids(),
            content,
            AgentExecutionDisposition::Completed,
        );
        output.structured = structured;
        Ok(output)
    }
}

#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;
