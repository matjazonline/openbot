//! Action reconciliation through the route and the production composition (P08-D3 rev 2): an
//! annotation of an HTTP action whose outcome is unknown, in both published forms; the outcome to
//! status table and the fresh-key rule; readiness and the replay that needs it (F068, F071);
//! tenancy; discovery; and competing claimants.
use super::*;
use crate::adapters::persistence::workflow::reconciliation_fixture::{
    HttpForm, UncertainHttp, uncertain_http,
};
use crate::application::workflow::WorkflowActor;
use crate::application::workflow::http::HttpConnections;
use crate::domain::workflow::CompanyId;

struct Reconciling {
    action: UncertainHttp,
    engine: Engine,
    member: Uuid,
    /// The owner of `other`, and nothing of the action's company.
    stranger: Uuid,
    other: Uuid,
}

impl Reconciling {
    async fn new(form: HttpForm) -> Self {
        let action = uncertain_http(form).await;
        let p = &action.persistence;
        let member = teammate(p, action.company, "member").await;
        let (stranger, other) = owner_and_company(p, "Elsewhere").await;
        Self {
            engine: Arc::new(WorkflowEngine::for_tests(p.clone())),
            action,
            member,
            stranger,
            other,
        }
    }

    async fn detail(&self) -> Value {
        let a = &self.action;
        let (status, detail) = answer(
            bindings::run(
                State(self.engine.clone()),
                by(a.owner),
                Path((a.company, a.run)),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        detail
    }

    async fn revision(&self) -> u64 {
        self.detail().await["revision"].as_u64().unwrap()
    }

    fn note(&self, key: &str, revision: u64, note: &str) -> Value {
        self.request(
            key,
            revision,
            json!({"kind": "unknown_note", "note": note, "claimed": "unknown"}),
        )
    }

    fn request(&self, key: &str, revision: u64, evidence: Value) -> Value {
        let a = &self.action;
        json!({"execution_id": a.execution, "invocation_id": a.invocation,
            "argument_digest": a.argument_digest, "marker_id": a.marker,
            "target": {"kind": "remote"}, "expected_revision": revision, "command_key": key,
            "evidence": evidence})
    }

    async fn post_as(&self, user: Uuid, company: Uuid, request: Value) -> (StatusCode, Value) {
        answer(
            reconciliations::reconcile(
                State(self.engine.clone()),
                by(user),
                Path((company, self.action.run)),
                body(request),
            )
            .await,
        )
        .await
    }

    async fn post(&self, request: Value) -> (StatusCode, Value) {
        self.post_as(self.action.owner, self.action.company, request)
            .await
    }

    async fn evidence(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM workflow_action_evidence WHERE run_id = $1")
            .bind(self.action.run)
            .fetch_one(self.action.persistence.pool())
            .await
            .unwrap()
    }

    async fn run_state(&self) -> (String, Option<String>) {
        sqlx::query_as("SELECT state, waiting_reason FROM workflow_runs WHERE id = $1")
            .bind(self.action.run)
            .fetch_one(self.action.persistence.pool())
            .await
            .unwrap()
    }

    /// Switch the action's connection on or off, by its owner.
    async fn set_enabled(&self, enabled: bool) {
        let (company, actor) = (
            CompanyId::new(self.action.company),
            WorkflowActor::authenticated(self.action.owner).unwrap(),
        );
        let p = &self.action.persistence;
        let revision: i64 = sqlx::query_scalar(
            "SELECT revision FROM workflow_http_connections WHERE company_id = $1 AND id = $2",
        )
        .bind(self.action.company)
        .bind(self.action.connection)
        .fetch_one(p.pool())
        .await
        .unwrap();
        p.set_http_enabled(company, actor, self.action.connection, revision, enabled)
            .await
            .unwrap();
    }
}

fn outcome(answer: &Value) -> &str {
    answer["outcome"]["kind"].as_str().unwrap()
}

/// F065, D3.4: both HTTP forms are discovered as reconcilable remote markers and annotated
/// through the production resources; the note records once, replays, and leaves the run waiting.
#[tokio::test]
async fn reconciliation_annotates_both_http_forms() {
    for form in [HttpForm::Slot, HttpForm::Tool] {
        let r = Reconciling::new(form).await;
        let detail = r.detail().await;
        let actions = detail["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 1, "{form:?}: {detail}");
        let action = &actions[0];
        assert_eq!(action["marker_id"], json!(r.action.marker));
        assert_eq!(action["effect_kind"], "remote");
        assert_eq!(
            (
                action["has_receipt"].clone(),
                action["reconcilable"].clone()
            ),
            (json!(false), json!(true)),
            "{action}"
        );
        assert_eq!(detail["actions_truncated"], false);
        let revision = r.revision().await;
        let request = r.note("n1", revision, "the provider timed out; nothing visible");
        let (status, recorded) = r.post(request.clone()).await;
        assert_eq!(status, StatusCode::OK, "{form:?}: {recorded}");
        assert_eq!(outcome(&recorded), "unknown_recorded");
        assert_eq!(recorded["replayed"], false);
        assert!(recorded["evidence_id"].is_string(), "{recorded}");
        assert_eq!(recorded["revision"], json!(revision + 1));
        assert_eq!(r.evidence().await, 1);
        assert_eq!(
            r.run_state().await,
            ("waiting".to_owned(), Some("reconciliation".to_owned()))
        );
        let (status, replayed) = r.post(request).await;
        assert_eq!(status, StatusCode::OK, "{replayed}");
        assert_eq!(
            (outcome(&replayed), replayed["replayed"].clone()),
            ("unknown_recorded", json!(true))
        );
        assert_eq!(r.evidence().await, 1);
    }
}

/// F066: the outcome to status table and the fresh-key rule, with exact outcomes and rows.
#[tokio::test]
async fn reconciliation_outcomes_follow_the_status_table() {
    let r = Reconciling::new(HttpForm::Slot).await;
    let read = r.revision().await;
    let (status, refused) = r.post(r.note("stale", read + 7, "late")).await;
    assert_eq!(
        (status, outcome(&refused)),
        (StatusCode::CONFLICT, "revision_conflict"),
        "{refused}"
    );
    assert_eq!(r.evidence().await, 0);
    // The refusal is recorded under its key: the corrected revision needs a fresh key.
    let (status, reused) = r.post(r.note("stale", read, "late")).await;
    assert_eq!(
        (status, outcome(&reused)),
        (StatusCode::CONFLICT, "idempotency_conflict"),
        "{reused}"
    );
    let (status, recorded) = r.post(r.note("fresh", read, "late")).await;
    assert_eq!(
        (status, outcome(&recorded)),
        (StatusCode::OK, "unknown_recorded"),
        "{recorded}"
    );
    let current = r.revision().await;
    let (status, other) = r.post(r.note("fresh", current, "another note")).await;
    assert_eq!(
        (status, outcome(&other)),
        (StatusCode::CONFLICT, "idempotency_conflict"),
        "{other}"
    );
    // No production verifier: a verified reference at the current revision records nothing.
    let verified = json!({"kind": "verified_reference", "registration": "fixture.http",
        "reference": "provider-record-1"});
    let (status, invalid) = r
        .post(r.request("verified", current, verified.clone()))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{invalid}");
    assert_eq!(r.evidence().await, 1);
    let (status, stale) = r
        .post(r.request("verified-stale", current + 3, verified))
        .await;
    assert_eq!(
        (status, outcome(&stale)),
        (StatusCode::CONFLICT, "revision_conflict"),
        "{stale}"
    );
    assert_eq!(r.evidence().await, 1);
}

/// F068, F071: an unready resource refuses annotation with 404 and records nothing, a replay
/// included; once it is ready again the annotation records.
#[tokio::test]
async fn reconciliation_needs_a_ready_resource_even_to_replay() {
    let r = Reconciling::new(HttpForm::Slot).await;
    let first = r.note("first", r.revision().await, "unknown");
    assert_eq!(r.post(first.clone()).await.0, StatusCode::OK);
    r.set_enabled(false).await;
    let (status, replay) = r.post(first).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{replay}");
    let second = r.note("second", r.revision().await, "still unknown");
    assert_eq!(r.post(second.clone()).await.0, StatusCode::NOT_FOUND);
    assert_eq!(r.evidence().await, 1);
    r.set_enabled(true).await;
    let (status, recorded) = r.post(second).await;
    assert_eq!(
        (status, outcome(&recorded)),
        (StatusCode::OK, "unknown_recorded"),
        "{recorded}"
    );
    assert_eq!(r.evidence().await, 2);
}

/// Tenancy: a stranger on either company path and a plain member are refused, and so is a marker
/// that is not this run's; nothing is recorded.
#[tokio::test]
async fn reconciliation_refuses_strangers_members_and_foreign_markers() {
    let r = Reconciling::new(HttpForm::Slot).await;
    let request = r.note("k", r.revision().await, "not mine");
    for (user, company) in [
        (r.stranger, r.action.company),
        (r.stranger, r.other),
        (r.member, r.action.company),
    ] {
        assert_eq!(
            r.post_as(user, company, request.clone()).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let mut foreign = request.clone();
    foreign["marker_id"] = json!(Uuid::new_v4());
    // An unknown marker is missing inside the snapshot, so it reads like any other absent target.
    let (status, refused) = r.post(foreign).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");
    assert_eq!(r.evidence().await, 0);
}

/// Competing claimants from one read revision: with different keys exactly one note records and
/// the other is refused; with the same key one row is recorded and the other answer replays it.
#[tokio::test]
async fn competing_reconciliations_record_once() {
    let r = Reconciling::new(HttpForm::Slot).await;
    let read = r.revision().await;
    let (a, b) = tokio::join!(
        r.post(r.note("a", read, "first view")),
        r.post(r.note("b", read, "second view"))
    );
    let mut outcomes = [outcome(&a.1).to_owned(), outcome(&b.1).to_owned()];
    outcomes.sort();
    assert!(
        outcomes == ["revision_conflict", "unknown_recorded"]
            || outcomes == ["blocked", "unknown_recorded"],
        "{a:?} {b:?}"
    );
    for (status, answer) in [&a, &b] {
        if outcome(answer) == "blocked" {
            assert_eq!(answer["outcome"]["reason"], "stale_snapshot", "{answer}");
            assert_eq!(*status, StatusCode::CONFLICT);
        }
    }
    assert_eq!(r.evidence().await, 1);
    assert_eq!(r.revision().await, read + 1);

    let read = r.revision().await;
    let same = r.note("same", read, "one view");
    let (c, d) = tokio::join!(r.post(same.clone()), r.post(same));
    assert_eq!((c.0, d.0), (StatusCode::OK, StatusCode::OK), "{c:?} {d:?}");
    assert_eq!(
        (outcome(&c.1), outcome(&d.1)),
        ("unknown_recorded", "unknown_recorded")
    );
    let replays = [&c.1, &d.1]
        .iter()
        .filter(|answer| answer["replayed"] == true)
        .count();
    assert_eq!(replays, 1, "{c:?} {d:?}");
    assert_eq!(r.evidence().await, 2);
    assert_eq!(r.revision().await, read + 1);
}

/// F070: a timeout under the reconciliation budget is retryable.
#[test]
fn workflow_timeouts_answer_service_unavailable() {
    let response = WorkflowError::App(AppError::Timeout("budget".into())).into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[axum::http::header::RETRY_AFTER], "1");
}
