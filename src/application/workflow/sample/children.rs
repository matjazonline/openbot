use super::*;
use crate::application::workflow::children::{
    RepeatRule, RoundDecision, child_completion, decide_round, provisional_output,
};
use crate::domain::workflow::{ChildOrdinal, ChoiceName, RouteSelection};
use serde_json::json;

impl Session {
    pub(super) fn child_step(
        &mut self,
        bundle: &Arc<PublishedBundle>,
        frame: &Frame,
        activation: &ActivatedExecution,
        path: &SamplePath,
    ) -> AppResult<PureCompletion> {
        let control = bundle
            .compiled()
            .child_control(&activation.step)
            .ok_or_else(|| refused("Missing frozen child control"))?;
        let child = bundle
            .children()
            .get(&control.child_version_id)
            .ok_or_else(|| refused("Child is absent from the frozen closure"))?
            .clone();
        let mut input = activation
            .inputs
            .get("input")
            .cloned()
            .ok_or_else(|| refused("Missing child input"))?;
        let params = activation
            .inputs
            .get("params")
            .cloned()
            .unwrap_or_else(|| json!({}));
        child
            .compiled()
            .validate_params(&params)
            .map_err(diagnostic)?;
        child
            .compiled()
            .validate_input(&input)
            .map_err(diagnostic)?;
        if control.repeat.is_none() {
            let output = self.run(child, frame.child(path.child("child"), input, params))?;
            return child_completion(bundle, activation, output, RouteSelection::Success);
        }
        let mut round = ChildOrdinal::FIRST;
        loop {
            self.charge(BudgetResource::Repetition)?;
            let result = self.run(
                child.clone(),
                frame.child(
                    path.child(format!("round-{}", round.get())),
                    input,
                    params.clone(),
                ),
            )?;
            let output = provisional_output(result, round);
            match repeat_decision(bundle, frame, activation, &child, round, &output)? {
                RoundDecision::Next(next) => {
                    input = next;
                    round = round
                        .next()
                        .ok_or_else(|| refused("Sample repeat bound exceeded"))?;
                }
                ending => {
                    let choice = if ending == RoundDecision::Done {
                        "done"
                    } else {
                        "exhausted"
                    };
                    return child_completion(
                        bundle,
                        activation,
                        output,
                        RouteSelection::Choice(
                            ChoiceName::parse(choice)
                                .map_err(|_| refused("Invalid repeat choice"))?,
                        ),
                    );
                }
            }
        }
    }
}

fn repeat_decision(
    bundle: &PublishedBundle,
    frame: &Frame,
    activation: &ActivatedExecution,
    child: &PublishedBundle,
    round: ChildOrdinal,
    output: &Value,
) -> AppResult<RoundDecision> {
    let control = bundle
        .compiled()
        .child_control(&activation.step)
        .ok_or_else(|| refused("Missing repeat control"))?;
    let mut outputs = frame.outputs.clone();
    outputs.insert(activation.step.clone(), output.clone());
    let context = Context {
        input: &frame.input,
        params: &frame.params,
        step_outputs: &outputs,
        run: frame.metadata,
    };
    let rule = RepeatRule {
        bindings: control
            .repeat
            .as_ref()
            .ok_or_else(|| refused("Missing repeat bindings"))?,
        max_iterations: control
            .max_iterations
            .ok_or_else(|| refused("Missing repeat bound"))?,
        child,
    };
    decide_round(
        &rule,
        &context,
        bundle.compiled().graph().context_limits(),
        round,
    )
    .map_err(|_| refused("Invalid scripted repeat transition"))
}
