//! Browser and API commands share the same administrative decision authority and receipts.
use super::*;
use crate::adapters::http::pages::{workflow_escalation_page, workflow_escalation_section};
use crate::application::workflow::RunRevision;
use crate::application::workflow::decisions::DecisionCommandKey;
use crate::application::workflow::escalation::{
    EscalationChoice, EscalationDecisionId, EscalationSubmission, SubmitEscalation,
};
use axum::{
    Form,
    extract::{Path, State},
    response::Html,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitBody {
    pub command_key: String,
    pub choice: EscalationChoice,
}

pub async fn read(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, decision)): Path<(Uuid, Uuid)>,
) -> WorkflowResult<Json<Value>> {
    let view = engine
        .escalations()
        .read_escalation(
            company(company_id),
            actor(&user)?,
            EscalationDecisionId::new(decision),
        )
        .await?;
    Ok(ok(
        serde_json::to_value(view).map_err(|e| AppError::Internal(e.to_string()))?
    ))
}

async fn apply(
    engine: &WorkflowServices,
    user: &AuthenticatedUser,
    company_id: Uuid,
    decision: Uuid,
    body: SubmitBody,
) -> AppResult<EscalationSubmission> {
    let result = engine
        .escalations()
        .submit_escalation(SubmitEscalation {
            company: company(company_id),
            actor: actor(user)?,
            decision: EscalationDecisionId::new(decision),
            command: DecisionCommandKey::parse(body.command_key)?,
            choice: body.choice,
        })
        .await?;
    engine.wake_workers();
    Ok(result)
}

pub async fn submit(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path((company_id, decision)): Path<(Uuid, Uuid)>,
    Json(body): Json<SubmitBody>,
) -> WorkflowResult<Json<Value>> {
    let result = apply(&engine, &user, company_id, decision, body).await?;
    Ok(ok(
        json!({"replayed":result==EscalationSubmission::Replayed}),
    ))
}

async fn page_company(engine: &WorkflowServices, decision: Uuid) -> AppResult<Uuid> {
    engine
        .reads()
        .decision_company(decision)
        .await?
        .map(|c| c.as_uuid())
        .ok_or_else(|| AppError::NotFound("Decision not found".into()))
}

pub async fn page(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(decision): Path<Uuid>,
) -> AppResult<Html<String>> {
    let company_id = page_company(&engine, decision).await?;
    let view = engine
        .escalations()
        .read_escalation(
            company(company_id),
            actor(&user)?,
            EscalationDecisionId::new(decision),
        )
        .await?;
    Ok(Html(workflow_escalation_page(&view)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitForm {
    pub command_key: String,
    pub choice: String,
    pub principal: Option<Uuid>,
    pub expected_run_revision: Option<u64>,
}

impl SubmitForm {
    fn body(self) -> AppResult<SubmitBody> {
        let choice = match self.choice.as_str() {
            "acknowledge" => EscalationChoice::Acknowledge,
            "reassign" => EscalationChoice::Reassign {
                principal: crate::entities::transport::PrincipalId::new(
                    self.principal
                        .ok_or_else(|| AppError::BadRequest("Choose a principal".into()))?,
                ),
            },
            "cancel" => EscalationChoice::Cancel {
                expected_run_revision: RunRevision(
                    self.expected_run_revision
                        .ok_or_else(|| AppError::BadRequest("Run revision is required".into()))?,
                ),
            },
            _ => return Err(AppError::BadRequest("Unknown escalation choice".into())),
        };
        Ok(SubmitBody {
            command_key: self.command_key,
            choice,
        })
    }
}

pub async fn ui_submit(
    State(engine): State<Engine>,
    user: AuthenticatedUser,
    Path(decision): Path<Uuid>,
    Form(form): Form<SubmitForm>,
) -> AppResult<Html<String>> {
    let company_id = page_company(&engine, decision).await?;
    let result = match form.body() {
        Ok(body) => apply(&engine, &user, company_id, decision, body).await,
        Err(error) => Err(error),
    };
    let message: Result<&str, String> = match result {
        Ok(_) => Ok("Escalation decision saved"),
        Err(AppError::BadRequest(text) | AppError::Conflict(text)) => Err(text),
        Err(AppError::NotFound(_) | AppError::Forbidden(_)) => {
            Err("You cannot act on this escalation".into())
        }
        Err(error) => return Err(error),
    };
    let view = engine
        .escalations()
        .read_escalation(
            company(company_id),
            actor(&user)?,
            EscalationDecisionId::new(decision),
        )
        .await?;
    Ok(Html(workflow_escalation_section(
        &view,
        Some(message.as_ref().map(|s| *s).map_err(|s| s.as_str())),
    )))
}
