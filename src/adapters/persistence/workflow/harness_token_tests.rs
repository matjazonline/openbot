use super::*;
use crate::adapters::persistence::workflow::tests::{
    approval_test_support::{command, notices},
    message_tests::support::member,
};
use crate::application::workflow::decisions::WorkflowDecisions;

#[path = "harness_token_refusal_tests.rs"]
mod refusal;

#[path = "harness_token_scheduling_tests.rs"]
mod scheduling;

#[path = "harness_token_atomic_tests.rs"]
mod atomic;

#[path = "harness_checkpoint_update_tests.rs"]
mod updates;

struct TokenFixture {
    model: ModelFixture,
    reviewer: WorkflowActor,
}

async fn configured() -> TokenFixture {
    let source: Value =
        serde_json::from_str(&registry::example("context.history").unwrap().source).unwrap();
    configured_source(source).await
}

async fn configured_source(mut source: Value) -> TokenFixture {
    let fixture = Fixture::new().await;
    let reviewer = member(&fixture, "admin").await;
    source["limits"]["root_budget"] = json!({"activations":10,"model_calls":10,"repetitions":10,
        "tokens":{"max_tokens":150,"reviewers":[{"kind":"user","id":reviewer}]}});
    let binding = BindingFixture::from_fixture(fixture, source).await;
    let f = AdmissionFixture::with_binding(binding).await;
    let admission = f.prepare(f.request("tokens", f.manual())).await;
    f.persistence().admit(&admission).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(admission.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: admission.company_id(),
        run: admission.proposed_run_id(),
        execution: admission.first_execution_id(),
        job: WorkflowJobId(job),
    };
    let model = open_native(native_for(f, scope).await).await;
    TokenFixture {
        model,
        reviewer: WorkflowActor::authenticated(reviewer).unwrap(),
    }
}

