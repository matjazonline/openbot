use super::super::message_tests::support::member;
use super::*;
use crate::adapters::protocols::email::EmailRenderer;
use crate::application::transport::{DeliveryComposer, TransportRenderers};
use crate::application::workflow::{activation::*, decisions::*, waits::*};
use tokio::sync::Barrier;

pub(super) struct DecisionFixture {
    pub(super) admission: AdmissionFixture,
    pub(super) scope: ActivationRequest,
    pub(super) users: Vec<Uuid>,
    pub(super) notices: DecisionNotices,
}
impl DecisionFixture {
    pub(super) fn new(
        count: usize,
        error_route: Option<&str>,
    ) -> impl std::future::Future<Output = Self> {
        Self::configured(count, move |source| {
            if let Some(route) = error_route {
                source["steps"]["start"]["routes"]["error"] = json!(route);
            }
        })
    }
    async fn configured(count: usize, customize: impl FnOnce(&mut Value)) -> Self {
        let fixture = Fixture::new().await;
        let mut users = Vec::new();
        for _ in 0..count {
            users.push(member(&fixture, "admin").await);
        }
        let mut source: Value =
            serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
        source["steps"]["start"]["with"]["reviewers"] = json!({"literal":users.iter().map(|id| json!({"kind":"user","id":id})).collect::<Vec<_>>()});
        source["steps"]["start"]["with"]["deadline"] =
            json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
        source["steps"]["start"]["with"]["data_schema"] =
            json!({"literal":{"type":"string","maxLength":32}});
        source["steps"]["start"]["routes"]["choices"] =
            json!({"accept":"finish","revise":"finish"});
        source["steps"]["finish"] = json!({"type":"data.map","with":{"value":{"literal":1},"output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
        customize(&mut source);
        let admission =
            AdmissionFixture::with_binding(BindingFixture::from_fixture(fixture, source).await)
                .await;
        let command = admission
            .prepare(admission.request("decision", admission.manual()))
            .await;
        admission.persistence().admit(&command).await.unwrap();
        let job =
            sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(admission.persistence().pool())
                .await
                .unwrap();
        let scope = ActivationRequest {
            company: command.company_id(),
            run: command.proposed_run_id(),
            execution: command.first_execution_id(),
            job: WorkflowJobId(job),
        };
        let renderers = Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new("agents.test")))
                .unwrap(),
        );
        let notices = DecisionNotices::new(
            DeliveryComposer::new(renderers, Arc::new(admission.persistence().clone())),
            "https://agents.test",
            "agents.test",
        );
        Self {
            admission,
            scope,
            users,
            notices,
        }
    }
    pub(super) fn persistence(&self) -> &PostgresPersistence {
        self.admission.persistence()
    }
    fn actor(&self, index: usize) -> WorkflowActor {
        WorkflowActor::authenticated(self.users[index]).unwrap()
    }
    pub(super) async fn park(&self) -> DecisionView {
        self.persistence()
            .park_decision(self.scope, &self.notices)
            .await
            .unwrap()
            .unwrap();
        let id = sqlx::query_scalar(
            "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
        )
        .bind(self.scope.company.as_uuid())
        .bind(self.scope.execution.as_uuid())
        .fetch_one(self.persistence().pool())
        .await
        .unwrap();
        self.persistence()
            .read_decision(self.scope.company, self.actor(0), DecisionId(id))
            .await
            .unwrap()
    }
    pub(super) fn submit(&self, view: &DecisionView, index: usize, key: &str) -> SubmitDecision {
        SubmitDecision {
            company: self.scope.company,
            actor: self.actor(index),
            decision: view.decision,
            command_key: DecisionCommandKey::parse(key).unwrap(),
            expected_revision: view.revision,
            expected_artifact: view.artifact_digest.clone(),
            choice: "accept".into(),
            feedback: String::new(),
            data: json!("accepted"),
        }
    }
    async fn counts(&self) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('decisions',(SELECT count(*) FROM workflow_decisions),'waits',(SELECT count(*) FROM workflow_waits),'reviewers',(SELECT count(*) FROM workflow_decision_reviewers),'deliveries',(SELECT count(*) FROM message_deliveries),'executions',(SELECT count(*) FROM workflow_executions),'run',(SELECT state FROM workflow_runs LIMIT 1),'job',(SELECT status FROM background_tasks WHERE id=$1))")
            .bind(self.scope.job.0).fetch_one(self.persistence().pool()).await.unwrap()
    }
}

