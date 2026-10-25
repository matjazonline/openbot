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

    let shown = text(pane(ui(), Path(action.run), pane_query(action.company, None)).await)
        .await
        .unwrap();
    assert!(shown.contains("waiting on reconciliation"), "{shown}");
    assert!(
        shown.contains("Reconciliation needed for a remote action ("),
        "the timeline lists the run's reconciliation record: {shown}"
    );
    // A waiting run moves only through a change that notifies, so its body takes no tick; the
    // form, typed note included, and the unfinished step are kept across a reload in the same
    // epoch while the revision holds (F022).
    assert!(shown.contains(&format!(
        r#"hx-trigger="sse:run-{}, sse:resync""#,
        action.run
    )));
    assert!(!shown.contains("sse:tick"));
    assert!(shown.contains("Record what happened (an annotation, not a resume)"));
    let epoch = field_value(&shown, "epoch");
    let revision = field_value(&shown, "expected_revision");
    let form_id = format!("run-reconcile-{}-remote-{epoch}-r{revision}", action.marker);
    assert!(
        shown.contains(&format!(r#"<details id="{form_id}" hx-preserve="true""#)),
        "{shown}"
    );
    assert!(shown.contains(&format!(
        r#"id="run-step-{}-r{revision}" hx-preserve="true""#,
        action.execution
    )));
    let redrawn = text(
        pane(
            ui(),
            Path(action.run),
            Query(PaneQuery {
                company_id: action.company,
                part: Some(PanePart::Body),
                epoch: Some(epoch.parse().unwrap()),
            }),
        )
        .await,
    )
    .await
    .unwrap();
    assert!(
        redrawn.contains(&form_id),
        "the same form, so the page keeps it"
    );
    assert_eq!(
        field_value(&redrawn, "command_key"),
        field_value(&shown, "command_key")
    );
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
    let reloaded = text(
        pane(
            ui(),
            Path(action.run),
            pane_query(action.company, Some(PanePart::Body)),
        )
        .await,
    )
    .await
    .unwrap();
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
    // The same form sent with another note after a lost answer: nothing new, and the answer says
    // the first note may well be the one recorded (F024).
    let mut edited = submitted.clone();
    edited.retain(|(name, _)| *name != "note");
    edited.push(("note", "Second thoughts."));
    let conflicted = reconcile(ui(), Path(action.run), query(), form(&edited))
        .await
        .unwrap()
        .0;
    assert!(
        conflicted
            .contains("Nothing new was recorded: this form was already sent with a different note"),
        "{conflicted}"
    );
    assert_eq!(evidence().await, 1);
    // The answer drew a new epoch, so the next form carries a fresh key.
    assert_ne!(
        field_value(&conflicted, "command_key"),
        field_value(&shown, "command_key")
    );
    // A plain member of the company reaches neither the run nor its annotation.
    let member = teammate(&action.persistence, action.company, "member").await;
    let refused = reconcile(
        ui_over(&action.persistence, &engine, member),
        Path(action.run),
        query(),
        form(&edited),
    )
    .await;
    assert!(
        matches!(refused, Err(WorkflowError::App(AppError::NotFound(_)))),
        "{refused:?}"
    );
    assert_eq!(evidence().await, 1);
}

/// The command keys a page draws (F023): a wake-up redraw in the same epoch repeats them, so a
/// press after a lost answer replays; a rerun that committed shows on the redrawn page, whose
/// key then asks for another one deliberately; another viewer or epoch draws other keys.
#[tokio::test]
async fn a_redraw_repeats_its_keys_until_the_command_shows_on_the_page() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Map", &example("data.map", workflow))
        .await;
    let binding = f.bound(version, RelatedAssociation::Company).await;
    let original = f.start(binding, json!({})).await;
    f.run_until_done(&[original]).await;
    let shown = f.pane(f.owner, original).await.unwrap();
    let epoch: Uuid = field_value(&shown, "epoch").parse().unwrap();
    let key = field_value(&shown, "command_key");
    let redraw = |user: Uuid, epoch: Uuid| {
        let ui = f.ui(user);
        let company = f.company;
        async move {
            text(
                pane(
                    ui,
                    Path(original),
                    Query(PaneQuery {
                        company_id: company,
                        part: Some(PanePart::Body),
                        epoch: Some(epoch),
                    }),
                )
                .await,
            )
            .await
            .unwrap()
        }
    };
    assert_eq!(
        field_value(&redraw(f.owner, epoch).await, "command_key"),
        key
    );
    assert_ne!(
        field_value(&redraw(f.owner, Uuid::new_v4()).await, "command_key"),
        key
    );
    assert_ne!(
        field_value(&redraw(f.admin, epoch).await, "command_key"),
        key
    );

    let press = || async {
        text(Ok(rerun(
            f.ui(f.owner),
            Path(original),
            f.query(),
            form(&[("command_key", &key)]),
        )
        .await
        .unwrap()))
        .await
        .unwrap()
    };
    let started = press().await;
    assert!(started.contains("Started run"), "{started}");
    // The answer was lost: the same form pressed again replays it.
    let replayed = press().await;
    assert!(replayed.contains("Already started run"), "{replayed}");
    let reruns: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE rerun_of_run_id = $1")
            .bind(original)
            .fetch_all(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(reruns.len(), 1);
    // A redraw after the rerun committed shows it, and asks for another one deliberately.
    let after = redraw(f.owner, epoch).await;
    assert!(after.contains("Rerun as") && after.contains(&pages::short_id(reruns[0])));
    assert!(after.contains("Rerun again"), "{after}");
    assert_ne!(field_value(&after, "command_key"), key);
}

/// An HTTP action the server accepted: the step lists its receipt, and the run's cancel says
/// that accepted effect stays.
#[tokio::test]
async fn an_accepted_action_shows_its_receipt_and_cancel_names_it() {
    use crate::adapters::persistence::workflow::reconciliation_fixture::accepted_http;
    let action = accepted_http().await;
    let engine: Engine = Arc::new(WorkflowEngine::for_tests(action.persistence.clone()));
    let ui = || ui_over(&action.persistence, &engine, action.owner);
    let shown = text(pane(ui(), Path(action.run), pane_query(action.company, None)).await)
        .await
        .unwrap();
    assert!(
        shown.contains("remote</span> action · attempt 1 · accepted"),
        "{shown}"
    );
    assert!(
        shown.contains(
            "1 action of this run was already accepted (listed under Actions); cancelling does not undo it."
        ),
        "{shown}"
    );
    let body = step(
        ui(),
        Path((action.run, action.execution)),
        Query(CompanyQuery {
            company_id: action.company,
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(
        body.contains("Action receipts") && body.contains("remote action accepted"),
        "{body}"
    );
    assert!(
        body.contains("&quot;ok&quot;: true") || body.contains("\"ok\": true"),
        "{body}"
    );
}