#[tokio::test]
async fn workflow_harness_token_approval_never_admits_and_claim_consumes_exact_request() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let request = model_request(&reservation());
    assert!(matches!(
        p.reserve_workflow_model(model.write, request.clone(), Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    assert_eq!(accounting(model).await["calls"], 0);
    assert_eq!(accounting(model).await["rows"], json!([]));
    let paused = stored(&model.native.fixture, model.run.run_id.0).await;
    let command = command(p, model.write.fence.scope, f.reviewer, "approve").await;
    let (left, right) = tokio::join!(
        p.submit_decision(command.clone()),
        p.submit_decision(command.clone())
    );
    left.unwrap();
    right.unwrap();
    assert_eq!(
        stored(&model.native.fixture, model.run.run_id.0).await,
        paused
    );
    assert_eq!(accounting(model).await["calls"], 0);
    p.submit_decision(command).await.unwrap();
    let write = resumed(model, &paused).await;
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("UPDATE task_harness_runs SET model_blocked_decision=NULL,revision=revision+1,checkpoint=jsonb_set(checkpoint,'{revision}',to_jsonb(revision+1)) WHERE id=$1")
        .bind(model.run.run_id.0).execute(&mut *tx).await.unwrap();
    assert!(
        tx.commit().await.is_err(),
        "SQL cannot consume approval without its reservation"
    );
    let mut substituted = request.clone();
    substituted.fingerprint = [8; 32];
    assert!(
        p.reserve_workflow_model(write, substituted, Some(&notices))
            .await
            .is_err()
    );
    assert!(matches!(
        p.reserve_workflow_model(write, request.clone(), Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::New(_)
    ));
    assert_eq!(accounting(model).await["calls"], 1);
    assert!(matches!(
        p.reserve_workflow_model(write, request, Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::PreviouslyAdmitted(_)
    ));
}

async fn resumed(model: &ModelFixture, paused: &Value) -> WorkflowCheckpointWrite {
    let p = model.native.fixture.persistence();
    let claim = p
        .settled()
        .claim_io(model.write.fence.scope, worker(), model.write.lease_policy)
        .await
        .unwrap()
        .unwrap();
    let write = WorkflowCheckpointWrite {
        fence: claim.fence,
        expected_revision: serde_json::from_value(paused["checkpoint"]["revision"].clone())
            .unwrap(),
        ..model.write
    };
    let WriteOutcome::Applied(run) = p.reserve_workflow_execution(write, 1000).await.unwrap()
    else {
        panic!("execution not reserved")
    };
    WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..write
    }
}

#[tokio::test]
async fn workflow_harness_token_rejection_preserves_zero_usage_and_cannot_reopen() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    assert!(matches!(
        p.reserve_workflow_model(model.write, model_request(&reservation()), Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::Parked(_)
    ));
    let reject = command(p, model.write.fence.scope, f.reviewer, "reject").await;
    p.submit_decision(reject.clone()).await.unwrap();
    assert_eq!(accounting(model).await["rows"], json!([]));
    let state: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',run.state,'refusal',usage.token_refusal) FROM workflow_runs AS run JOIN workflow_run_budgets AS link ON link.company_id=run.company_id AND link.run_id=run.id JOIN workflow_root_budget_usage AS usage ON usage.company_id=link.company_id AND usage.root_run_id=link.root_run_id WHERE run.id=$1")
        .bind(model.write.fence.scope.run.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert_eq!(state, json!({"state":"failed","refusal":"rejected"}));
    let mut late = reject;
    late.choice = "approve".into();
    assert!(p.submit_decision(late).await.is_err());
    assert!(
        p.settled()
            .claim_io(model.write.fence.scope, worker(), model.write.lease_policy)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn workflow_harness_token_settlement_records_excess_before_parking() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(model.write, model_request(&request), Some(&notices))
        .await
        .unwrap()
    else {
        panic!("admission failed")
    };
    let run = permit.consume();
    let mut turn = response(&request);
    turn.input_tokens = 100;
    turn.output_tokens = 100;
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..model.write
    };
    let WorkflowModelSettlement::Parked { checkpoint, .. } = p
        .settle_workflow_model(write, turn.clone(), Some(&notices))
        .await
        .unwrap()
    else {
        panic!("settlement did not park")
    };
    assert_eq!(checkpoint.turns.last(), Some(&turn));
    let usage = accounting(model).await;
    assert_eq!(usage["calls"], 1);
    assert_eq!(usage["rows"][0]["input_tokens"], 100);
    assert_eq!(usage["rows"][0]["output_tokens"], 100);
    let paused = stored(&model.native.fixture, model.run.run_id.0).await;
    let approve = command(p, model.write.fence.scope, f.reviewer, "approve").await;
    p.submit_decision(approve).await.unwrap();
    let write = resumed(model, &paused).await;
    // The old settled request cannot become a provider-entry permit.
    assert!(matches!(
        p.reserve_workflow_model(write, model_request(&request), Some(&notices))
            .await
            .unwrap(),
        WorkflowModelAdmission::PreviouslyAdmitted(_)
    ));
}

#[tokio::test]
async fn workflow_harness_token_unavailable_review_replay_cannot_grant_progression() {
    let f = configured().await;
    let model = &f.model;
    let p = model.native.fixture.persistence();
    let notices = notices(p);
    let mut request = reservation();
    request.input_tokens = 50;
    request.output_tokens = 50;
    let WorkflowModelAdmission::New(permit) = p
        .reserve_workflow_model(model.write, model_request(&request), Some(&notices))
        .await
        .unwrap()
    else {
        panic!("admission failed")
    };
    let run = permit.consume();
    let mut turn = response(&request);
    turn.input_tokens = 100;
    turn.output_tokens = 100;
    let write = WorkflowCheckpointWrite {
        expected_revision: run.revision,
        ..model.write
    };
    assert!(matches!(
        p.settle_workflow_model(write, turn.clone(), None)
            .await
            .unwrap(),
        WorkflowModelSettlement::ReviewUnavailable(_)
    ));
    let before = accounting(model).await;
    assert!(matches!(
        p.settle_workflow_model(write, turn, None).await.unwrap(),
        WorkflowModelSettlement::ReviewUnavailable(_)
    ));
    assert_eq!(accounting(model).await, before);
}
