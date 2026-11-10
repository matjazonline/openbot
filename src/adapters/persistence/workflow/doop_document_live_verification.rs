use super::*;

#[derive(sqlx::FromRow)]
struct Delivery {
    body: String,
    level: i32,
    classification_matches: bool,
    correlation_matches: bool,
    destination: Uuid,
    delivery: Uuid,
}

pub(super) struct Verifier<'a> {
    pub(super) environment: &'a setup::Environment,
    pub(super) delivery: &'a delivery_support::DeliveryFixture,
    pub(super) input: &'a Input,
}

impl Verifier<'_> {
    pub(super) async fn document(
        &self,
        document: &setup::CompanyDocument,
        scope: RunReadScope,
        view: &RunView,
        index: usize,
    ) -> Value {
        let environment = self.environment;
        let delivery = self.delivery;
        let input = self.input;
        assert_eq!(view.detail.summary.state, "succeeded");
        let execution = view
            .detail
            .executions
            .iter()
            .find(|execution| execution.step.as_str() == "result")
            .unwrap();
        let step = environment
            .persistence
            .step_view(scope, execution.execution)
            .await
            .unwrap()
            .unwrap();
        let output = step.committed_output.unwrap();
        let effects = environment.client.company(index);
        assert_eq!(effects.canvases.len(), 1);
        assert_eq!(effects.count("create_canvas"), 1);
        assert_eq!(effects.count("create_frame"), 1);
        assert_eq!(effects.count("edit_frame_html"), 1);
        assert_eq!(effects.count("get_frame"), 1);
        assert_eq!(effects.accepted_edits, 1);
        let canvas = &effects.canvases[0].0;
        let frame = &effects.frame.unwrap().0;
        let url = format!("{}/c/{canvas}", input.origin);
        assert_eq!(output["canvas_id"], *canvas);
        assert_eq!(
            output["workspace_id"],
            json!(input.companies[index].workspace)
        );
        assert_eq!(output["canvas_url"], url);
        assert_eq!(output["status"], "draft");
        assert_eq!(output["visual_review"], "not_performed");
        Box::pin(notification(environment, scope, &url, delivery)).await;
        json!({"openbot_company":document.target.company,"connection_id":document.connection.id,
        "bot_user_id":input.companies[index].bot,"workspace_id":input.companies[index].workspace,
        "canvas_id":canvas,"frame_id":frame,"canvas_url":url,"run_id":scope.run,
        "status":"draft","visual_review":"not_performed",
        "expected_old_text":model::before(index),"expected_targeted_edit":model::after(index),
        "post_edit_verification":"fresh bot read required before human enrichment",
        "model_exchanges":5,"current_audience_delivery":true})
    }
}

async fn notification(
    environment: &setup::Environment,
    scope: RunReadScope,
    url: &str,
    delivery: &delivery_support::DeliveryFixture,
) {
    let delivered = sqlx::query_as::<_, Delivery>(
        "SELECT message.clean_text_body AS body, message.access_level::int AS level,
                message.access_level=run.content_level AS classification_matches,
                message.correlation_id=run.correlation_id AS correlation_matches,
                target.destination_id AS destination, target.delivery_id AS delivery
         FROM workflow_message_destinations AS target
         JOIN messages AS message ON message.company_id=target.company_id AND message.id=target.message_id
         JOIN workflow_runs AS run ON run.company_id=target.company_id AND run.id=target.run_id
         WHERE target.company_id=$1 AND target.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid())
        .fetch_all(environment.persistence.pool()).await.unwrap();
    assert_eq!(delivered.len(), 1);
    let message = &delivered[0];
    assert!(message.body.contains(url));
    // Manual synthetic input starts PUBLIC; connection clearance is a ceiling,
    // not an input classification. The ordinary private-source case covers L2.
    assert_eq!(
        message.level,
        i32::from(crate::entities::access_level::AccessLevel::PUBLIC.as_i16())
    );
    assert!(message.classification_matches && message.correlation_matches);
    assert_eq!(message.destination, scope.viewer.user_id());
    let sent = delivery.transport.sent.lock().unwrap();
    let actual: Vec<_> = sent
        .iter()
        .filter(|sent| sent.id.as_uuid() == message.delivery)
        .collect();
    assert_eq!(
        actual.len(),
        1,
        "one real delivery owner call per current document audience"
    );
    assert!(
        serde_json::to_string(&actual[0].email)
            .unwrap()
            .contains(url)
    );
}

pub(super) async fn unknown(environment: &setup::Environment, scope: RunReadScope) -> RunView {
    let view = environment
        .persistence
        .run_view(scope)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.detail.summary.state, "waiting");
    assert_eq!(
        view.detail.summary.waiting_reason.as_deref(),
        Some("reconciliation")
    );
    assert!(
        view.detail
            .actions
            .iter()
            .any(|action| action.effect_kind == "remote" && !action.has_receipt)
    );
    assert!(
        !view
            .detail
            .executions
            .iter()
            .any(|step| step.step.as_str() == "edit" || step.step.as_str() == "result")
    );
    let notifications: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_message_destinations WHERE company_id=$1 AND run_id=$2",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(environment.persistence.pool())
    .await
    .unwrap();
    assert_eq!(notifications, 0);
    view
}
