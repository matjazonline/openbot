//! Normal document admission and revision/artifact-checked approvals shared by both fixtures.
use super::*;

pub(super) struct DocumentRun<'a> {
    pub(super) persistence: &'a PostgresPersistence,
    pub(super) engine: &'a WorkflowEngine,
    pub(super) target: BindingTarget,
    pub(super) channel: crate::application::workflow::RelatedChannelId,
}

impl DocumentRun<'_> {
    pub(super) fn request(&self, input: Value) -> AdmitWorkflowRequest {
        AdmitWorkflowRequest {
            history_session: None,
            company_id: self.target.company,
            actor: self.target.actor,
            association: RelatedAssociation::Channel(self.channel),
            trigger: TriggerRef::new(
                self.target.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: self.target.binding,
            idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
            input,
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        }
    }

    pub(super) async fn admit(&self, input: Value) -> AppResult<RunReadScope> {
        let admitted = self
            .engine
            .services()
            .service()
            .admit(self.request(input))
            .await?;
        let AdmissionResult::Created(run) = admitted else {
            panic!("new run")
        };
        self.engine.services().wake_workers();
        Ok(RunReadScope {
            company: self.target.company,
            run,
            viewer: self.target.actor,
        })
    }

    pub(super) async fn approvals(&self, scope: RunReadScope) -> Vec<DecisionId> {
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND submitted_at IS NULL ORDER BY created_at LIMIT 8")
            .bind(scope.company.as_uuid()).bind(scope.run.as_uuid())
            .fetch_all(self.persistence.pool()).await.unwrap().into_iter().map(DecisionId).collect()
    }

    pub(super) async fn approve(&self, scope: RunReadScope, decision: DecisionId) {
        let p = &self.persistence;
        let view = p
            .read_decision(scope.company, scope.viewer, decision)
            .await
            .unwrap();
        let submitted = p
            .submit_decision(SubmitDecision {
                company: scope.company,
                actor: scope.viewer,
                decision,
                command_key: DecisionCommandKey::parse(Uuid::new_v4().to_string()).unwrap(),
                expected_revision: view.revision,
                expected_artifact: view.artifact_digest,
                choice: "approve".into(),
                feedback: String::new(),
                data: json!({}),
            })
            .await
            .unwrap();
        if submitted.successor.is_some() {
            self.engine.services().wake_workers();
        }
    }
}
