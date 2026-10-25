//! Human decisions: read, submit and comment over the decision commands, which authorize the
//! reviewer (05-D2), and the page the review notices link to, which submits the same commands.
use super::*;
use crate::adapters::http::pages::{
    DecisionFormInputs, DecisionInputs, DecisionOutcome, workflow_decision_page,
    workflow_decision_section,
};
use crate::application::workflow::decisions::{
    AddDecisionComment, DecisionCommandKey, DecisionDisposition, DecisionId, DecisionView,
    SubmitDecision,
};
use axum::{
    Form,
    body::Bytes,
    extract::{Path, State},
    response::Html,
};
use serde::Deserialize;

fn view_json(view: &DecisionView) -> Value {
    json!({
        "subject":view.subject.as_str(),"data_schema":view.data_schema,"feedback_required":view.feedback_required,"can_submit":view.can_submit,
        "version_id":view.context.version.as_uuid(),
        "history":view.history.iter().map(|entry|json!({"decision_id":entry.decision.0,"run_id":entry.run.as_uuid(),"step":entry.step.as_str(),"round":entry.round,"artifact_digest":entry.artifact_digest,"choice":entry.choice,"feedback":entry.feedback,"state":entry.state})).collect::<Vec<_>>(),
        "history_truncated":view.history_truncated,
        "decision_id": view.decision.0,
        "run_id": view.run.as_uuid(),
        "execution_id": view.execution.as_uuid(),
        "step": view.step.as_str(),
        "revision": view.revision,
        "artifact_digest": view.artifact_digest,
        "proposal": view.proposal,
        "deadline": view.deadline,
        "state": view.state,
        "reviewers": view.reviewers,
        "choices": view.choices,
        "submission": view.submission.as_ref().map(|submission| json!({
            "submitted_by": submission.submitted_by,
            "submitted_at": submission.submitted_at,
            "choice": submission.choice,
            "feedback": submission.feedback,
            "data": submission.data,
        })),
        "comments": view.comments.iter().map(|comment| json!({
            "sequence": comment.sequence,
            "author_principal": comment.author_principal,
            "body": comment.body,
            "created_at": comment.created_at,
        })).collect::<Vec<_>>(),
    })
}

fn replayed(disposition: DecisionDisposition) -> bool {
    disposition == DecisionDisposition::Replayed
}

pub async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, decision)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let view = engine
        .decisions()
        .read_decision(company(company_id), actor(&user)?, DecisionId(decision))
        .await?;
    Ok(ok(view_json(&view)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitBody {
    pub command_key: String,
    pub expected_revision: i64,
    pub expected_artifact: String,
    pub choice: String,
    #[serde(default)]
    pub feedback: String,
    pub data: Value,
}

async fn submit_command(
    engine: &WorkflowEngine,
    company_id: Uuid,
    user: &AuthenticatedUser,
    decision: Uuid,
    body: SubmitBody,
) -> AppResult<DecisionDisposition> {
    let submitted = engine
        .decisions()
        .submit_decision(SubmitDecision {
            company: company(company_id),
            actor: actor(user)?,
            decision: DecisionId(decision),
            command_key: DecisionCommandKey::parse(body.command_key)?,
            expected_revision: body.expected_revision,
            expected_artifact: body.expected_artifact,
            choice: body.choice,
            feedback: body.feedback,
            data: body.data,
        })
        .await?;
    // The continuation committed with the submission; a worker runs it.
    if submitted.successor.is_some() {
        engine.wake_workers();
    }
    Ok(submitted.disposition)
}

pub async fn submit(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, decision)): Path<(Uuid, Uuid)>,
    Json(body): Json<SubmitBody>,
) -> WorkflowResult<Json<Value>> {
    let disposition = submit_command(&engine, company_id, &user, decision, body).await?;
    Ok(ok(json!({"replayed": replayed(disposition)})))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentBody {
    pub command_key: String,
    pub body: String,
}

async fn comment_command(
    engine: &WorkflowEngine,
    company_id: Uuid,
    user: &AuthenticatedUser,
    decision: Uuid,
    body: CommentBody,
) -> AppResult<(DecisionDisposition, i64)> {
    let comment = engine
        .decisions()
        .add_decision_comment(AddDecisionComment {
            company: company(company_id),
            actor: actor(user)?,
            decision: DecisionId(decision),
            command_key: DecisionCommandKey::parse(body.command_key)?,
            body: body.body,
        })
        .await?;
    Ok((comment.disposition, comment.sequence))
}

pub async fn comment(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, decision)): Path<(Uuid, Uuid)>,
    Json(body): Json<CommentBody>,
) -> WorkflowResult<(StatusCode, Json<Value>)> {
    let (disposition, sequence) =
        comment_command(&engine, company_id, &user, decision, body).await?;
    let status = if replayed(disposition) {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        ok(json!({"sequence": sequence, "replayed": replayed(disposition)})),
    ))
}