#[tokio::test]
async fn workflow_decision_park_submit_restart_replay_and_notices() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let view = f.park().await;
    assert_eq!(view.state, "waiting");
    assert_eq!(view.reviewers.len(), 2);
    assert_eq!(f.park().await, view);
    let before = f.counts().await;
    assert_eq!(before["deliveries"], 2);
    let command = f.submit(&view, 0, "one");
    let applied = f
        .persistence()
        .submit_decision(command.clone())
        .await
        .unwrap();
    assert_eq!(applied.disposition, DecisionDisposition::Applied);
    assert!(applied.successor.is_some());
    let restarted = PostgresPersistence::new(f.persistence().pool().clone());
    let replay = restarted.submit_decision(command.clone()).await.unwrap();
    assert_eq!(replay.disposition, DecisionDisposition::Replayed);
    assert_eq!(replay.successor, applied.successor);
    let mut different = command;
    different.data = json!("different");
    assert!(matches!(
        restarted.submit_decision(different).await,
        Err(AppError::Conflict(_))
    ));
    let after = f.counts().await;
    assert_eq!(after["executions"], 2);
    assert_eq!(after["deliveries"], 2);
    let read = restarted
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(read.revision, 2);
    assert_eq!(read.state, "completed");
    assert_eq!(read.submission.unwrap().data, json!("accepted"));
}

#[tokio::test]
async fn workflow_decision_two_reviewers_compete_for_one_successor() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let view = f.park().await;
    let barrier = Barrier::new(2);
    let submit = |index| {
        let (barrier, f, view) = (&barrier, &f, &view);
        async move {
            barrier.wait().await;
            f.persistence()
                .submit_decision(f.submit(view, index, "race"))
                .await
        }
    };
    let (left, right) = tokio::join!(submit(0), submit(1));
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let loser = if left.is_err() { left } else { right };
    assert!(matches!(loser, Err(AppError::Conflict(_))));
    assert_eq!(f.counts().await["executions"], 2);
}

#[tokio::test]
async fn workflow_decision_invalid_stale_unlisted_and_comment_replay() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    for field in ["choice", "data", "feedback", "revision", "artifact"] {
        let mut command = f.submit(&view, 0, field);
        match field {
            "choice" => command.choice = "unknown".into(),
            "data" => command.data = json!(12),
            "feedback" => command.choice = "revise".into(),
            "revision" => command.expected_revision += 1,
            _ => command.expected_artifact = "0".repeat(64),
        }
        assert!(
            f.persistence().submit_decision(command).await.is_err(),
            "{field}"
        );
    }
    let mut command = f.submit(&view, 0, "unlisted");
    command.actor = f.admission.binding.target.actor;
    assert!(matches!(
        f.persistence().submit_decision(command).await,
        Err(AppError::NotFound(_))
    ));
    let comment = AddDecisionComment {
        company: f.scope.company,
        actor: f.admission.binding.target.actor,
        decision: view.decision,
        command_key: DecisionCommandKey::parse("comment").unwrap(),
        body: "internal discussion".into(),
    };
    assert_eq!(
        f.persistence()
            .add_decision_comment(comment.clone())
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Applied
    );
    assert_eq!(
        f.persistence()
            .add_decision_comment(comment.clone())
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Replayed
    );
    let open = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(open.revision, 1);
    assert_eq!(open.comments.len(), 1);
    f.persistence()
        .submit_decision(f.submit(&view, 0, "valid"))
        .await
        .unwrap();
    let mut late = comment;
    late.command_key = DecisionCommandKey::parse("late").unwrap();
    assert!(matches!(
        f.persistence().add_decision_comment(late).await,
        Err(AppError::Conflict(_))
    ));
}

