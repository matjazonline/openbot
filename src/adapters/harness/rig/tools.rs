//! One run owns declarations, budgets and the stop latch. Since the legacy tool catalogue
//! was removed (06C), the only tool is `read_resource` for attached skills; workflow agents
//! use ActionService-dispatched tools instead. Model calls enter `invoke`.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use rig::tool::{DynamicTool, ToolExecutionError, ToolOutput};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{
    app_error::{AppError, AppResult},
    entities::value_objects::ToolId,
    services::harness::ToolInvocation,
};

pub const MAX_TOOL_ARGUMENT_BYTES: usize = 65_536;
pub const MAX_TOOL_INVOCATIONS: usize = 64;
pub const MAX_TOOL_OUTPUT_CHARS: usize = 16_384;
pub const MAX_TOOL_RESULT_CHARS: usize = 65_536;
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// A correlator is not a provider wire ID or an approval key. Never write it into provider history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCorrelationId(String);

impl ToolCorrelationId {
    pub fn parse(value: &str) -> AppResult<Self> {
        if value.is_empty() || value.len() > 256 || !value.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid("Invalid tool correlation ID"));
        }
        Ok(Self(value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStopReason {
    Suspended,
    Budget,
    Timeout,
    HostFailure,
    Protocol,
}

/// An attached skill's resource read: the only tool a legacy run still declares.
struct Entry {
    description: String,
    schema: Value,
    catalog: Arc<super::skills::SkillCatalog>,
}

#[derive(Default)]
struct ExecutionState {
    attempts: usize,
    stop: Option<ToolStopReason>,
}

pub struct ToolBridge {
    trace: Option<Arc<dyn crate::services::harness::HarnessTrace>>,
    entries: BTreeMap<ToolId, Entry>,
    validators: BTreeMap<ToolId, jsonschema::Validator>,
    state: Mutex<ExecutionState>,
    deadline: tokio::time::Instant,
    budget: Option<Arc<super::budget::RunBudget>>,
}

impl Default for ToolBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolBridge {
    pub fn new() -> Self {
        Self {
            trace: None,
            entries: BTreeMap::new(),
            validators: BTreeMap::new(),
            state: Mutex::new(ExecutionState::default()),
            deadline: tokio::time::Instant::now() + Duration::from_secs(300),
            budget: None,
        }
    }

    pub async fn stop_reason(&self) -> Option<ToolStopReason> {
        self.state.lock().await.stop
    }

    pub(super) fn with_budget(mut self, budget: Arc<super::budget::RunBudget>) -> Self {
        self.budget = Some(budget);
        self
    }

    pub(super) fn with_resources(
        mut self,
        catalog: Arc<super::skills::SkillCatalog>,
    ) -> AppResult<Self> {
        if catalog.is_empty() {
            return Ok(self);
        }
        let id: ToolId = "read_resource".into();
        if self.entries.contains_key(&id) {
            return Err(invalid("Resource tool identity collision"));
        }
        let schema = serde_json::json!({"type":"object","properties":{
            "skill_uri":{"type":"string","minLength":1,"maxLength":160}},
            "required":["skill_uri"],"additionalProperties":false});
        self.validators
            .insert(id.clone(), super::schema::compile(&schema)?);
        self.entries.insert(id, Entry {
            description: "Load an attached skill's full instructions using its exact catalog URI.".into(),
            schema, catalog,
        });
        Ok(self)
    }

    /// Bind declared schemas and limits to the durable run.
    pub(super) fn catalogue_fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let entries: Vec<_> = self
            .entries
            .iter()
            .map(|(id, entry)| {
                serde_json::json!({"id":id,"description":entry.description,"schema":entry.schema,
                "approval":false,"limits":[MAX_TOOL_OUTPUT_CHARS, MAX_TOOL_RESULT_CHARS]})
            })
            .collect();
        format!(
            "{:x}",
            Sha256::digest(serde_json::json!(entries).to_string().as_bytes())
        )
    }

    pub(super) fn schema_bytes(&self) -> usize {
        self.entries
            .iter()
            .map(|(id, entry)| {
                id.len() + entry.description.len() + entry.schema.to_string().len() + 64
            })
            .sum()
    }

    /// The enclosing application may shorten the server ceiling, never extend it.
    pub(super) fn restrict_deadline(&mut self, deadline: tokio::time::Instant) {
        self.deadline = self.deadline.min(deadline);
    }

    pub fn with_deadline(mut self, deadline: tokio::time::Instant) -> Self {
        self.deadline = self.deadline.min(deadline);
        self
    }

    /// Serializes effects even if a caller mistakenly requests concurrent dispatch. The latch is
    /// held across the actual future; cancellation drops that future, never a detached worker.
    pub async fn invoke(
        &self,
        id: &ToolId,
        correlation: &ToolCorrelationId,
        args: Value,
    ) -> AppResult<ToolInvocation> {
        let label = self
            .entries
            .get_key_value(id)
            .map_or_else(|| ToolId::from("unknown"), |(id, _)| id.clone());
        let mut trace =
            super::trace::ToolTrace::new(self.trace.clone(), label, correlation.as_str());
        // Box the bridge seam: tracing should not grow the durable worker's poll frame.
        let result = Box::pin(self.invoke_inner(id, args, &mut trace)).await;
        trace.finish(&result).await;
        result
    }

    async fn invoke_inner(
        &self,
        id: &ToolId,
        args: Value,
        trace: &mut super::trace::ToolTrace,
    ) -> AppResult<ToolInvocation> {
        let mut state = self.state.lock().await;
        self.reserve_attempt(&mut state, &args)?;
        let entry = match self.validate_call(id, &args) {
            Ok(entry) => entry,
            Err(reason) => return Ok(ToolInvocation::failure(reason)),
        };
        // If the caller drops this future mid-effect, reusing this bridge must fail closed.
        state.stop = Some(ToolStopReason::HostFailure);
        let deadline = self
            .deadline
            .min(tokio::time::Instant::now() + TOOL_TIMEOUT);
        if deadline <= tokio::time::Instant::now() {
            state.stop = Some(ToolStopReason::Timeout);
            return Err(AppError::Timeout("Tool deadline exceeded".into()));
        }
        let result = tokio::time::timeout_at(deadline, self.execute(entry, args, trace)).await;
        let mut invocation = match result {
            Err(_) => {
                state.stop = Some(ToolStopReason::Timeout);
                return Err(AppError::Timeout("Tool deadline exceeded".into()));
            }
            Ok(Err(error)) => {
                state.stop = Some(ToolStopReason::HostFailure);
                return Err(error);
            }
            Ok(Ok(invocation)) => invocation,
        };
        state.stop = invocation
            .suspends_run()
            .then_some(ToolStopReason::Suspended);
        let rendered = invocation.render();
        if rendered.chars().count() > MAX_TOOL_OUTPUT_CHARS {
            trace.output_truncated = true;
            invocation.output = Value::String(truncate(&rendered, MAX_TOOL_OUTPUT_CHARS));
        }
        if self
            .budget
            .as_ref()
            .is_some_and(|budget| budget.charge(invocation.render().len()).is_err())
        {
            state.stop = Some(ToolStopReason::Budget);
            return Err(invalid("Rig run token budget exhausted"));
        }
        Ok(invocation)
    }

    fn reserve_attempt(&self, state: &mut ExecutionState, args: &Value) -> AppResult<()> {
        if state.stop.is_some() {
            return Err(invalid("Tool run has stopped"));
        }
        if state.attempts >= MAX_TOOL_INVOCATIONS {
            state.stop = Some(ToolStopReason::Budget);
            return Err(invalid("Tool invocation budget exhausted"));
        }
        state.attempts += 1;
        if self
            .budget
            .as_ref()
            .is_some_and(|budget| budget.charge(args.to_string().len()).is_err())
        {
            state.stop = Some(ToolStopReason::Budget);
            return Err(invalid("Rig run token budget exhausted"));
        }
        Ok(())
    }

    fn validate_call(&self, id: &ToolId, args: &Value) -> Result<&Entry, &'static str> {
        let entry = self.entries.get(id).ok_or("Tool is not granted")?;
        if !args.is_object() || args.to_string().len() > MAX_TOOL_ARGUMENT_BYTES {
            return Err("Tool arguments must be an object within 64 KiB");
        }
        if !self
            .validators
            .get(id)
            .is_some_and(|validator| validator.is_valid(args))
        {
            return Err("Tool arguments do not match its declared schema");
        }
        Ok(entry)
    }

    /// Re-read attached skill resources so a changed skill authorization fails the replay.
    /// Every other committed result is replayed from the checkpoint.
    pub(super) async fn restore_local_state(
        &self,
        run: &crate::services::harness::runs::RunCheckpoint,
    ) -> AppResult<()> {
        for saved in run.invocations.iter().filter(|saved| {
            saved.call.tool_id.as_str() == "read_resource" && saved.result.is_some()
        }) {
            let id = &saved.call.tool_id;
            let entry = self
                .validate_call(id, &saved.call.arguments)
                .map_err(invalid)?;
            let correlation = ToolCorrelationId::parse(&saved.call.invocation_id.0.to_string())?;
            // Rebuilding local state is replay, not a second logical tool execution.
            let mut trace = super::trace::ToolTrace::new(None, id.clone(), correlation.as_str());
            let restored = self
                .execute(entry, saved.call.arguments.clone(), &mut trace)
                .await?;
            if Some(&restored.output) != saved.result.as_ref() {
                return Err(invalid("Saved tool state or skill authorization changed"));
            }
        }
        Ok(())
    }

    pub(super) async fn stop(&self, reason: ToolStopReason) {
        self.state.lock().await.stop.get_or_insert(reason);
    }

    async fn execute(
        &self,
        entry: &Entry,
        args: Value,
        trace: &mut super::trace::ToolTrace,
    ) -> AppResult<ToolInvocation> {
        trace
            .started(args.as_object().map_or(0, |args| args.len()))
            .await;
        let uri = args
            .get("skill_uri")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Missing skill URI"))?;
        Ok(ToolInvocation::success(Value::String(
            entry.catalog.read(uri).await?,
        )))
    }

    pub(super) fn set_trace(
        &mut self,
        trace: Option<Arc<dyn crate::services::harness::HarnessTrace>>,
    ) {
        self.trace = trace;
    }

    pub(super) fn supported_ids(&self) -> Vec<ToolId> {
        self.entries.keys().cloned().collect()
    }

    pub(super) fn definitions(&self) -> Vec<rig::completion::ToolDefinition> {
        self.entries
            .iter()
            .map(|(id, entry)| rig::completion::ToolDefinition {
                name: id.to_string(),
                description: entry.description.clone(),
                parameters: entry.schema.clone(),
            })
            .collect()
    }

    pub(super) fn declarations(
        self: &Arc<Self>,
        handoff: &Arc<super::tool_hook::ToolHandoff>,
    ) -> Vec<DynamicTool> {
        self.entries
            .iter()
            .map(|(id, entry)| {
                let bridge = self.clone();
                let handoff = handoff.clone();
                let id = id.clone();
                DynamicTool::new(
                    id.to_string(),
                    &entry.description,
                    entry.schema.clone(),
                    move |_, args| {
                        let bridge = bridge.clone();
                        let handoff = handoff.clone();
                        let id = id.clone();
                        Box::pin(async move {
                            let correlation = handoff.take(&id, &args)?;
                            match bridge.invoke(&id, &correlation, args).await {
                                Ok(result) if result.success => {
                                    Ok(ToolOutput::text(result.render()))
                                }
                                Ok(result) => {
                                    Err(ToolExecutionError::refused("Tool rejected arguments")
                                        .with_model_output(ToolOutput::text(result.render())))
                                }
                                Err(_) => {
                                    Err(ToolExecutionError::refused("Tool execution stopped"))
                                }
                            }
                        })
                    },
                )
            })
            .collect()
    }
}

fn truncate(text: &str, limit: usize) -> String {
    const MARKER: &str = " [truncated]";
    if limit < MARKER.len() {
        return MARKER.chars().take(limit).collect();
    }
    let mut value: String = text.chars().take(limit - MARKER.len()).collect();
    value.push_str(MARKER);
    value
}

fn invalid(message: &str) -> AppError {
    AppError::BadRequest(message.into())
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
