use super::*;
use crate::application::workflow::agent::{AgentToolDeclaration, PreparedTool};
use crate::entities::response_contract::ResponseContract;
use crate::services::{
    harness::runs::untrusted_context_prompt, response_contract::final_answer_preamble,
};
use serde::Deserialize;

pub(super) fn entry(row: &Metadata, viewer: AccessLevel) -> AppResult<HistoryEntry> {
    let state = HistoryState::try_from(row.state.clone())?;
    let level = if state == HistoryState::Completed {
        row.access_level
    } else {
        row.access_level.max(row.content_level)
    };
    let visible =
        AccessLevel::try_from(level).map_err(|error| AppError::Internal(error.into()))? <= viewer;
    let source_visible = AccessLevel::try_from(row.content_level)
        .map_err(|error| AppError::Internal(error.into()))?
        <= viewer;
    Ok(HistoryEntry {
        order: row.position,
        state,
        timing: HistoryTiming {
            created_at: row.created_at,
            activated_at: row.activated_at,
            finished_at: row.finished_at,
        },
        payload: if !visible {
            HistoryPayloadState::Restricted
        } else if row.payload_retained {
            HistoryPayloadState::Retained
        } else {
            HistoryPayloadState::Expired
        },
        exchange: visible.then(|| AgentExchangeId::new(row.id)),
        source: (visible && source_visible).then(|| source(row)),
        activity: visible.then(|| activity(row, state)),
        provenance: visible.then(|| HistoryProvenance {
            agent: crate::entities::agent_capability::AgentId::new(row.agent_id),
            revision: crate::entities::agent_capability::AgentRevisionId::new(row.revision_id),
            manifest: crate::entities::agent_capability::DependencyManifestId::new(row.manifest_id),
        }),
    })
}

fn activity(row: &Metadata, state: HistoryState) -> HistoryActivity {
    match state {
        HistoryState::Completed => HistoryActivity::Produced,
        HistoryState::Failed => HistoryActivity::FailedBeforeOutput,
        HistoryState::Cancelled => HistoryActivity::CancelledBeforeOutput,
        HistoryState::Queued if row.waiting_reason.as_deref() == Some("agent_turn") => {
            HistoryActivity::WaitingForTurn
        }
        HistoryState::Queued => HistoryActivity::Queued,
        HistoryState::Active if row.human_wait => HistoryActivity::WaitingForHuman,
        HistoryState::Active => HistoryActivity::Active,
    }
}

pub(super) fn source(row: &Metadata) -> HistorySource {
    HistorySource {
        run: RunId::new(row.run_id),
        execution: row.execution_id,
        checkpoint: row.checkpoint_id,
    }
}

#[derive(Deserialize)]
struct StoredInspection {
    version: u8,
    preamble: String,
    context: Value,
    history: FrozenAgentHistory,
    output_schema: Value,
    tools: Vec<PreparedTool>,
}

pub(super) fn frozen(value: Value) -> AppResult<FrozenHistoryInspection> {
    // Typed allowlist: stored dispatch targets are parsed but only shared model declarations leave this boundary.
    let stored: StoredInspection = serde_json::from_value(value).map_err(|_| invalid())?;
    if stored.version != crate::application::workflow::agent::PREPARED_AGENT_VERSION
        || stored.history.selected.len() > 16
    {
        return Err(invalid());
    }
    stored.history.validate().map_err(|_| invalid())?;
    let contract = ResponseContract::parse(
        &serde_json::json!({"version":1,"format":"json_schema","schema":stored.output_schema})
            .to_string(),
    )
    .map_err(|_| invalid())?;
    let tools: Vec<AgentToolDeclaration> =
        stored.tools.iter().map(PreparedTool::declaration).collect();
    Ok(FrozenHistoryInspection {
        history: stored.history,
        prompt: final_answer_preamble(stored.preamble, Some(&contract)),
        current_prompt: untrusted_context_prompt(&stored.context)?,
        output_schema: stored.output_schema,
        tools,
    })
}

pub(super) fn detail(
    row: &Metadata,
    viewer: AccessLevel,
    payload: Option<PayloadRow>,
) -> AppResult<HistoryDetail> {
    let entry = entry(row, viewer)?;
    if entry.payload == HistoryPayloadState::Restricted {
        return Ok(HistoryDetail {
            entry,
            payload: None,
            status: None,
        });
    }
    let payload = match (entry.payload, payload) {
        (HistoryPayloadState::Retained, Some(payload)) => {
            if payload.preparation_digest != row.preparation_digest
                || payload.output_digest != row.output_digest
                || payload.state != row.state
            {
                return Err(changed());
            }
            Some(HistoryPayload {
                input: payload.input,
                output: payload.output,
                frozen: payload.frozen_preparation.map(frozen).transpose()?,
                selected_sources: vec![],
            })
        }
        (HistoryPayloadState::Retained, None) => return Err(changed()),
        _ => None,
    };
    Ok(HistoryDetail {
        entry,
        payload,
        status: Some(HistoryOutputStatus {
            workflow: row.workflow_state.clone(),
            waiting_reason: row.waiting_reason.clone(),
            channel_response: row.channel_response.clone(),
            delivery: row.delivery.clone(),
        }),
    })
}

fn invalid() -> AppError {
    AppError::Internal("Invalid stored agent history preparation".into())
}
pub(super) fn changed() -> AppError {
    AppError::Conflict("Agent history changed; reload the inspection".into())
}