#[tokio::test]
async fn workflow_decision_park_composition_error_fails_run_unrouted() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices.clone().unrenderable())
            .await
            .unwrap()
            .is_none()
    );
    assert_failed_without_park(&f).await;
}

#[tokio::test]
async fn workflow_decision_park_unprovable_notice_key_fails_run_unrouted() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    sqlx::query("UPDATE users SET email=$2 WHERE id=$1")
        .bind(f.users[0])
        .bind(format!("{}@example.test", "a".repeat(400)))
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices)
            .await
            .unwrap()
            .is_none()
    );
    assert_failed_without_park(&f).await;
}

async fn assert_failed_without_park(f: &DecisionFixture) {
    let counts = f.counts().await;
    assert_eq!(counts["run"], "failed");
    assert_eq!(counts["job"], "failed");
    for table in ["waits", "decisions", "reviewers", "deliveries"] {
        assert_eq!(counts[table], 0);
    }
    let events: Vec<String> =
        sqlx::query_scalar("SELECT event_kind FROM workflow_run_events WHERE run_id=$1")
            .bind(f.scope.run.as_uuid())
            .fetch_all(f.persistence().pool())
            .await
            .unwrap();
    assert!(
        events
            .iter()
            .any(|event| event == "decision.reviewer_unavailable")
    );
    assert!(
        f.persistence()
            .park_decision(f.scope, &f.notices)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn workflow_decision_park_with_sixteen_reviewers() {
    let f = Box::pin(DecisionFixture::new(16, None)).await;
    assert_eq!(f.park().await.reviewers.len(), 16);
    let counts = f.counts().await;
    assert_eq!(counts["reviewers"], 16);
    assert_eq!(counts["deliveries"], 16);
}

async fn make_due(f: &DecisionFixture) {
    sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'")
        .bind(f.scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
}

#[tokio::test]
async fn workflow_decision_timeout_without_error_route_fails_run() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    assert_eq!(
        f.persistence().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Pending
    );
    make_due(&f).await;
    assert!(matches!(
        f.persistence().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Expired(_)
    ));
    let counts = f.counts().await;
    assert_eq!(counts["run"], "failed");
    assert_eq!(counts["executions"], 1);
    assert!(matches!(
        f.persistence()
            .submit_decision(f.submit(&view, 0, "late"))
            .await,
        Err(AppError::Conflict(_))
    ));
    let terminal: Uuid =
        sqlx::query_scalar("SELECT terminal_execution_id FROM workflow_runs WHERE id=$1")
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(terminal, f.scope.execution.as_uuid());
    let timed_out: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_run_events WHERE run_id=$1 AND event_kind='decision.timed_out')").bind(f.scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert!(timed_out);
}

#[tokio::test]
async fn workflow_decision_timeout_error_route_successor_receives_null() {
    let f = Box::pin(DecisionFixture::new(1, Some("finish"))).await;
    f.park().await;
    make_due(&f).await;
    assert!(matches!(
        f.persistence().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Expired(_)
    ));
    assert_eq!(f.counts().await["executions"], 2);
    let output: Value =
        sqlx::query_scalar("SELECT committed_output FROM workflow_executions WHERE id=$1")
            .bind(f.scope.execution.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(output, Value::Null);
    assert!(matches!(
        f.persistence().resume_wait(f.scope).await.unwrap(),
        WaitProgress::Expired(
            crate::application::workflow::completion::CommitDisposition::Replayed
        )
    ));
    assert_eq!(f.counts().await["executions"], 2);
}

#[tokio::test]
async fn workflow_decision_submit_competes_with_timeout() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let barrier = Barrier::new(2);
    let submit = async {
        barrier.wait().await;
        f.persistence()
            .submit_decision(f.submit(&view, 0, "submit"))
            .await
    };
    let timeout = async {
        barrier.wait().await;
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM workflow_runs WHERE id=$1 FOR UPDATE")
            .bind(f.scope.run.as_uuid())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE workflow_waits SET created_at=clock_timestamp()-interval '2 seconds',deadline=clock_timestamp()-interval '1 second' WHERE run_id=$1 AND state='waiting'").bind(f.scope.run.as_uuid()).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        f.persistence().resume_wait(f.scope).await.unwrap()
    };
    let (submitted, _) = tokio::join!(submit, timeout);
    let counts = f.counts().await;
    if submitted.is_ok() {
        assert_eq!(counts["executions"], 2);
    } else {
        assert!(matches!(submitted, Err(AppError::Conflict(_))));
        assert_eq!(counts["run"], "failed");
        assert_eq!(counts["executions"], 1);
    }
}

#[tokio::test]
async fn workflow_decision_submit_competes_with_cancel() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let command = CancelCommand {
        company_id: f.scope.company,
        run_id: f.scope.run,
        actor: f.admission.binding.target.actor,
        command_key: IdempotencyKey::parse("cancel").unwrap(),
        expected_revision: f
            .persistence()
            .head(f.scope.company, f.scope.run)
            .await
            .unwrap()
            .unwrap()
            .revision,
    };
    let barrier = Barrier::new(2);
    let submit = async {
        barrier.wait().await;
        f.persistence()
            .submit_decision(f.submit(&view, 0, "submit"))
            .await
    };
    let cancel = async {
        barrier.wait().await;
        f.persistence().cancel(command).await.unwrap()
    };
    let (submitted, cancelled) = tokio::join!(submit, cancel);
    if matches!(cancelled, CancelResult::Applied { .. }) {
        assert!(matches!(submitted, Err(AppError::Conflict(_))));
        assert_eq!(f.counts().await["executions"], 1);
    } else {
        assert!(submitted.is_ok());
        assert!(matches!(cancelled, CancelResult::RevisionConflict { .. }));
        assert_eq!(f.counts().await["executions"], 2);
    }
}

