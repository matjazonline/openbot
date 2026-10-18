use super::*;
use crate::adapters::protocols::email::EmailRenderer;
use crate::application::transport::{DeliveryComposer, TransportRenderers};
use crate::application::workflow::{activation::ActivationRequest, decisions::*};

pub(super) fn composer(p: &PostgresPersistence) -> DeliveryComposer {
    let renderers = TransportRenderers::new()
        .register(Arc::new(EmailRenderer::new("agents.test")))
        .unwrap();
    DeliveryComposer::new(Arc::new(renderers), Arc::new(p.clone()))
}

pub(super) fn notices(p: &PostgresPersistence) -> DecisionNotices {
    DecisionNotices::new(composer(p), "https://agents.test", "agents.test")
}

pub(super) async fn command(
    p: &PostgresPersistence,
    scope: ActivationRequest,
    actor: WorkflowActor,
    choice: &str,
) -> SubmitDecision {
    let id = sqlx::query_scalar(
        "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let view = p
        .read_decision(scope.company, actor, DecisionId(id))
        .await
        .unwrap();
    SubmitDecision {
        company: scope.company,
        actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse("approval-command").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: choice.into(),
        feedback: String::new(),
        data: serde_json::json!({}),
    }
}
