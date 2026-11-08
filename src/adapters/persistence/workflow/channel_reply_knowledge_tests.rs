use super::*;
use crate::application::workflow::{completion::*, waits::*};
use crate::entities::access_level::AccessLevel;

fn source(kind: &str) -> Value {
    let mut source = if kind == "data.map" {
        final_source(kind)
    } else {
        serde_json::from_str(&registry::example(kind).unwrap().source).unwrap()
    };
    source["output_schema"] = json!(true);
    source["steps"]["start"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"channel.no_reply",
        "with":{"value":{"literal":null},"output_schema":{"literal":true}},
        "routes":{"success":"$end"}});
    if kind == "wait.timer" {
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()-chrono::Duration::seconds(1)).to_rfc3339()});
    }
    source
}

enum Review {
    Automatic,
    Required,
}

struct Classified {
    finalization: FinalFixture,
    recipient: Uuid,
}
impl Classified {
    async fn new(kind: &str, review: Review) -> Self {
        let s = Intents::new().await;
        let recipient = policy_runtime_tests::verified_recipient(&s).await;
        let finalization = Box::pin(FinalFixture::configured(
            s,
            source(kind),
            literal_contract("Classified"),
            matches!(review, Review::Required),
        ))
        .await;
        let mut tx = finalization.p.pool().begin().await.unwrap();
        crate::adapters::persistence::workflow::knowledge::raise_content_level_on(
            &mut tx,
            finalization.scope.company,
            finalization.scope.run,
            AccessLevel::L5,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        Self {
            finalization,
            recipient,
        }
    }
    async fn lower_audience(&self) {
        sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
            .bind(self.finalization.scope.company.as_uuid())
            .bind(self.recipient)
            .execute(self.finalization.p.pool())
            .await
            .unwrap();
    }
}

async fn facts(f: &FinalFixture) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('state',run.state,'level',run.content_level,
        'events',(SELECT count(*) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2
            AND event_kind='knowledge.audience_below_content'),
        'executions',(SELECT count(*) FROM workflow_executions WHERE company_id=$1 AND run_id=$2),
        'attempts',(SELECT count(*) FROM task_attempts WHERE task_id=$3),
        'retry',job.retry_count,'ordinal',job.attempt_ordinal,'continuations',job.continuation_count,
        'job',job.status,'proposals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2),
        'decisions',(SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2))
        FROM workflow_runs AS run JOIN background_tasks AS job ON job.company_id=run.company_id AND job.id=$3
        WHERE run.company_id=$1 AND run.id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).bind(f.scope.job.0)
        .fetch_one(f.p.pool()).await.unwrap()
}

async fn assert_refused(f: &FinalFixture, attempts: i64, audit: i64) {
    let actual = facts(f).await;
    assert_eq!(actual["state"], "failed", "{actual}");
    assert_eq!(actual["level"], 5);
    assert_eq!(actual["job"], "failed");
    assert_eq!(actual["events"], audit);
    assert_eq!(
        actual["executions"], 1,
        "knowledge must never take the configured error edge"
    );
    assert_eq!(actual["attempts"], attempts, "no synthetic owner attempt");
    assert_eq!(
        actual["retry"], attempts,
        "only the real fenced failure consumes an attempt"
    );
    assert_eq!(actual["decisions"], 0);
}

#[tokio::test]
async fn channel_reply_knowledge_pure_owner_and_allowed_control() {
    for denied in [false, true] {
        let classified = Box::pin(Classified::new("data.map", Review::Automatic)).await;
        let f = &classified.finalization;
        if denied {
            classified.lower_audience().await;
        }
        let result = f.advance().await;
        if denied {
            assert_eq!(result.disposition, BatchDisposition::Failed);
            assert_refused(f, 0, 1).await;
            assert_run_view_code(f).await;
            assert_eq!(facts(f).await["proposals"], 0);
        } else {
            assert_eq!(result.disposition, BatchDisposition::Completed);
            assert_eq!(f.state().await["decision"], "pending");
        }
    }
}

#[tokio::test]
async fn channel_reply_knowledge_fenced_owner_records_actual_levels_and_allowed_control() {
    for denied in [false, true] {
        let classified = Box::pin(Classified::new("context.history", Review::Automatic)).await;
        let f = &classified.finalization;
        let policy = LeasePolicy::new(Duration::from_secs(5)).unwrap();
        let claim =
            f.p.claim_io(f.scope, WorkflowWorkerId(Uuid::new_v4()), policy)
                .await
                .unwrap()
                .unwrap();
        if denied {
            classified.lower_audience().await;
        }
        let result =
            f.p.complete_io(FencedWorkflowResult {
                fence: claim.fence,
                output: json!({"items":[],"token_count":0,"omitted":0}),
                policy,
            })
            .await
            .unwrap();
        if denied {
            assert!(result.is_none());
            assert_refused(f, 1, 0).await;
            let actual: Value = sqlx::query_scalar(
                "SELECT jsonb_build_object('code',workflow_failure_code,
                'content',knowledge_content_level,'destination',knowledge_destination_level,
                'safety',workflow_retry_safety) FROM task_attempts WHERE task_id=$1",
            )
            .bind(f.scope.job.0)
            .fetch_one(f.p.pool())
            .await
            .unwrap();
            assert_eq!(
                actual,
                json!({"code":"knowledge.audience_below_content",
                "content":5,"destination":0,"safety":"safe"})
            );
            assert!(
                f.p.complete_io(FencedWorkflowResult {
                    fence: claim.fence,
                    output: json!({"items":[],"token_count":0,"omitted":0}),
                    policy
                })
                .await
                .unwrap()
                .is_none()
            );
            assert_refused(f, 1, 0).await;
        } else {
            assert!(result.is_some());
            assert_eq!(f.state().await["run"], "succeeded");
        }
    }
}