#[tokio::test]
async fn workflow_decision_revoked_reviewer_and_foreign_company_are_hidden() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let mut foreign = f.submit(&view, 0, "foreign");
    foreign.company = CompanyId::new(Uuid::new_v4());
    assert!(matches!(
        f.persistence().submit_decision(foreign).await,
        Err(AppError::NotFound(_))
    ));
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.users[0])
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(matches!(
        f.persistence()
            .submit_decision(f.submit(&view, 0, "demoted"))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await["run"], "waiting");
}

#[path = "decision_sql_tests.rs"]
mod sql_tests;

#[path = "decision_lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "decision_authority_tests.rs"]
mod authority_tests;

#[path = "decision_boundary_tests.rs"]
mod boundary_tests;

#[path = "decision_provenance_tests.rs"]
mod provenance_tests;

async fn reviewer_rows(
    f: &DecisionFixture,
    decision: DecisionId,
) -> Vec<(Uuid, Option<Uuid>, bool)> {
    sqlx::query_as("SELECT user_id, notice_delivery_id, notice_opted_out FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 ORDER BY ordinal")
        .bind(f.scope.company.as_uuid()).bind(decision.0).fetch_all(f.persistence().pool()).await.unwrap()
}

fn check_violation(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> bool {
    result
        .err()
        .and_then(|error| {
            error
                .as_database_error()?
                .code()
                .map(|code| code == "23514")
        })
        .unwrap_or(false)
}

/// P08-Q8: an opted-out reviewer gets no notice, a reviewer row that records the opt-out, and can
/// still submit; the other reviewer still gets theirs. The guard refuses an opt-out the
/// preference does not back, and a row with neither a notice nor an opt-out.
#[tokio::test]
async fn workflow_decision_opted_out_reviewer_gets_no_notice_and_still_reviews() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let pool = f.persistence().pool();
    sqlx::query("INSERT INTO user_notification_preferences(user_id, decision_email_enabled) VALUES($1, false)")
        .bind(f.users[1]).execute(pool).await.unwrap();

    let view = f.park().await;
    assert_eq!(view.reviewers.len(), 2);
    let rows = reviewer_rows(&f, view.decision).await;
    assert_eq!(rows[0].0, f.users[0]);
    assert!(
        rows[0].1.is_some() && !rows[0].2,
        "the opted-in reviewer is noticed"
    );
    assert_eq!(rows[1].0, f.users[1]);
    assert!(
        rows[1].1.is_none() && rows[1].2,
        "the opted-out reviewer is recorded as such"
    );
    assert_eq!(f.counts().await["deliveries"], 1);

    // A person principal of the company who is not a reviewer and has no preference row.
    let other: Uuid = sqlx::query_scalar("SELECT principal.id FROM principals AS principal WHERE principal.company_id=$1 AND principal.kind='person' AND NOT EXISTS (SELECT 1 FROM workflow_decision_reviewers AS reviewer WHERE reviewer.decision_id=$2 AND reviewer.principal_id=principal.id) LIMIT 1")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).fetch_one(pool).await.unwrap();
    let forge = |opted_out: bool| {
        sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,ordinal,principal_id,notice_delivery_id,notice_opted_out) VALUES($1,$2,2,$3,NULL,$4)")
            .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(other).bind(opted_out)
            .execute(pool)
    };
    assert!(
        check_violation(forge(true).await),
        "an opt-out the preference does not back"
    );
    assert!(
        check_violation(forge(false).await),
        "neither a notice nor an opt-out"
    );

    let applied = f
        .persistence()
        .submit_decision(f.submit(&view, 1, "opted-out"))
        .await
        .unwrap();
    assert_eq!(applied.disposition, DecisionDisposition::Applied);
}

