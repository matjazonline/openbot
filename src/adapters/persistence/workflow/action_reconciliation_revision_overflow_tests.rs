//! Numerical initial-value fixture; all later transitions retain their real owners.
use super::*;

// Calibrated against the matching DEFAULT1 history; changed setup must fail early.
const SETUP_REVISION_DELTA: u64 = 8;

struct LostApplied {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<LedgerVerifier>,
    provider: LedgerProvider,
    schema: Value,
}

struct InitiallyAdmitted {
    fixture: AdmissionFixture,
    scope: ActivationRequest,
    schema: Value,
}

async fn revision_schema(f: &AdmissionFixture) -> Value {
    let default: String = sqlx::query_scalar("SELECT pg_get_expr(attr_default.adbin,attr_default.adrelid) FROM pg_attrdef AS attr_default JOIN pg_attribute AS attribute ON attribute.attrelid=attr_default.adrelid AND attribute.attnum=attr_default.adnum WHERE attr_default.adrelid='workflow_runs'::regclass AND attribute.attname='revision'")
        .fetch_one(f.persistence().pool()).await.unwrap();
    let triggers: Value = sqlx::query_scalar("SELECT jsonb_agg(jsonb_build_object('table',table_name.relname,'name',owned_trigger.tgname,'enabled',owned_trigger.tgenabled,'definition',pg_get_triggerdef(owned_trigger.oid),'function',pg_get_functiondef(owned_trigger.tgfoid)) ORDER BY table_name.relname,owned_trigger.tgname) FROM pg_trigger AS owned_trigger JOIN pg_class AS table_name ON table_name.oid=owned_trigger.tgrelid JOIN pg_namespace AS namespace ON namespace.oid=table_name.relnamespace WHERE namespace.nspname='public'")
        .fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(default, "1");
    assert!(
        triggers
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["enabled"] != "D")
    );
    json!({"default":default,"triggers":triggers})
}