#[tokio::test]
async fn channel_reply_knowledge_parked_owner_keeps_real_wait_and_no_new_attempt() {
    for denied in [false, true] {
        let classified = Box::pin(Classified::new("wait.timer", Review::Automatic)).await;
        let f = &classified.finalization;
        let wait = f.p.park_wait(f.scope).await.unwrap().unwrap();
        if denied {
            classified.lower_audience().await;
        }
        let result = f.p.resume_wait(f.scope).await.unwrap();
        if denied {
            assert!(matches!(result, WaitProgress::Failed { .. }));
            assert_refused(f, 0, 1).await;
            let state: String = sqlx::query_scalar(
                "SELECT state FROM workflow_waits WHERE company_id=$1 AND id=$2",
            )
            .bind(f.scope.company.as_uuid())
            .bind(wait.wait.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
            assert_eq!(state, "completed");
            let before = facts(f).await;
            let _ = f.p.resume_wait(f.scope).await;
            assert_eq!(facts(f).await, before);
        } else {
            assert!(matches!(result, WaitProgress::Completed(_)));
            assert_eq!(f.state().await["run"], "succeeded");
        }
    }
}

#[tokio::test]
async fn channel_reply_knowledge_approved_contenders_settle_once_without_reexecuting_handler() {
    for denied in [false, true] {
        let classified = Box::pin(Classified::new("data.map", Review::Required)).await;
        let f = &classified.finalization;
        assert_eq!(f.advance().await.disposition, BatchDisposition::Deferred);
        let saved: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object('business',business_output,'content',content,'integrity',integrity,'operation',operation_digest,'approval',approval_decision_id) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap();
        f.p.submit_decision(f.command("approve").await)
            .await
            .unwrap();
        if denied {
            classified.lower_audience().await;
        }
        let (a, b) = tokio::join!(
            f.p.resume_finalization(f.scope),
            f.p.resume_finalization(f.scope)
        );
        if denied {
            assert!(matches!(a.unwrap(), FinalizationProgress::Refused));
            assert!(matches!(b.unwrap(), FinalizationProgress::Refused));
            assert_refused(f, 0, 1).await;
            assert_eq!(facts(f).await["proposals"], 1);
        } else {
            assert!(matches!(a.unwrap(), FinalizationProgress::Completed(_)));
            assert!(matches!(b.unwrap(), FinalizationProgress::Completed(_)));
            assert_eq!(f.state().await["run"], "succeeded");
        }
        let after: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object('business',business_output,'content',content,'integrity',integrity,'operation',operation_digest,'approval',approval_decision_id) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2",
        )
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .fetch_one(f.p.pool())
        .await
        .unwrap();
        assert_eq!(saved, after, "the approved artifact remains immutable");
    }
}

#[tokio::test]
async fn channel_reply_knowledge_commit_abort_and_lost_receipt_preserve_exact_owner_effects() {
    let classified = Box::pin(Classified::new("data.map", Review::Automatic)).await;
    let f = &classified.finalization;
    classified.lower_audience().await;
    sqlx::raw_sql("CREATE FUNCTION reject_knowledge_audit() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.event_kind='knowledge.audience_below_content' THEN RAISE EXCEPTION 'injected audit commit failure'; END IF; RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER reject_knowledge_audit AFTER INSERT ON workflow_run_events
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_knowledge_audit();")
        .execute(f.p.pool()).await.unwrap();
    let before = facts(f).await;
    assert!(
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap()
        )
        .await
        .is_err()
    );
    assert_eq!(
        facts(f).await,
        before,
        "a real aborted COMMIT leaves no partial refusal or projection"
    );
    sqlx::raw_sql("DROP TRIGGER reject_knowledge_audit ON workflow_run_events; DROP FUNCTION reject_knowledge_audit();")
        .execute(f.p.pool()).await.unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    let result = crate::adapters::persistence::workflow::batch::advance_on(
        &mut tx,
        f.scope,
        BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        f.p.channel_reply_notices(),
    )
    .await
    .unwrap();
    assert_eq!(result.disposition, BatchDisposition::Failed);
    tx.commit().await.unwrap(); // Intentionally discard the successful caller receipt.
    assert_refused(f, 0, 1).await;
    let committed = facts(f).await;
    let _ =
        f.p.advance_pure(
            f.scope,
            BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
        )
        .await;
    assert_eq!(
        facts(f).await,
        committed,
        "retry cannot infer success or repeat refusal effects"
    );
}

async fn assert_run_view_code(f: &FinalFixture) {
    use crate::adapters::http::pages::{RunPaneView, run_pane};
    use crate::application::workflow::run_views::{RunReadScope, RunViews};
    let view =
        f.p.run_view(RunReadScope {
            company: f.scope.company,
            viewer: f.s.owner(),
            run: f.scope.run,
        })
        .await
        .unwrap()
        .unwrap();
    assert!(
        view.events
            .iter()
            .any(|event| event.kind == "knowledge.audience_below_content")
    );
    let html = run_pane(&RunPaneView {
        company_id: f.scope.company.as_uuid(),
        viewer: f.s.owner().user_id(),
        epoch: Uuid::new_v4(),
        run: &view,
        graph_html: "",
        people: &Default::default(),
        banner: None,
    });
    assert!(html.contains("knowledge.audience_below_content"));
    assert!(!html.contains("knowledge.audience below content"));
}
