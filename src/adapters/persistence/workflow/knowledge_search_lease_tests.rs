//! Actual database ownership, deferred COMMIT fencing and ambiguous acknowledgment recovery.
use super::*;
use crate::application::knowledge::search::KnowledgeObservation;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{
    Abort, Gate, GatePool, Stall, connect,
};

impl GatePool for SearchFixture {
    fn gate_pool(&self) -> &sqlx::PgPool {
        self.p().pool()
    }
}

fn private_observation() -> KnowledgeObservation {
    KnowledgeObservation::new(
        json!({"passages":[{"file":"sales.md","heading":null,"text":"PRIVATE FLOOR 60"}]}),
        Some(AccessLevel::L5),
    )
}

async fn expire(f: &SearchFixture, boundary: &str) {
    let statement = match boundary {
        "lease" => {
            "UPDATE background_tasks SET lock_expires_at=clock_timestamp()-interval '1 microsecond' WHERE id=$1"
        }
        "deadline" => {
            // Fault injection changes only the deadline; the admitted ceiling is never rewritten.
            sqlx::query(
                "ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable",
            )
            .execute(f.p().pool())
            .await
            .unwrap();
            "UPDATE workflow_runs SET deadline=clock_timestamp()-interval '1 microsecond' WHERE id=$1"
        }
        _ => unreachable!(),
    };
    let id = if boundary == "lease" {
        f.claim.fence.scope.job.0
    } else {
        f.claim.fence.scope.run.as_uuid()
    };
    sqlx::query(statement)
        .bind(id)
        .execute(f.p().pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn knowledge_search_expired_database_lease_or_deadline_refuses_before_embedding() {
    for boundary in ["lease", "deadline"] {
        let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
        let call = f.prepare(json!({"query":"floor"})).await;
        expire(&f, boundary).await;
        let before = f.snapshot().await;
        assert!(
            f.knowledge()
                .call_knowledge_tool(f.claim.fence, call)
                .await
                .is_err(),
            "{boundary}"
        );
        assert_eq!(f.snapshot().await, before);
        assert_eq!(f.model.query_count(), 0);
        f.library.shutdown().await.unwrap();
    }
}

async fn inject_expiry(f: &SearchFixture, phase: &str, boundary: &str) {
    let expiry = match boundary {
        "lease" => {
            "UPDATE background_tasks SET lock_expires_at=clock_timestamp()-interval '1 microsecond' WHERE id=NEW.job_id;"
        }
        "deadline" => {
            sqlx::query(
                "ALTER TABLE workflow_runs DISABLE TRIGGER workflow_run_snapshot_immutable",
            )
            .execute(f.p().pool())
            .await
            .unwrap();
            "UPDATE workflow_runs SET deadline=clock_timestamp()-interval '1 microsecond' WHERE id=NEW.run_id;"
        }
        _ => unreachable!(),
    };
    let trigger = match phase {
        "statement" => {
            "CREATE TRIGGER search_expiry BEFORE INSERT ON workflow_knowledge_consumptions FOR EACH ROW EXECUTE FUNCTION search_expiry()"
        }
        "commit" => {
            "CREATE CONSTRAINT TRIGGER aa_search_expiry AFTER INSERT ON workflow_knowledge_consumptions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION search_expiry()"
        }
        _ => unreachable!(),
    };
    sqlx::raw_sql(&format!(
        "CREATE SEQUENCE search_expiry_entered;
        CREATE FUNCTION search_expiry() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
        PERFORM nextval('search_expiry_entered'); {expiry} RETURN NEW; END $$; {trigger}"
    ))
    .execute(f.p().pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn knowledge_search_actual_statement_and_commit_expiry_roll_back_payload_rise_and_proof() {
    for phase in ["statement", "commit"] {
        for boundary in ["lease", "deadline"] {
            let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
            let call = f.prepare(json!({"query":"floor"})).await;
            inject_expiry(&f, phase, boundary).await;
            let before = f.snapshot().await;
            let error = f
                .p()
                .observe_knowledge(f.claim.fence, &call, private_observation())
                .await
                .unwrap_err();
            let entered: bool = sqlx::query_scalar("SELECT is_called FROM search_expiry_entered")
                .fetch_one(f.p().pool())
                .await
                .unwrap();
            assert!(
                entered,
                "{phase}/{boundary}: actual transaction entered the fault"
            );
            if phase == "commit" {
                assert!(
                    format!("{error:?}").contains("knowledge_consumption_live"),
                    "{error:?}"
                );
            }
            assert_eq!(f.snapshot().await, before, "{phase}/{boundary}");
            f.library.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn knowledge_search_real_replacement_claim_replays_original_and_fences_stale_competitor() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    f.upload_sales().await;
    let call = f.prepare(json!({"query":"floor"})).await;
    let original = f
        .knowledge()
        .call_knowledge_tool(f.claim.fence, copy_call(&call))
        .await
        .unwrap();
    assert!(matches!(original, AgentToolObservation::Completed(_)));
    assert_eq!(f.model.query_count(), 1);
    assert!(
        f.p()
            .release_io(f.claim.fence, lease(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()-interval '1 microsecond' WHERE id=$1",
    )
    .bind(f.claim.fence.scope.job.0)
    .execute(f.p().pool())
    .await
    .unwrap();
    let current = f
        .p()
        .claim_io(
            f.claim.fence.scope,
            WorkflowWorkerId(Uuid::new_v4()),
            lease(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_ne!(current.fence.generation, f.claim.fence.generation);
    assert!(tokio::time::Instant::now() < f.claim.window.expires);
    let barrier = tokio::sync::Barrier::new(2);
    let (old, new) = tokio::join!(
        async {
            barrier.wait().await;
            f.p().knowledge_context(f.claim.fence, &call).await
        },
        async {
            barrier.wait().await;
            f.knowledge()
                .call_knowledge_tool(current.fence, copy_call(&call))
                .await
        },
    );
    assert!(old.is_err(), "stale cached lease grants no content");
    let AgentToolObservation::Completed(replayed) = new.unwrap() else {
        panic!("missing canonical result")
    };
    let AgentToolObservation::Completed(original) = original else {
        unreachable!()
    };
    assert_eq!(replayed, original);
    assert_eq!(f.model.query_count(), 1);
    assert_eq!(f.snapshot().await["content"], 5);
    assert_eq!(f.snapshot().await["proofs"], 2);
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_proof_rewrite_delete_and_cross_company_insert_are_rejected() {
    let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let call = f.prepare(json!({"query":"floor"})).await;
    f.p()
        .observe_knowledge(f.claim.fence, &call, private_observation())
        .await
        .unwrap();
    let before = f.snapshot().await;
    for statement in [
        "UPDATE workflow_knowledge_consumptions SET content_level=6 WHERE job_id=$1",
        "UPDATE workflow_knowledge_consumptions SET result='{}' WHERE job_id=$1",
        "DELETE FROM workflow_knowledge_consumptions WHERE job_id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(f.claim.fence.scope.job.0)
            .execute(f.p().pool())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("immutable"), "{error:?}");
        assert_eq!(f.snapshot().await, before);
    }
    let foreign = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
    let error = sqlx::query("INSERT INTO workflow_knowledge_consumptions
        SELECT $2,run_id,execution_id,job_id,attempt_number,execution_generation,worker_id,
            checkpoint_id,invocation_id,content_level,result FROM workflow_knowledge_consumptions WHERE job_id=$1")
        .bind(f.claim.fence.scope.job.0).bind(foreign.claim.fence.scope.company.as_uuid())
        .execute(f.p().pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    assert_eq!(f.snapshot().await, before);
    foreign.library.shutdown().await.unwrap();
    f.library.shutdown().await.unwrap();
}

#[tokio::test]
async fn knowledge_search_unknown_commit_ack_returns_no_content_then_reconstruction_reads_truth() {
    for applies in [true, false] {
        let f = Box::pin(SearchFixture::new(AccessLevel::L5)).await;
        f.upload_sales().await;
        let call = f.prepare(json!({"query":"floor"})).await;
        let before = f.snapshot().await;
        let (mut observer, _) = connect(&f).await;
        let mut gate = Gate::arm(
            &f,
            "workflow_knowledge_consumptions",
            "INSERT",
            "true",
            Stall::Commit,
        )
        .await;
        let p = f.p().clone();
        let fence = f.claim.fence;
        let pending = copy_call(&call);
        let task = tokio::spawn(async move {
            p.observe_knowledge(fence, &pending, private_observation())
                .await
        });
        let _stalled = gate
            .stalled(&mut observer, &[&|| !task.is_finished()])
            .await;
        if !applies {
            gate.abort_on_release(Abort::Every).await;
        }
        let error = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("knowledge.observe commit outcome unknown"),
            "{error:?}"
        );
        assert_eq!(
            f.snapshot().await,
            before,
            "unacknowledged COMMIT is still blocked"
        );
        gate.drain().await;
        assert_eq!(f.snapshot().await["proofs"], if applies { 1 } else { 0 });
        sqlx::raw_sql("DROP TRIGGER lease_gate_arm ON workflow_knowledge_consumptions; DROP TRIGGER aa_lease_gate ON workflow_knowledge_consumptions")
            .execute(f.p().pool()).await.unwrap();
        let AgentToolObservation::Completed(result) = f
            .knowledge()
            .call_knowledge_tool(f.claim.fence, call)
            .await
            .unwrap()
        else {
            panic!("reconstruction did not resolve canonical truth")
        };
        assert!(result.to_string().contains("PRIVATE FLOOR 60"));
        assert_eq!(f.model.query_count(), if applies { 0 } else { 1 });
        assert_eq!(f.snapshot().await["content"], 5);
        assert_eq!(f.snapshot().await["proofs"], 1);
        f.library.shutdown().await.unwrap();
    }
}
