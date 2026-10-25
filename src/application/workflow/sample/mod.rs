//! Isolated execution over the production engine's pure activation/completion contracts (P09-D1).
//! This module has no production service, resource binding, network client or persistence handle.
//! Children and direct tools stay inside this evaluator; reports cannot be admitted or resumed.
use super::{
    activation::{ActivatedExecution, prepare_activation},
    batch::{PureCompletion, execute_pure},
    publication::PublishedBundle,
};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{
    BudgetResource, Context, ExecutionId, RootBudgetLimits, RunId, RunMetadata, StepId,
    TransitionTarget, WorkflowId,
};
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

mod children;
mod dispatch;
mod tools;
mod types;
pub use types::*;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct ScriptKey {
    workflow: WorkflowId,
    step: StepId,
}
struct Session {
    scripts: BTreeMap<ScriptKey, VecDeque<ScriptedResponse>>,
    budget: RootBudgetLimits,
    report: SampleReport,
    report_bytes: usize,
}
struct Frame {
    input: Value,
    params: Value,
    outputs: BTreeMap<StepId, Value>,
    metadata: RunMetadata,
    path: SamplePath,
    depth: usize,
}
impl Frame {
    fn context(&self) -> Context<'_> {
        Context {
            input: &self.input,
            params: &self.params,
            step_outputs: &self.outputs,
            run: self.metadata,
        }
    }
    fn child(&self, path: SamplePath, input: Value, params: Value) -> Self {
        Self {
            input,
            params,
            outputs: BTreeMap::new(),
            metadata: RunMetadata {
                run_id: RunId::new(Uuid::new_v4()),
                parent_run_id: Some(self.metadata.run_id),
                deadline: self.metadata.deadline,
            },
            path,
            depth: self.depth + 1,
        }
    }
}
fn refused(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}
fn diagnostic(error: super::compiler::Diagnostic) -> AppError {
    refused(error.message)
}

pub fn execute(bundle: Arc<PublishedBundle>, request: SampleRequest) -> AppResult<SampleReport> {
    let bytes = serde_json::to_vec(&request).map_err(|_| refused("Invalid sample request"))?;
    if bytes.len() > MAX_SAMPLE_BYTES {
        return Err(refused("Sample input and scripts exceed 256 KiB"));
    }
    if request.scripts.len() > MAX_SAMPLE_STEPS as usize {
        return Err(refused("Sample exceeds 128 script keys"));
    }
    let scripts = script_queues(request.scripts)?;
    let mut session = Session {
        scripts,
        budget: bundle
            .compiled()
            .graph()
            .definition()
            .limits
            .root_budget
            .clone(),
        report_bytes: 1024,
        report: SampleReport {
            content_hash: bundle.content_hash().clone(),
            status: SampleStatus::Stopped,
            output: None,
            steps: vec![],
            effects: vec![],
            usage: SampleUsage::default(),
            unused_responses: 0,
            diagnostic: None,
        },
    };
    let frame = Frame {
        input: request.input,
        params: request.params,
        outputs: BTreeMap::new(),
        metadata: RunMetadata {
            run_id: RunId::new(Uuid::new_v4()),
            parent_run_id: None,
            deadline: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
        },
        path: SamplePath("sample".into()),
        depth: 0,
    };
    match session.run(bundle, frame).and_then(|output| {
        session.charge_report(&output)?;
        Ok(output)
    }) {
        Ok(output) => {
            session.report.status = SampleStatus::Completed;
            session.report.output = Some(output);
        }
        Err(error) => session.report.diagnostic = Some(error.to_string()),
    }
    session.report.unused_responses = session.scripts.values().map(VecDeque::len).sum();
    Ok(session.report)
}

