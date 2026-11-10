//! Explicit two-company document workflow against the real disposable private Doop image.
use super::*;
use crate::adapters::persistence::doop_live_input::{Company, Input, input};
use crate::application::workflow::AdmitWorkflowRequest;
use crate::services::test_support::ScriptedLlm;
use secrecy::ExposeSecret;
use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, time::Instant};

#[path = "doop_document_live_client.rs"]
mod client;
#[path = "doop_document_live_driver.rs"]
mod driver;
#[path = "doop_document_live_model.rs"]
mod model;
#[path = "doop_document_live_setup.rs"]
mod setup;
#[path = "doop_document_live_verification.rs"]
mod verification;

struct KnownRun {
    scope: RunReadScope,
    replay: AdmitWorkflowRequest,
}
struct KnownDocuments {
    records: Vec<Value>,
    models: Vec<ScriptedLlm>,
}

async fn admit_known(
    environment: &setup::Environment,
    document: &setup::CompanyDocument,
    index: usize,
) -> KnownRun {
    let runtime = document.runtime(&environment.engine);
    let value = json!({"title":format!("Company {} actual workflow document",index+1),
            "brief":"Write a short synthetic project brief, then perform one targeted edit."});
    let request = runtime.request(value.clone());
    let mut replay = runtime.request(value);
    replay.idempotency_key = request.idempotency_key.clone();
    replay.trigger = request.trigger.clone();
    replay.correlation_id = request.correlation_id;
    let AdmissionResult::Created(run) = environment
        .engine
        .services()
        .service()
        .admit(request)
        .await
        .unwrap()
    else {
        panic!("new document admission required");
    };
    environment.engine.services().wake_workers();
    KnownRun {
        scope: RunReadScope {
            company: document.target.company,
            run,
            viewer: document.target.actor,
        },
        replay,
    }
}

async fn known_documents(
    environment: &setup::Environment,
    documents: &[setup::CompanyDocument],
    delivery: &delivery_support::DeliveryFixture,
    input: &Input,
    started: Instant,
) -> KnownDocuments {
    let mut runs = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        runs.push(Box::pin(admit_known(environment, document, index)).await);
    }
    stage(started, "documents_admitted");
    let scopes: Vec<_> = runs.iter().map(|run| run.scope).collect();
    let coordinator = driver::Driver {
        environment,
        documents,
        delivery,
    };
    // Model endpoints are read only at agent execution. Preserve their60s lifetime by
    // starting them after the real initial create approvals, before any editor can run.
    coordinator
        .drive(
            &environment.engine,
            &scopes,
            driver::Checkpoint::CreateApproval,
        )
        .await;
    let mut models = Vec::new();
    for (index, document) in documents.iter().enumerate() {
        models.push(model::start(document.agent, environment.client.clone(), index).await);
    }
    stage(started, "model_listeners_started");
    coordinator
        .drive(
            &environment.engine,
            &scopes,
            driver::Checkpoint::EditorApproval,
        )
        .await;
    stage(started, "editor_approval_checkpoint");
    for index in 0..2 {
        let effects = environment.client.company(index);
        assert_eq!(effects.count("create_canvas"), 1);
        assert_eq!(effects.count("create_frame"), 0);
    }
    let restarted = WorkflowEngine::for_tests_with_mcp(
        environment.persistence.as_ref().clone(),
        environment.client.clone(),
    );
    let views = coordinator
        .drive(&restarted, &scopes, driver::Checkpoint::Terminal)
        .await;
    stage(started, "documents_terminal");
    if views
        .iter()
        .any(|view| view.detail.summary.state != "succeeded")
    {
        Box::pin(failed_documents(environment, &scopes, &views, models)).await;
    }
    let verifier = verification::Verifier {
        environment,
        delivery,
        input,
    };
    let mut records = Vec::new();
    for (index, ((document, run), view)) in documents.iter().zip(runs).zip(views).enumerate() {
        records.push(Box::pin(verifier.document(document, run.scope, &view, index)).await);
        assert_eq!(
            restarted
                .services()
                .service()
                .admit(run.replay)
                .await
                .unwrap(),
            AdmissionResult::Replayed(run.scope.run)
        );
    }
    KnownDocuments { records, models }
}

async fn failed_documents(
    environment: &setup::Environment,
    scopes: &[RunReadScope],
    views: &[RunView],
    models: Vec<ScriptedLlm>,
) -> ! {
    for (scope, view) in scopes.iter().zip(views) {
        eprintln!("DOOP_E2E_FAILURE state={}", view.detail.summary.state);
        for execution in &view.detail.executions {
            let step = environment
                .persistence
                .step_view(*scope, execution.execution)
                .await
                .unwrap()
                .unwrap();
            for attempt in step.attempts {
                if let Some(code) = attempt.failure_code {
                    eprintln!("DOOP_E2E_FAILURE step={} code={code}", execution.step);
                }
            }
        }
    }
    for mut model in models {
        eprintln!("DOOP_E2E_MODEL observed={}", model.observed().len());
        match model.finish().await {
            Ok(count) => eprintln!("DOOP_E2E_MODEL completed={count}"),
            Err(class) => eprintln!("DOOP_E2E_MODEL failure={class}"),
        }
    }
    panic!("both real document workflows must succeed");
}