/// P08-Q8 under contention: a park reads the reviewer's preference `FOR SHARE`, so a toggle in
/// flight is waited for rather than read around, and the park records what it committed to.
#[tokio::test]
async fn workflow_decision_park_waits_for_a_preference_toggle_in_flight() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let pool = f.persistence().pool().clone();
    sqlx::query("INSERT INTO user_notification_preferences(user_id, decision_email_enabled) VALUES($1, false)")
        .bind(f.users[0]).execute(&pool).await.unwrap();
    // The toggle back on: its row locked and changed, not yet committed.
    let mut toggle = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE user_notification_preferences SET decision_email_enabled = true WHERE user_id = $1",
    )
    .bind(f.users[0])
    .execute(&mut *toggle)
    .await
    .unwrap();

    let watch_then_commit = async {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
        let mut waited = false;
        while tokio::time::Instant::now() < deadline {
            waited = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock' AND query LIKE '%FROM user_notification_preferences WHERE user_id=$1 FOR SHARE%')")
                .fetch_one(&pool).await.unwrap();
            if waited {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        toggle.commit().await.unwrap();
        waited
    };
    let (parked, waited) = tokio::join!(
        f.persistence().park_decision(f.scope, &f.notices),
        watch_then_commit
    );
    parked.unwrap().expect("the decision parks");
    assert!(waited, "the park waited on the toggle's row lock");

    let decision: Uuid = sqlx::query_scalar(
        "SELECT id FROM workflow_decisions WHERE company_id=$1 AND execution_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.execution.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    let rows = reviewer_rows(&f, DecisionId(decision)).await;
    assert!(
        rows[0].1.is_some() && !rows[0].2,
        "the park saw the committed opt-in and sent the notice"
    );
}
