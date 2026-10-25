//! The run controls through the workspace: each acts at the revision the page showed, says what
//! it did or why it did nothing, and the annotation of an uncertain action records a note without
//! resuming the run.
use super::*;

#[tokio::test]
async fn controls_act_at_the_revision_seen_and_say_what_they_did() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // A step that fails at run time: its output is not the integer its schema asks for.
    let workflow = Uuid::new_v4();
    let mut source = example("data.map", workflow);
    source["input_schema"] = json!({"type": "object", "properties": {"x": {}}, "required": ["x"]});
    source["steps"]["start"]["with"]["value"] = json!({"ref": "/input/x"});
    source["steps"]["start"]["with"]["output_schema"] = json!({"literal": {"type": "integer"}});
    let version = f.published(workflow, "Strict", &source).await;
    let binding = f.bound(version, RelatedAssociation::Company).await;

    // Cancel: a stale revision does nothing and shows the run as it is; the current one cancels.
    let queued = f.start(binding, json!({"x": 1})).await;
    let pane = f.pane(f.owner, queued).await.unwrap();
    assert!(
        pane.contains("Cancel run")
            && pane.contains("No effect of this run has been accepted yet.")
    );
    let key = field_value(&pane, "command_key");
    let (_, revision) = f.state(queued).await;
    let stale = cancel(
        f.ui(f.owner),
        Path(queued),
        f.query(),
        form(&[
            ("expected_revision", &(revision + 5).to_string()),
            ("command_key", &key),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(stale.contains("Nothing was done"), "{stale}");
    assert_eq!(f.state(queued).await.0, "queued");
    let fresh = field_value(&stale, "command_key");
    assert_ne!(fresh, key, "a fresh key after a refusal");
    let cancelled = cancel(
        f.ui(f.owner),
        Path(queued),
        f.query(),
        form(&[
            ("expected_revision", &revision.to_string()),
            ("command_key", &fresh),
        ]),
    )
    .await
    .unwrap()
    .0;
    assert!(cancelled.contains("Cancelled"), "{cancelled}");
    // The timeline records the control with its actor and revisions.
    assert!(
        cancelled.contains("cancel by ") && cancelled.contains(": applied (revision "),
        "{cancelled}"
    );
    assert!(cancelled.contains("effects already accepted stay"));
    assert_eq!(f.state(queued).await.0, "cancelled");

    // Retry: refused for a run that has not failed; applied to a failed one.
    let failing = f.start(binding, json!({"x": "not a number"})).await;
    f.run_until_done(&[failing]).await;
    let (state, revision) = f.state(failing).await;
    assert_eq!(state, "failed");
    let pane = f.pane(f.owner, failing).await.unwrap();
    assert!(pane.contains("Retry failed step") && pane.contains("not a new revision"));
    let retried = retry(
        f.ui(f.owner),
        Path(failing),
        f.query(),
        form(&[
            ("expected_revision", &revision.to_string()),
            ("command_key", "retry-1"),
        ]),
    )
    .await
    .unwrap()
    .0;
    // An output that breaks its schema fails the same way again, so the step is not safe to
    // repeat: the command refuses and says why (an applied retry is the JSON route's test).
    assert!(
        retried.contains("Nothing was done: This run cannot be retried"),
        "{retried}"
    );
    assert_eq!(f.state(failing).await.0, "failed");
    let refused = retry(
        f.ui(f.owner),
        Path(queued),
        f.query(),
        form(&[("expected_revision", "1"), ("command_key", "retry-2")]),
    )
    .await
    .unwrap()
    .0;
    assert!(refused.contains("Nothing was done"), "{refused}");
}

/// A leased HTTP step whose outcome is unknown (the reconciliation fixture's path): the pane
/// offers the annotation with the action's identities, the step opens to its failed worker
/// attempt, and the form records an unknown note through the shared command without resuming
/// the run. The same form again, with the page's key, records nothing more.
#[tokio::test]
async fn an_uncertain_action_is_annotated_from_the_pane_and_its_step_shows_the_attempt() {
    use crate::adapters::persistence::workflow::reconciliation_fixture::{
        HttpForm, uncertain_http,
    };
    let action = uncertain_http(HttpForm::Slot).await;
    let engine: Engine = Arc::new(WorkflowEngine::for_tests(action.persistence.clone()));
    let ui = || ui_over(&action.persistence, &engine, action.owner);
    let query = || {
        Query(CompanyQuery {
            company_id: action.company,
        })
    };

    let shown = pane(ui(), Path(action.run), pane_query(action.company, None))
        .await
        .unwrap()
        .0;
    assert!(shown.contains("waiting on reconciliation"), "{shown}");
    assert!(
        shown.contains("Reconciliation needed for a remote action ("),
        "the timeline lists the run's reconciliation record: {shown}"
    );
    assert!(shown.contains(&format!(
        r#"hx-trigger="sse:run-{}, sse:resync, sse:tick""#,
        action.run
    )));
    assert!(shown.contains("Record what happened (an annotation, not a resume)"));
    assert_eq!(field_value(&shown, "marker_id"), action.marker.to_string());
    assert_eq!(
        field_value(&shown, "invocation_id"),
        action.invocation.to_string()
    );
    assert_eq!(
        field_value(&shown, "argument_digest"),
        action.argument_digest
    );
    assert_eq!(field_value(&shown, "target"), "remote");

    let body = step(ui(), Path((action.run, action.execution)), query())
        .await
        .unwrap()
        .0;
    assert!(body.contains("Diagnostics: worker attempts"), "{body}");
    assert!(body.contains("<td>failed</td>"), "{body}");

    let fields = [
        "execution_id",
        "invocation_id",
        "argument_digest",
        "marker_id",
        "target",
        "expected_revision",
        "command_key",
    ]
    .map(|name| (name, field_value(&shown, name)));
    let mut submitted: Vec<(&str, &str)> = fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    submitted.push(("note", "The vendor's dashboard shows nothing yet."));
    let recorded = reconcile(ui(), Path(action.run), query(), form(&submitted))
        .await
        .unwrap()
        .0;
    // The answer sits above the body, which is all a wake-up reloads.
    let body_at = recorded.find(r#"<div id="run-body""#).unwrap();
    assert!(
        recorded[..body_at].contains("Recorded: what happened to this action is unknown"),
        "{recorded}"
    );
    let reloaded = pane(
        ui(),
        Path(action.run),
        pane_query(action.company, Some(PanePart::Body)),
    )
    .await
    .unwrap()
    .0;
    assert!(!reloaded.contains("Recorded:"), "{reloaded}");
    let evidence = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM workflow_action_evidence WHERE run_id = $1",
        )
        .bind(action.run)
        .fetch_one(action.persistence.pool())
        .await
        .unwrap()
    };
    assert_eq!(evidence().await, 1);
    let (state, reason): (String, Option<String>) =
        sqlx::query_as("SELECT state, waiting_reason FROM workflow_runs WHERE id = $1")
            .bind(action.run)
            .fetch_one(action.persistence.pool())
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_deref()),
        ("waiting", Some("reconciliation"))
    );

    // Pressing the same form again replays the command: nothing new is recorded.
    let again = reconcile(ui(), Path(action.run), query(), form(&submitted))
        .await
        .unwrap()
        .0;
    assert!(again.contains("Recorded"), "{again}");
    assert_eq!(evidence().await, 1);
}