async fn lost_receipt(
    environment: &setup::Environment,
    documents: &[setup::CompanyDocument],
    delivery: &delivery_support::DeliveryFixture,
    started: Instant,
) -> Value {
    environment.client.lose_next_create(0);
    let scope = Box::pin(environment.uncertain(&documents[0])).await;
    let coordinator = driver::Driver {
        environment,
        documents,
        delivery,
    };
    coordinator
        .drive(
            &environment.engine,
            &[scope],
            driver::Checkpoint::Reconciliation,
        )
        .await;
    stage(started, "lost_receipt_parked");
    let first = verification::unknown(environment, scope).await;
    let effects = environment.client.company(0);
    assert_eq!(effects.canvases.len(), 2);
    assert_eq!(effects.count("create_canvas"), 2);
    let health = Box::pin(environment.health(&documents[1])).await;
    let restarted = WorkflowEngine::for_tests_with_mcp(
        environment.persistence.as_ref().clone(),
        environment.client.clone(),
    );
    let healthy = coordinator
        .drive(&restarted, &[health], driver::Checkpoint::Terminal)
        .await;
    assert_eq!(healthy[0].detail.summary.state, "succeeded");
    stage(started, "healthy_progress_after_worker_restart");
    let second = verification::unknown(environment, scope).await;
    assert_eq!(first.detail.actions.len(), second.detail.actions.len());
    assert_eq!(environment.client.company(0).count("create_canvas"), 2);
    assert_eq!(environment.client.company(1).count("create_canvas"), 1);
    json!({"openbot_company":scope.company,"run_id":scope.run,"canvas_id":effects.canvases[1],
        "remote_effect":"actual confirmed create","injected_fault":"receipt lost after successful real response",
        "outcome":"unknown","manual_reconciliation":true,"blind_replay":false,"document_notifications":0,
        "independent_no_effect_workflow":"succeeded"})
}

fn stage(started: Instant, label: &'static str) {
    println!(
        "DOOP_E2E_STAGE stage={label} elapsed_ms={}",
        started.elapsed().as_millis()
    );
}

fn write_record(input: &Input, documents: Vec<Value>, lost: Value) {
    let record = json!({"version":1,"origin":input.origin,"source":input.source,"companies":documents,
        "lost_receipt":lost,"model_fixture":"deterministic responses over normal provider HTTP",
        "paid_live_inference":"NOT EXERCISED","snapshot_guard":"enabled before and after",
        "worker_groups":2,"known_create_worker_restart":true,"delivery_owner":"single real counted DeliveryWorker/EmailRenderer"});
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    assert!(bytes.len() <= 16384);
    let encoded = std::str::from_utf8(&bytes).unwrap();
    for company in &input.companies {
        assert!(!encoded.contains(company.token.expose_secret()));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(std::env::var_os("DOOP_TEST_SETUP_RECORD").expect("owned result path"))
        .unwrap();
    output.write_all(&bytes).unwrap();
}

#[tokio::test]
#[ignore = "explicit disposable private Doop image and owned PostgreSQL; strict private stdin required"]
async fn doop_e2e_live_company_documents() {
    let started = Instant::now();
    let input = input();
    let environment = Box::pin(setup::Environment::new(&input)).await;
    stage(started, "environment_ready");
    let mut documents = Vec::new();
    for company in &input.companies {
        documents.push(Box::pin(environment.company(company, &input.origin)).await);
        stage(started, "company_registered_and_published");
    }
    environment
        .assert_foreign(&documents[0], &documents[1])
        .await;
    environment
        .assert_foreign(&documents[1], &documents[0])
        .await;
    let delivery = delivery_support::worker(&documents[0].fixture, documents[0].channel);
    let known = Box::pin(known_documents(
        &environment,
        &documents,
        &delivery,
        &input,
        started,
    ))
    .await;
    let lost = Box::pin(lost_receipt(&environment, &documents, &delivery, started)).await;
    for mut model in known.models {
        let observed = serde_json::to_string(&model.observed()).unwrap();
        for company in &input.companies {
            assert!(!observed.contains(company.token.expose_secret()));
        }
        assert_eq!(model.finish().await.unwrap(), 5);
    }
    environment.service.shutdown().await.unwrap();
    setup::immutable_guard(&environment.persistence).await;
    write_record(&input, known.records, lost);
    println!("DOOP_OPENBOT_E2E_PASS companies=2 documents=2 creates=3 unknown=1");
}