fn script_queues(
    entries: Vec<StepScript>,
) -> AppResult<BTreeMap<ScriptKey, VecDeque<ScriptedResponse>>> {
    let mut scripts = BTreeMap::new();
    let mut count = 0;
    for script in entries {
        count += script.responses.len();
        if count > MAX_SAMPLE_STEPS as usize
            || scripts
                .insert(
                    ScriptKey {
                        workflow: script.workflow_id,
                        step: script.step,
                    },
                    script.responses.into(),
                )
                .is_some()
        {
            return Err(refused(
                "Use unique workflow/step scripts and at most 128 responses",
            ));
        }
    }
    Ok(scripts)
}

impl Session {
    fn run(&mut self, bundle: Arc<PublishedBundle>, mut frame: Frame) -> AppResult<Value> {
        if frame.depth > MAX_DEPTH {
            return Err(refused("Sample child depth exceeds 8"));
        }
        let compiled = bundle.compiled();
        compiled.validate_input(&frame.input).map_err(diagnostic)?;
        compiled
            .validate_params(&frame.params)
            .map_err(diagnostic)?;
        let definition = compiled.graph().definition();
        let mut step = definition.entry.clone();
        for ordinal in 1..=definition.limits.max_steps.min(MAX_SAMPLE_STEPS) {
            self.charge(BudgetResource::Activation)?;
            let prepared = prepare_activation(&bundle, &step, &frame.context())?;
            let activation = ActivatedExecution {
                execution: ExecutionId::new(Uuid::new_v4()),
                step: step.clone(),
                ordinal: u64::from(ordinal),
                inputs: prepared.inputs,
                choice: prepared.choice,
            };
            let path = frame.path.child(format!("{}:{ordinal}", step.as_str()));
            let completion = self
                .step(&bundle, &frame, &activation, &path)
                .map_err(|error| {
                    refused(format!(
                        "{} ({}): {error}",
                        path.as_str(),
                        definition.steps[&step].step_type
                    ))
                })?;
            let trace = SampleStep {
                path,
                workflow_id: definition.workflow_id,
                step: step.clone(),
                kind: definition.steps[&step].step_type.clone(),
                inputs: activation.inputs,
                output: completion.output.clone(),
            };
            self.charge_report(&trace)?;
            self.report.steps.push(trace);
            frame
                .outputs
                .insert(step.clone(), completion.output.clone());
            match completion.target {
                TransitionTarget::End => return Ok(completion.output),
                TransitionTarget::Step(next) => step = next,
            }
        }
        Err(refused("Sample reached the workflow step limit"))
    }

    fn response(&mut self, bundle: &PublishedBundle, step: &StepId) -> AppResult<ScriptedResponse> {
        let workflow = bundle.compiled().graph().definition().workflow_id;
        self.scripts
            .get_mut(&ScriptKey {
                workflow,
                step: step.clone(),
            })
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| {
                refused(format!(
                    "Missing scripted response for workflow {workflow}, step {step}"
                ))
            })
    }
    fn charge(&mut self, resource: BudgetResource) -> AppResult<()> {
        let used = match resource {
            BudgetResource::Activation => &mut self.report.usage.activations,
            BudgetResource::ModelCall => &mut self.report.usage.model_calls,
            BudgetResource::Repetition => &mut self.report.usage.repetitions,
        };
        let limit = self.budget.limit(resource).min(MAX_SAMPLE_STEPS);
        if *used >= limit {
            return Err(refused(format!(
                "Sample root {resource:?} budget exhausted"
            )));
        }
        *used += 1;
        Ok(())
    }
    fn charge_report(&mut self, value: &impl serde::Serialize) -> AppResult<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| refused("Invalid sample report"))?
            .len();
        if bytes > MAX_REPORT_BYTES.saturating_sub(self.report_bytes) {
            return Err(refused("Sample report exceeds 1 MiB"));
        }
        self.report_bytes += bytes;
        Ok(())
    }
    fn effect(&mut self, effect: SampleEffect) -> AppResult<()> {
        self.charge_report(&effect)?;
        self.report.effects.push(effect);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
