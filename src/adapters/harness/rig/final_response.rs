//! Shared final-candidate gate for durable and direct executions.
use super::compile::CompiledRun;
use crate::adapters::response_schema::JsonResponseValidator;
use crate::{
    app_error::{AppError, AppResult},
    services::{
        harness::{runs::*, sanitize_text},
        response_contract::{InvalidResponse, StructuredResponse},
    },
};
fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}
pub(super) fn repair_reservation(
    checkpoint: &RunCheckpoint,
) -> AppResult<Option<RepairReservation>> {
    let Some(turn) = checkpoint.turns.last() else {
        return Ok(None);
    };
    let Some(reason) = turn.invalid_response else {
        return Ok(None);
    };
    if checkpoint.repair_count() >= crate::services::response_contract::MAX_REPAIR_CALLS {
        return Err(crate::services::response_contract::invalid_output());
    }
    Ok(Some(RepairReservation {
        candidate: turn.request_id,
        reason,
    }))
}

pub(super) fn response_preamble(
    checkpoint: &RunCheckpoint,
    compiled: &CompiledRun,
    repair: Option<&RepairReservation>,
) -> AppResult<String> {
    let mut preamble = compiled.render_preamble()?;
    if let Some(contract) = &checkpoint.identity.response_contract {
        preamble.push_str("\nFinal answer: return exactly one JSON value conforming to this Draft 2020-12 schema. No prose or code fences. Apply this only to the final answer, after tools finish.\n");
        preamble.push_str(&contract.schema().to_string());
    }
    if let Some(repair) = repair {
        preamble.push_str("\nReplace the invalid final answer with a complete JSON answer. Tools are disabled. Validation reason: ");
        preamble.push_str(repair.reason.code());
    }
    Ok(preamble)
}

pub(super) fn assess_candidate(
    checkpoint: &RunCheckpoint,
    secret: &str,
    turn: &mut SavedModelTurn,
    repair: Option<&RepairReservation>,
    diagnostics: super::diagnostics::OutputContext,
) -> AppResult<()> {
    let Some(contract) = &checkpoint.identity.response_contract else {
        return Ok(());
    };
    if repair.is_none() && !turn.calls.is_empty() {
        return Ok(());
    }
    let result = if repair.is_some() && !turn.calls.is_empty() {
        Err(InvalidResponse::ToolCall)
    } else {
        StructuredResponse::validate(contract, &turn.text, Some(secret), &JsonResponseValidator)?
    };
    turn.invalid_response = match result {
        Err(reason) => Some(reason),
        Ok(response) => oversized_output(checkpoint, turn, response, diagnostics)?,
    };
    // A rejected repair tool call is continuation text, never an executable invocation.
    turn.calls.clear();
    turn.text = if turn.text.len() > crate::services::response_contract::MAX_RESPONSE_BYTES {
        String::new()
    } else {
        sanitize_text(&turn.text, Some(secret))
    };
    if turn.text.is_empty() {
        turn.text = "[invalid final candidate omitted]".into();
    }
    let mut data = candidate_replay(&turn.text)?;
    if data.to_string().len() > MAX_RESULT_BYTES {
        turn.invalid_response = Some(InvalidResponse::OutputLimit);
        turn.text = "[invalid final candidate omitted]".into();
        data = candidate_replay(&turn.text)?;
    }
    turn.continuation = vec![ContinuationBlock {
        schema_version: 1,
        provider: checkpoint.identity.provider.clone(),
        kind: "assistant_v1".into(),
        data,
    }];
    Ok(())
}

fn candidate_replay(text: &str) -> AppResult<serde_json::Value> {
    serde_json::to_value(AssistantReplay {
        message_id: None,
        response_id: None,
        content: vec![SavedAssistantContent::Text {
            text: text.into(),
            parameters: None,
        }],
    })
    .map_err(|_| invalid("Invalid candidate continuation"))
}

fn oversized_output(
    checkpoint: &RunCheckpoint,
    turn: &SavedModelTurn,
    response: StructuredResponse,
    diagnostics: super::diagnostics::OutputContext,
) -> AppResult<Option<InvalidResponse>> {
    let mut projected = checkpoint.clone();
    projected.turns.push(turn.clone());
    let mut output = super::diagnostics::output(
        &projected,
        diagnostics.inputs,
        diagnostics.supported_tools,
        response.body().into(),
        crate::services::harness::AgentExecutionDisposition::Completed,
    );
    output.structured = Some(response);
    Ok((serde_json::to_vec(&output)
        .map_err(|_| invalid("Invalid candidate output"))?
        .len()
        > MAX_RESULT_BYTES)
        .then_some(InvalidResponse::OutputLimit))
}
