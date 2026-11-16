//! Escalation tool effects and their action receipt share the existing fenced writer transaction.
use super::action_dispatch::{LockedActionAuthority, SqlActionAuthority, SqlLocalAction};
use super::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::binding::{ResourceReadiness, ResourceStatus};
use crate::application::workflow::decisions::DecisionCommandKey;
use crate::application::workflow::escalation::{EscalationSubmission, SubmitEscalation, tools::*};
use serde_json::Value;
use sqlx::Acquire;
use std::collections::BTreeSet;

pub struct EscalationActionAuthority;
pub struct EscalationEffects;

#[async_trait]
impl SqlActionAuthority for EscalationActionAuthority {
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        let request = authority.action.request();
        if !matches!(&request.target,ActionTarget::Local { resource,resource_kind }
            if *resource==request.scope.company.as_uuid() && resource_kind.as_str()==RESOURCE_KIND)
        {
            return Err(missing());
        }
        authority::authorize_company(tx, request.scope.company, authority.actor).await?;
        let kind = EscalationTool::parse(request.operation_key.as_str()).ok_or_else(missing)?;
        let decision = match kind {
            EscalationTool::Read => {
                serde_json::from_value::<ReadInput>(request.arguments.clone())
                    .map_err(|_| invalid())?
                    .decision_id
            }
            EscalationTool::Respond => {
                serde_json::from_value::<RespondInput>(request.arguments.clone())
                    .map_err(|_| invalid())?
                    .decision_id
            }
        };
        // Successful saved observations retain current read scope for their referenced case.
        // A fresh unavailable command instead commits a typed refusal in the local effect;
        // replaying that public refusal does not require a case that never existed.
        let saved_success:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_action_receipts AS receipt \
            JOIN workflow_action_intents AS intent ON intent.company_id=receipt.company_id AND intent.id=receipt.invocation_id \
            WHERE intent.company_id=$1 AND intent.run_id=$2 AND intent.execution_id=$3 AND intent.argument_digest=$4 \
              AND receipt.result->>'ok'='true')")
            .bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid()).bind(request.scope.execution.as_uuid())
            .bind(authority.action.argument_digest().as_str()).fetch_one(&mut **tx).await?;
        if saved_success {
            escalations::read_prelocked_on(
                tx,
                request.scope.company,
                authority.actor,
                decision,
                escalations::ReadPurpose::Tool,
            )
            .await?;
        }
        Ok(LockedActionAuthority {
            resource: ResourceStatus {
                id: RuntimeResourceId::new(request.scope.company.as_uuid()),
                company_id: request.scope.company,
                kind: TypeName::parse(RESOURCE_KIND).map_err(|_| invalid())?,
                supported_contracts: EscalationTool::ALL
                    .into_iter()
                    .map(|tool| TypeName::parse(tool.name()).map_err(|_| invalid()))
                    .collect::<AppResult<BTreeSet<_>>>()?,
                authorized: true,
                readiness: ResourceReadiness::Ready,
            },
            policy: CurrentActionPolicy {
                tool: snapshot(request.scope.company, kind)?,
                approval_required: false,
            },
        })
    }
}

#[async_trait]
impl EscalationPreparation for PostgresPersistence {
    async fn escalation_tool_request(
        &self,
        scope: ActionScope,
        call: &EscalationToolCall,
    ) -> AppResult<ActionRequest> {
        let crate::application::agent::ToolCallOrigin::Agent(origin) = call.origin else {
            return Err(missing());
        };
        let mut tx = http::begin_http(self).await?;
        let facts = http::runtime::facts(&mut tx, scope).await?;
        let request = agent_origin::request_on(
            &mut tx,
            scope,
            &facts,
            origin,
            &call.call,
            call.arguments.clone(),
        )
        .await?;
        if request.operation_key.as_str() != call.kind.name() || !trusted(&request.contract)? {
            return Err(missing());
        }
        tx.commit().await?;
        Ok(request)
    }
}

