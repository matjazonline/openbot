//! The run's controls, each the JSON route's own command: cancel, retry, rerun, and recording
//! what happened to an action (an annotation, never a resume). Every form carries the run
//! revision it was shown at and a fresh command key. A refusal that means the run moved answers
//! with the run as it is now, so the next press carries the new revision and a new key. A
//! request that fails outright (a 503) leaves the page and its key as they were, so pressing
//! again repeats the same request, which the command replays (WF-C005).
use super::super::super::bindings::{cancel_command, retry_command, start_command};
use super::super::super::reconciliations::{ReconcileBody, reconcile_command};
use super::*;
use crate::application::workflow::actions::ReconciliationOutcome;
use axum::body::Bytes;

/// The message a control's refusal shows on the pane; `None` is the request's own failure.
fn refusal(error: &WorkflowError) -> Option<String> {
    match error {
        WorkflowError::App(AppError::Conflict(message)) => Some(format!(
            "Nothing was done: {message}. This is the run as it is now."
        )),
        WorkflowError::App(AppError::BadRequest(message)) => {
            Some(format!("Nothing was done: {message}."))
        }
        WorkflowError::Diagnostic(diagnostic) => {
            Some(format!("Nothing was done: {}.", diagnostic.message))
        }
        _ => None,
    }
}

/// The pane after a control: what it did, or why it did nothing.
async fn answered<T>(
    ui: &Ui,
    company: Uuid,
    run: Uuid,
    outcome: WorkflowResult<T>,
    done: impl FnOnce(T) -> String,
) -> WorkflowResult<Html<String>> {
    let message = match outcome {
        Ok(value) => Ok(done(value)),
        Err(error) => match refusal(&error) {
            Some(message) => Err(message),
            None => return Err(error),
        },
    };
    let banner = match &message {
        Ok(message) => pages::RunBanner::Done(message),
        Err(message) => pages::RunBanner::Refused(message),
    };
    Ok(Html(ui.run_pane_html(company, run, Some(banner)).await?))
}

/// POST /ui/workflow-runs/{run}/cancel — cancel a top-level run at the revision seen.
pub(super) async fn cancel(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let outcome = cancel_command(
        &ui.engine,
        company,
        ui.actor()?,
        run,
        form.number("expected_revision")?,
        &form.text("command_key"),
    )
    .await;
    answered(&ui, company, run, outcome, |revision| {
        format!(
            "Cancelled (revision {revision}). Steps not yet started will not run; effects already accepted stay."
        )
    })
    .await
}

/// POST /ui/workflow-runs/{run}/retry — resume a failed run at its failed step, when safe.
pub(super) async fn retry(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let outcome = retry_command(
        &ui.engine,
        company,
        ui.actor()?,
        run,
        form.number("expected_revision")?,
        &form.text("command_key"),
    )
    .await;
    answered(&ui, company, run, outcome, |revision| {
        format!(
            "Retrying the failed step (revision {revision}). It is the same run, not a new one."
        )
    })
    .await
}

/// POST /ui/workflow-runs/{run}/rerun — a new run of the same binding with this run's input,
/// recorded as its rerun. The pane moves to the new run.
pub(super) async fn rerun(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let view = ui.visible_run(company, run).await?;
    let summary = &view.detail.summary;
    if summary.parent.is_some() {
        return Ok(answered(
            &ui,
            company,
            run,
            Err::<(), _>(WorkflowError::App(AppError::Conflict(
                "a child run is rerun through its parent".into(),
            ))),
            |_| String::new(),
        )
        .await?
        .into_response());
    }
    let started = start_command(
        &ui.engine,
        company,
        ui.actor()?,
        summary.binding.as_uuid(),
        Some(summary.association),
        view.input.clone(),
        form.text("command_key"),
        Some(run),
    )
    .await;
    match started {
        Ok((new_run, replayed)) => {
            let new_run = new_run.as_uuid();
            let message = format!(
                "{} run {} as a rerun of {}: a new run with its own history.",
                if replayed {
                    "Already started"
                } else {
                    "Started"
                },
                pages::short_id(new_run),
                pages::short_id(run),
            );
            let html = ui
                .run_pane_html(company, new_run, Some(pages::RunBanner::Done(&message)))
                .await?;
            Ok(selected(pushed(
                html,
                &format!("/ui/workflow-runs?company_id={company}&run_id={new_run}"),
            )))
        }
        Err(error) => Ok(
            answered(&ui, company, run, Err::<(), _>(error), |_| String::new())
                .await?
                .into_response(),
        ),
    }
}

/// POST /ui/workflow-runs/{run}/reconcile — record what happened to one action as an unknown
/// note. An annotation: the run never resumes from it; recovery is cancel then rerun.
pub(super) async fn reconcile(
    ui: Ui,
    Path(run): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let target = match form.get("target") {
        Some("message_delivery") => serde_json::json!({
            "kind": "message_delivery",
            "ordinal": form.number("ordinal")?,
            "delivery_id": form.text("delivery_id"),
        }),
        _ => serde_json::json!({"kind": "remote"}),
    };
    let body: ReconcileBody = serde_json::from_value(serde_json::json!({
        "execution_id": form.text("execution_id"),
        "invocation_id": form.text("invocation_id"),
        "argument_digest": form.text("argument_digest"),
        "marker_id": form.text("marker_id"),
        "target": target,
        "expected_revision": form.number("expected_revision")?,
        "command_key": form.text("command_key"),
        "evidence": {"kind": "unknown_note", "note": form.text("note"), "claimed": "unknown"},
    }))
    .map_err(|_| AppError::BadRequest("The reconciliation form is incomplete".into()))?;
    let outcome = reconcile_command(&ui.engine, company, ui.actor()?, run, body).await;
    let (banner, done) = match outcome {
        Ok(result) => match result.outcome {
            ReconciliationOutcome::UnknownRecorded => (
                "Recorded: what happened to this action is unknown. The run does not resume from \
                 this; to recover, cancel it and rerun."
                    .to_owned(),
                true,
            ),
            ReconciliationOutcome::RevisionConflict => (
                "Nothing was recorded: the run changed since the page was shown. This is the run \
                 as it is now."
                    .to_owned(),
                false,
            ),
            // The same form sent again with other content: what it sent first may well have been
            // recorded, and is then in the timeline.
            ReconciliationOutcome::IdempotencyConflict => (
                "Nothing new was recorded: this form was already sent with a different note. If \
                 the first one was recorded, it is in the timeline; this is the run as it is now."
                    .to_owned(),
                false,
            ),
            ReconciliationOutcome::Blocked { reason } => (
                format!(
                    "Nothing was recorded: {}.",
                    serde_json::to_value(reason)
                        .ok()
                        .and_then(|value| value.as_str().map(|text| text.replace('_', " ")))
                        .unwrap_or_else(|| "the action cannot be reconciled now".into())
                ),
                false,
            ),
            ReconciliationOutcome::EvidenceConflict => (
                "Recorded, but it conflicts with other evidence for this action.".to_owned(),
                false,
            ),
            other => (
                format!(
                    "Recorded ({}).",
                    serde_json::to_value(&other)
                        .ok()
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                ),
                true,
            ),
        },
        Err(error) => match refusal(&error) {
            Some(message) => (message, false),
            None => return Err(error),
        },
    };
    let banner = if done {
        pages::RunBanner::Done(&banner)
    } else {
        pages::RunBanner::Refused(&banner)
    };
    Ok(Html(ui.run_pane_html(company, run, Some(banner)).await?))
}
