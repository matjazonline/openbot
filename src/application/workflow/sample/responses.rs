//! Preview only: this session has no production admission, notice, or delivery capability.
use super::*;
use crate::domain::workflow::ResponseDisposition;

impl Session {
    pub(super) fn completed_response(
        &mut self,
        bundle: &PublishedBundle,
        completion: &PureCompletion,
    ) -> AppResult<()> {
        let response = if completion.response_disposition == ResponseDisposition::Suppressed {
            SampleResponse::Suppressed
        } else if let Some(contract) = &self.response_contract {
            SampleResponse::Automatic {
                content: contract
                    .project(
                        bundle
                            .compiled()
                            .graph()
                            .definition()
                            .output_schema
                            .as_ref()
                            .unwrap_or(&Value::Bool(true)),
                        &completion.output,
                    )?
                    .into(),
                approval_required: self.response_approval_policy.is_some(),
            }
        } else {
            SampleResponse::AutomaticUnmapped
        };
        self.set_response(response)
    }

    pub(super) fn waiting_response(
        &mut self,
        kind: &str,
        activation: &ActivatedExecution,
    ) -> AppResult<()> {
        let (stage, preview) = match kind {
            "decision.human" => (SampleWaitStage::FormalReview, "proposal"),
            "message.ask" if activation.inputs.get("approval").is_some() => {
                (SampleWaitStage::QuestionApproval, "content")
            }
            "message.ask" => (SampleWaitStage::HumanReply, "content"),
            _ => return Ok(()),
        };
        self.set_response(SampleResponse::Waiting {
            stage,
            preview: activation
                .inputs
                .get(preview)
                .cloned()
                .unwrap_or(Value::Null),
        })
    }

    fn set_response(&mut self, response: SampleResponse) -> AppResult<()> {
        self.charge_report(&response)?;
        self.report.response = Some(response);
        Ok(())
    }
}
