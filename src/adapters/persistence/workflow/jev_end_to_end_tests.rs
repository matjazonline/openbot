use super::jev_tests::{admit, bound_source, create, fixture};
use super::*;
use crate::adapters::{http::pages, protocols::email::EmailRenderer, workflow_jev::tests::server};
use crate::application::{
    transport::{DeliveryComposer, TransportRenderers},
    workflow::{
        decisions::*, lease::*, polling::*, reads::WorkflowReads, run_views::RunViews, worker::*,
    },
};
use crate::infra::workflow::WorkflowEngine;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn notices(f: &Fixture) -> DecisionNotices {
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(EmailRenderer::new("agents.test")))
            .unwrap(),
    );
    DecisionNotices::new(
        DeliveryComposer::new(renderers, Arc::new(f.persistence.clone())),
        "https://agents.test",
        "agents.test",
    )
}

async fn drive(f: &Fixture, engine: &WorkflowEngine, run: RunId) {
    let notices = notices(f);
    let worker = WorkflowWorker {
        port: &f.persistence,
        handler: engine.handler(),
        notices: Some(&notices),
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(Duration::from_secs(30)).unwrap(),
        poll: PollPolicy::new(Duration::from_secs(1), 16).unwrap(),
        active_jobs: None,
    };
    for _ in 0..8 {
        let candidates = f.persistence.poll_work(None, 16).await.unwrap();
        let Some(candidate) = candidates
            .candidates
            .into_iter()
            .find(|candidate| candidate.scope.run == run)
        else {
            return;
        };
        worker
            .process(candidate, &CancellationToken::new())
            .await
            .unwrap();
    }
    panic!("scenario did not quiesce");
}

async fn assert_history(f: &Fixture, run: RunId, step_name: &str, output: &Value) {
    let detail = f
        .persistence
        .run_detail(f.target.company, run)
        .await
        .unwrap()
        .unwrap();
    let execution = detail
        .executions
        .iter()
        .find(|step| step.step == step_name)
        .unwrap()
        .execution;
    let view = f
        .persistence
        .step_view(f.target.company, run, execution)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.committed_output.as_ref().unwrap(), output);
    assert_eq!(view.model_calls.len(), 1);
    assert_eq!(view.model_calls[0].provider, "typesafe");
    let html = pages::run_step_body(&pages::StepBodyView {
        company_id: f.target.company.as_uuid(),
        step: &view,
        agent: None,
    });
    assert!(html.contains("typesafe") && html.contains("jev-1.14.0"));
    assert!(
        !html.contains("fixture-jev-secret")
            && !html.contains("data_base64")
            && !html.contains("enc:v2")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_usage_calls WHERE run_id=$1 AND agent_id IS NULL AND provider='typesafe' AND settled_at IS NOT NULL")
        .bind(run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn jev_end_to_end_mixed_thresholds_automatic_and_uncertain_human() {
    for human in [false, true] {
        let f = fixture().await;
        let c = create(&f).await;
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../samples/jev-triage.json")).unwrap();
        let mut source = fixture["source"].clone();
        source["steps"]["review"]["with"]["reviewers"]["literal"][0]["id"] =
            json!(f.target.actor.user_id());
        let target = bound_source(&f, &c, source).await;
        let AdmissionResult::Created(run) = admit(&f, target, "mixed-scenario").await.unwrap()
        else {
            panic!()
        };
        let sample = if human { "uncertain_sample" } else { "sample" };
        let output = fixture[sample]["scripts"][0]["responses"][0]["output"].clone();
        let (http, server) = server(
            200,
            "",
            serde_json::to_vec(&output).unwrap(),
            Duration::ZERO,
        )
        .await;
        let engine =
            WorkflowEngine::for_tests(f.persistence.clone()).with_jev_test_provider(Arc::new(http));
        drive(&f, &engine, run).await;
        server.await.unwrap();
        assert_history(&f, run, "evaluate", &output).await;
        let detail = f
            .persistence
            .run_detail(target.company, run)
            .await
            .unwrap()
            .unwrap();
        if human {
            assert_eq!(detail.summary.state, "waiting");
            let decision = DecisionId(detail.waits.iter().find_map(|wait| wait.decision).unwrap());
            let view = f
                .persistence
                .read_decision(target.company, target.actor, decision)
                .await
                .unwrap();
            f.persistence
                .submit_decision(SubmitDecision {
                    company: target.company,
                    actor: target.actor,
                    decision,
                    command_key: DecisionCommandKey::parse("accept").unwrap(),
                    expected_revision: view.revision,
                    expected_artifact: view.artifact_digest,
                    choice: "accept".into(),
                    feedback: String::new(),
                    data: json!({"path":"human"}),
                })
                .await
                .unwrap();
            drive(&f, &engine, run).await;
        } else {
            assert!(
                detail
                    .executions
                    .iter()
                    .any(|step| step.step == "automatic")
            );
            assert!(!detail.executions.iter().any(|step| step.step == "review"));
        }
        assert_eq!(
            f.persistence
                .run_detail(target.company, run)
                .await
                .unwrap()
                .unwrap()
                .summary
                .state,
            "succeeded"
        );
    }
}

#[tokio::test]
async fn jev_end_to_end_box_worker_history_and_invalid_output_rejection() {
    use crate::adapters::{
        storage::{BucketKind, FileStorage, test_support::FakeStorage},
        workflow_jev::JevAssetReader,
    };
    use crate::application::workflow::jev::assets::tests as images;
    for invalid in [false, true] {
        let f = fixture().await;
        let c = create(&f).await;
        let bytes = images::png(280, 280);
        let attachment = super::jev_assets_tests::attachment(&f, &bytes).await;
        let storage = Arc::new(FakeStorage::new());
        storage
            .store_object(BucketKind::Private, &attachment.key, "image/png", bytes)
            .await
            .unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../samples/jev-image.json")).unwrap();
        let mut source = fixture["source"].clone();
        source["steps"]["start"]["with"]["assets"]["literal"]["photo"] = attachment.reference;
        let target = bound_source(&f, &c, source).await;
        let AdmissionResult::Created(run) = admit(&f, target, "box-scenario").await.unwrap() else {
            panic!()
        };
        let mut output = images::output();
        if invalid {
            output["answers"]["mug"]["box"]["right"] = json!(0.01);
        }
        let (http, server) = server(
            200,
            "",
            serde_json::to_vec(&output).unwrap(),
            Duration::ZERO,
        )
        .await;
        let engine = WorkflowEngine::for_tests(f.persistence.clone())
            .with_jev_test_provider(Arc::new(http))
            .with_jev_assets(Arc::new(JevAssetReader::new(
                f.persistence.clone(),
                Some(storage),
            )));
        drive(&f, &engine, run).await;
        let wire = server.await.unwrap();
        assert!(wire.contains("data_base64"));
        let detail = f
            .persistence
            .run_detail(target.company, run)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.summary.state,
            if invalid { "failed" } else { "succeeded" }
        );
        if !invalid {
            assert_history(&f, run, "start", &output).await;
        }
        let ledger: String = sqlx::query_scalar("SELECT to_jsonb(invocation)::text FROM workflow_jev_invocations AS invocation WHERE run_id=$1").bind(run.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
        assert!(!ledger.contains("data_base64") && !ledger.contains("fixture-jev-secret"));
    }
}