/// The decision's company, from the decision row (the notice links name only the decision). The
/// decision read then authorizes the viewer, so an unknown or foreign decision is not found.
async fn page_company(engine: &WorkflowEngine, decision: Uuid) -> AppResult<Uuid> {
    engine
        .reads()
        .decision_company(decision)
        .await?
        .map(|company| company.as_uuid())
        .ok_or_else(|| AppError::NotFound("Decision not found".into()))
}

pub async fn page(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(decision): Path<Uuid>,
) -> AppResult<Html<String>> {
    let company_id = page_company(&engine, decision).await?;
    let view = engine
        .decisions()
        .read_decision(company(company_id), actor(&user)?, DecisionId(decision))
        .await?;
    Ok(Html(workflow_decision_page(&view)))
}

#[derive(Debug, Deserialize)]
pub struct CommentForm {
    pub command_key: String,
    pub body: String,
}

/// Re-render the section after a form command. A refusal the reviewer can act on (invalid input,
/// a decision that changed, no access) is a banner over the current view; any other failure is
/// the route's error, which the page's error feedback reports.
async fn section_after(
    engine: &WorkflowEngine,
    company_id: Uuid,
    user: &AuthenticatedUser,
    decision: Uuid,
    result: AppResult<&str>,
    inputs: Option<&DecisionFormInputs>,
) -> AppResult<Html<String>> {
    // The banner: what the command did, or why it was refused.
    let banner: Result<&str, String> = match result {
        Ok(done) => Ok(done),
        Err(AppError::BadRequest(message) | AppError::Conflict(message)) => Err(message),
        Err(AppError::NotFound(_)) => Err("You cannot act on this decision".to_owned()),
        Err(error) => return Err(error),
    };
    let view = engine
        .decisions()
        .read_decision(company(company_id), actor(user)?, DecisionId(decision))
        .await?;
    let outcome = match &banner {
        Ok(done) => DecisionOutcome::Done(done),
        Err(refusal) => DecisionOutcome::Refused(refusal),
    };
    Ok(Html(workflow_decision_section(
        &view,
        Some(outcome),
        if banner.is_err() { inputs } else { None },
    )))
}

pub async fn ui_submit(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(decision): Path<Uuid>,
    body: Bytes,
) -> AppResult<Html<String>> {
    let company_id = page_company(&engine, decision).await?;
    let view = engine
        .decisions()
        .read_decision(company(company_id), actor(&user)?, DecisionId(decision))
        .await?;
    let fields = super::ui::FormFields::parse(&body);
    let mut inputs = DecisionInputs::default();
    for (name, value) in fields.pairs() {
        inputs
            .values
            .entry(name.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    let result = match submission_body(&view, &inputs) {
        Ok(body) => submit_command(&engine, company_id, &user, decision, body)
            .await
            .map(|_| "Decision submitted"),
        Err(error) => Err(error),
    };
    section_after(
        &engine,
        company_id,
        &user,
        decision,
        result,
        Some(&DecisionFormInputs::Submission(inputs)),
    )
    .await
}

fn submission_body(view: &DecisionView, inputs: &DecisionInputs) -> AppResult<SubmitBody> {
    use crate::application::workflow::authoring::{parameter_fields, parameters_from_fields};
    let text = |name: &str| inputs.get(name).unwrap_or("").to_owned();
    let data = if inputs.get("data_mode") == Some("fields") {
        let fields = parameter_fields(view.data_schema.as_ref())
            .ok_or_else(|| AppError::BadRequest("This decision requires JSON data".into()))?;
        parameters_from_fields(&fields, |name| {
            inputs.get(&format!("data:{name}")).map(str::to_owned)
        })
        .map_err(AppError::BadRequest)?
    } else {
        serde_json::from_str(if text("data").trim().is_empty() {
            "null"
        } else {
            inputs.get("data").unwrap_or("null")
        })
        .map_err(|_| AppError::BadRequest("The data is not valid JSON".into()))?
    };
    Ok(SubmitBody {
        command_key: text("command_key"),
        expected_revision: text("expected_revision")
            .parse()
            .map_err(|_| AppError::BadRequest("The decision revision is missing".into()))?,
        expected_artifact: text("expected_artifact"),
        choice: text("choice"),
        feedback: text("feedback"),
        data,
    })
}

pub async fn ui_comment(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(decision): Path<Uuid>,
    Form(form): Form<CommentForm>,
) -> AppResult<Html<String>> {
    let company_id = page_company(&engine, decision).await?;
    let inputs = DecisionFormInputs::Comment(DecisionInputs {
        values: [
            ("command_key".into(), form.command_key.clone()),
            ("body".into(), form.body.clone()),
        ]
        .into_iter()
        .collect(),
    });
    let result = comment_command(
        &engine,
        company_id,
        &user,
        decision,
        CommentBody {
            command_key: form.command_key,
            body: form.body,
        },
    )
    .await
    .map(|_| "Comment posted");
    section_after(&engine, company_id, &user, decision, result, Some(&inputs)).await
}