fn command(action: &FrozenAction, subject: &ApprovalSubject) -> AppResult<SubmitEscalation> {
    let request = action.request();
    let input: RespondInput =
        serde_json::from_value(request.arguments.clone()).map_err(|_| invalid())?;
    Ok(SubmitEscalation {
        company: request.scope.company,
        actor: WorkflowActor::authenticated(request.context.actor)?,
        decision: input.decision_id,
        choice: input.choice,
        command: DecisionCommandKey::parse(format!(
            "escalation-tool:{}",
            subject.invocation.as_uuid()
        ))?,
    })
}

/// Discovery is lock-free and uses the exact persisted intent, never caller arguments.
pub(super) async fn target_hint_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
) -> AppResult<Option<escalations::CommandTarget>> {
    let operation: Option<Value> = sqlx::query_scalar(
        "SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 \
        AND execution_id=$3 AND id=$4 AND argument_digest=$5",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.scope().run.as_uuid())
    .bind(request.scope().execution.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .bind(request.subject.argument_digest.as_str())
    .fetch_optional(&mut *db)
    .await?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    let action = FrozenAction::restore(operation)?;
    if action.scope() != request.scope()
        || action.argument_digest() != &request.subject.argument_digest
    {
        return Err(invalid());
    }
    if action.request().operation_key.as_str() != EscalationTool::Respond.name()
        || !trusted(&action.request().contract)?
    {
        return Ok(None);
    }
    escalations::target_hint_on(db, &command(&action, &request.subject)?).await
}

#[async_trait]
impl SqlLocalAction for EscalationEffects {
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
        subject: &ApprovalSubject,
    ) -> AppResult<Value> {
        let request = action.request();
        let kind = EscalationTool::parse(request.operation_key.as_str()).ok_or_else(missing)?;
        // A savepoint rolls back a refused command before the outer action commits its typed
        // refusal receipt. Success stays in this SAME connection/transaction/fence.
        let mut savepoint = tx.begin().await?;
        let result = match kind {
            EscalationTool::Read => {
                let input: ReadInput =
                    serde_json::from_value(request.arguments.clone()).map_err(|_| invalid())?;
                let actor = WorkflowActor::authenticated(request.context.actor)?;
                escalations::read_prelocked_on(
                    &mut savepoint,
                    request.scope.company,
                    actor,
                    input.decision_id,
                    escalations::ReadPurpose::Tool,
                )
                .await
                .and_then(read_result)
            }
            EscalationTool::Respond => {
                let command = command(action, subject)?;
                // The action hint collector already locked/revalidated this exact target's
                // account before company; this lookup cannot acquire a new account.
                let target = escalations::target_hint_on(&mut savepoint, &command).await?;
                escalations::submit_prelocked_on(&mut savepoint,&command,target.as_ref()).await
                    .map(|result|serde_json::json!({"ok":true,"replayed":result==EscalationSubmission::Replayed}))
            }
        };
        match result {
            Ok(value) => {
                savepoint.commit().await?;
                Ok(value)
            }
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => {
                savepoint.rollback().await?;
                Ok(refusal(
                    "workflow.escalation_unavailable",
                    "Escalation or current authority is unavailable",
                ))
            }
            Err(AppError::Conflict(_)) => {
                savepoint.rollback().await?;
                Ok(refusal(
                    "workflow.escalation_changed",
                    "Escalation or its run revision has changed",
                ))
            }
            Err(AppError::BadRequest(_)) => {
                savepoint.rollback().await?;
                Ok(refusal(
                    "workflow.invalid_input",
                    "This escalation command is unavailable",
                ))
            }
            Err(error) => {
                savepoint.rollback().await?;
                Err(error)
            }
        }
    }
}

fn read_result(view: crate::application::workflow::escalation::EscalationView) -> AppResult<Value> {
    let mut value = serde_json::to_value(view).map_err(|_| invalid())?;
    let fields = value.as_object_mut().ok_or_else(invalid)?;
    fields.remove("assignment_options");
    fields.remove("assignment_options_truncated");
    Ok(serde_json::json!({"ok":true,"decision":value}))
}