async fn initially_admitted(initial: u64) -> InitiallyAdmitted {
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let schema = revision_schema(&f).await;
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(
        runs, 0,
        "initial default injection belongs only to an empty isolated fixture"
    );
    assert!((1..=i64::MAX as u64).contains(&initial));
    // Only this disposable database's omitted INSERT input changes; no live revision UPDATE.
    sqlx::query(&format!(
        "ALTER TABLE workflow_runs ALTER COLUMN revision SET DEFAULT {initial}"
    ))
    .execute(f.persistence().pool())
    .await
    .unwrap();
    let command = f.prepare(f.request("overflow-initial", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    sqlx::query("ALTER TABLE workflow_runs ALTER COLUMN revision SET DEFAULT 1")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(revision_schema(&f).await, schema);
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    };
    InitiallyAdmitted {
        fixture: f,
        scope,
        schema,
    }
}

async fn lost_applied(initial: u64) -> LostApplied {
    let InitiallyAdmitted {
        fixture: f,
        scope,
        schema,
    } = Box::pin(initially_admitted(initial)).await;
    // Box the real admission/claim/action seam to preserve stock 2 MiB test stacks.
    let (fixture, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    let verifier = ledger(&fixture, &request).await;
    let provider = LedgerProvider::new(
        &fixture,
        Delivery::Apply {
            result: good(),
            recover: true,
            lose: true,
        },
    );
    let observed = invoke(&fixture, &request, &provider).await;
    assert!(
        matches!(&observed, Err(AppError::Timeout(message)) if message == "response lost after durable effect")
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(&fixture).await, 1);
    assert_eq!(counts(&fixture).await.0, 0);
    park(&fixture, &request).await;
    assert_eq!(revision_schema(&fixture).await, schema);
    LostApplied {
        fixture,
        request,
        verifier,
        provider,
        schema,
    }
}

fn waiting_history(before: &Value) {
    assert_eq!(before["workflow_runs"][0]["state"], "waiting");
    assert_eq!(
        before["workflow_runs"][0]["waiting_reason"],
        "reconciliation"
    );
    assert_eq!(before["background_tasks"][0]["status"], "failed");
    for column in [
        "worker_id",
        "execution_generation",
        "locked_at",
        "lock_expires_at",
    ] {
        assert!(
            before["background_tasks"][0]
                .get(column)
                .expect("background task lease column must exist")
                .is_null(),
            "{column}"
        );
    }
    assert_eq!(
        before["workflow_action_remote_entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_receipts",
        "workflow_action_evidence_commands",
        "workflow_action_evidence_conflicts",
        "workflow_action_evidence_consumptions",
    ] {
        assert_eq!(before[table], json!([]), "{table}");
    }
    assert_eq!(
        before["fixture_provider_effects"].as_array().unwrap().len(),
        1
    );
}

fn recovered_links(after: &Value, command: &ReconcileActionCommand, saved: &ReconciliationResult) {
    assert_eq!(
        saved.outcome,
        ReconciliationOutcome::Scheduled { receipt_only: true }
    );
    assert!(!saved.replayed);
    for table in [
        "workflow_action_evidence",
        "workflow_action_evidence_coverage",
        "workflow_action_receipts",
        "workflow_action_evidence_commands",
    ] {
        assert_eq!(after[table].as_array().unwrap().len(), 1, "{table}");
    }
    let evidence = &after["workflow_action_evidence"][0];
    let coverage = &after["workflow_action_evidence_coverage"][0];
    let receipt = &after["workflow_action_receipts"][0];
    let stored = &after["workflow_action_evidence_commands"][0];
    assert_eq!(evidence["id"], json!(saved.evidence.unwrap().as_uuid()));
    assert_eq!(evidence["disposition"], "applied");
    for row in [evidence, coverage, receipt, stored] {
        assert_eq!(row["company_id"], json!(command.scope.company.as_uuid()));
        assert_eq!(row["run_id"], json!(command.scope.run.as_uuid()));
        assert_eq!(
            row["execution_id"],
            json!(command.scope.execution.as_uuid())
        );
        assert_eq!(
            row["invocation_id"],
            json!(command.subject.invocation.as_uuid())
        );
        assert_eq!(
            row["argument_digest"],
            json!(command.subject.argument_digest.as_str())
        );
        assert_eq!(row["dispatch_id"], json!(command.marker.as_uuid()));
    }
    assert_eq!(coverage["evidence_id"], evidence["id"]);
    assert_eq!(
        coverage["remote_entry_id"],
        after["workflow_action_remote_entries"][0]["id"]
    );
    assert_eq!(receipt["reconciliation_evidence_id"], evidence["id"]);
    assert!(receipt["remote_entry_id"].is_null());
    assert_eq!(receipt["result"], good());
    assert_eq!(stored["evidence_id"], evidence["id"]);
    assert_eq!(stored["command_key"], json!(command.command_key.as_str()));
    assert_eq!(stored["outcome"], json!(saved.outcome));
    assert_eq!(
        stored["expected_revision"],
        json!(command.expected_revision.0)
    );
    assert_eq!(stored["result_revision"], json!(saved.revision.0));
    assert_eq!(
        stored["scheduled_job_id"],
        after["background_tasks"][0]["id"]
    );
    assert_eq!(
        after["workflow_runs"][0]["revision"],
        json!(saved.revision.0)
    );
    assert_eq!(after["workflow_runs"][0]["state"], "running");
    assert_eq!(after["background_tasks"][0]["status"], "pending");
    assert!(
        after["workflow_run_events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["sequence"] == stored["audit_sequence"]
                && event["event_kind"] == "action_reconciled"
                && event["actor_id"] == json!(command.actor.user_id())
                && event["execution_id"] == json!(command.scope.execution.as_uuid()))
    );
}

async fn normal_headroom() {
    let history = Box::pin(lost_applied(1)).await;
    let f = &history.fixture;
    let command = proof_command(f, &history.request, &history.verifier, "normal-headroom").await;
    let before = all_tables(f).await;
    waiting_history(&before);
    let delta = command.expected_revision.0 - 1;
    eprintln!(
        "normal initial=1 setup_delta={delta} pre_command={}",
        command.expected_revision.0
    );
    assert_eq!(
        delta, SETUP_REVISION_DELTA,
        "changed setup requires recalibration, never live revision repair"
    );
    let saved = run_command(f, &command, history.verifier).await.unwrap();
    let after = all_tables(f).await;
    recovered_links(&after, &command, &saved);
    assert_eq!(
        after["fixture_provider_effects"],
        before["fixture_provider_effects"]
    );
    assert_eq!(
        after["workflow_action_remote_entries"],
        before["workflow_action_remote_entries"]
    );
    assert_eq!(history.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(revision_schema(f).await, history.schema);
    eprintln!(
        "normal outcome={:?} final_revision={} restored_default=1 triggers_unchanged=true",
        saved.outcome, saved.revision.0
    );
}

#[tokio::test]
async fn workflow_action_reconciliation_revision_overflow_recovered_applied_later_reopen_rolls_back()
 {
    Box::pin(normal_headroom()).await;
    let target = i64::MAX as u64 - 1;
    let initial = target - SETUP_REVISION_DELTA;
    let history = Box::pin(lost_applied(initial)).await;
    let f = &history.fixture;
    let command = proof_command(
        f,
        &history.request,
        &history.verifier,
        "later-owner-overflow",
    )
    .await;
    assert_eq!(command.expected_revision.0, target);
    let before = all_tables(f).await;
    waiting_history(&before);
    assert_eq!(before["workflow_runs"][0]["revision"], json!(target));
    // facts::insert_on reaches MAX and adds coverage; truth_on adds the recovered
    // receipt. The following continue_on failed-job UPDATE overflows its owner.
    // Audit/command insertion is later and never executes on this error path.
    let error = run_command(f, &command, history.verifier)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, AppError::Database(message)
        if message == "error returned from database: bigint out of range"),
        "{error}"
    );
    assert_eq!(
        all_tables(f).await,
        before,
        "all public rows, including provider effect, roll back exactly"
    );
    assert_eq!(history.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects(f).await, 1);
    assert_eq!(revision_schema(f).await, history.schema);
    // AppError::from(sqlx::Error) preserves display text but not SQLSTATE22003.
    eprintln!(
        "overflow initial={initial} setup_delta={SETUP_REVISION_DELTA} pre_command={target} diagnostic={error} whole_public_unchanged=true restored_default=1 triggers_unchanged=true"
    );
}
