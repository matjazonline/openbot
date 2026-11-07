//! Declared questions share the real message action and delivery ledgers. The event wait,
//! canonical prompt and its immutable respondent are committed before provider work exists.
use super::super::knowledge_release::{self, MessageRecipient, MessageRecipients};
use super::super::*;
use crate::application::agent::ToolCallOrigin;
use crate::application::workflow::{
    actions::*, message::*, triggers::CHANNEL_QUESTION_REPLY_EVENT,
};
use crate::entities::transport::DeliveryCancellationReason;
use chrono::{DateTime, Utc};
use serde_json::json;

pub(super) struct Question {
    deadline: DateTime<Utc>,
    source: Uuid,
    message: Uuid,
    respondent: Uuid,
}

#[derive(sqlx::FromRow)]
struct QuestionExecution {
    binding_id: Uuid,
    step_id: String,
    frozen_inputs: serde_json::Value,
    deadline: DateTime<Utc>,
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    action: &FrozenAction,
    plan: &MessagePlan,
    origin: MessageOrigin,
) -> AppResult<Option<Question>> {
    let request = action.request();
    if request.operation_key.as_str() != "message.ask" {
        return Ok(None);
    }
    if origin != MessageOrigin::Step || request.context.origin != ToolCallOrigin::Workflow {
        return Err(missing());
    }
    let requested = request
        .arguments
        .get("deadline")
        .and_then(serde_json::Value::as_str)
        .filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
        .ok_or_else(|| AppError::BadRequest("A question requires an absolute deadline".into()))?;
    let scope = request.scope;
    let execution: QuestionExecution = sqlx::query_as(
        "SELECT run.binding_id, execution.step_id, execution.frozen_inputs, LEAST(run.deadline,$4::text::timestamptz) AS deadline \
         FROM workflow_runs AS run JOIN workflow_executions AS execution \
           ON execution.company_id=run.company_id AND execution.run_id=run.id \
         WHERE run.company_id=$1 AND run.id=$2 AND execution.id=$3",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(requested)
    .fetch_one(&mut *db)
    .await?;
    let binding = admission_binding::read_saved_binding(
        db,
        scope.company,
        WorkflowBindingId::new(execution.binding_id),
        scope.run.as_uuid(),
    )
    .await?;
    let step = StepId::parse(&execution.step_id).map_err(|_| invalid())?;
    if binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .get(&step)
        .is_none_or(|step| step.step_type.as_str() != "message.ask")
    {
        return Err(missing());
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *db)
        .await?;
    let deadline = question_deadline(execution, request, now)?;
    let destination = requested_destination(request, plan)?;
    let respondent: Option<Uuid> = sqlx::query_scalar(
        "SELECT author_principal_id FROM messages WHERE company_id=$1 AND id=$2 FOR KEY SHARE",
    )
    .bind(scope.company.as_uuid())
    .bind(destination.destination_id)
    .fetch_optional(&mut *db)
    .await?
    .flatten();
    Ok(Some(Question {
        deadline,
        source: destination.destination_id,
        message: destination.message.id.as_uuid(),
        respondent: respondent.ok_or_else(missing)?,
    }))
}

fn requested_destination<'a>(
    request: &ActionRequest,
    plan: &'a MessagePlan,
) -> AppResult<&'a PlannedDestination> {
    let inputs = &request.arguments;
    let [destination] = plan.destinations.as_slice() else {
        return Err(invalid());
    };
    if destination.kind != DestinationKind::Reply
        || inputs["source_message"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            != Some(destination.destination_id)
        || inputs["content"]["body"].as_str() != Some(destination.message.clean_text_body.as_str())
    {
        return Err(invalid());
    }
    Ok(destination)
}

fn question_deadline(
    execution: QuestionExecution,
    request: &ActionRequest,
    now: DateTime<Utc>,
) -> AppResult<DateTime<Utc>> {
    let mut inputs = execution.frozen_inputs;
    inputs
        .as_object_mut()
        .ok_or_else(invalid)?
        .remove("approval");
    if inputs != request.arguments {
        return Err(invalid());
    }
    // PostgreSQL owns the original RFC3339 value's microsecond rounding.
    let deadline = execution.deadline;
    if deadline <= now {
        return Err(AppError::BadRequest(
            "The question deadline has elapsed".into(),
        ));
    }
    Ok(deadline)
}

pub(super) async fn insert_on(
    db: &mut PgConnection,
    request: &ActionDispatchRequest,
    wait: Uuid,
    question: Question,
) -> AppResult<()> {
    let scope = request.scope();
    let notification = json!({"version":1,"wait_id":wait,"message_id":question.message});
    sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,event_name,correlation,notification_intent,created_at) VALUES($1,$2,$3,$4,'event',$5,$6,$7,$8,clock_timestamp())")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(wait).bind(question.deadline).bind(CHANNEL_QUESTION_REPLY_EVENT)
        .bind(question.message.to_string()).bind(notification).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_channel_questions(company_id,run_id,execution_id,invocation_id,wait_id,message_id,source_message_id,respondent_id,deadline) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(wait).bind(question.message)
        .bind(question.source).bind(question.respondent).bind(question.deadline)
        .execute(db).await?;
    Ok(())
}

/// The caller holds the run/wait lock. Delivery cancellation never rewrites a provider fact.
pub(in crate::adapters::persistence::workflow) async fn close_on(
    db: &mut PgConnection,
    scope: crate::application::workflow::activation::ActivationRequest,
) -> AppResult<()> {
    let invocation: Option<Uuid> = sqlx::query_scalar(
        "SELECT invocation_id FROM workflow_channel_questions WHERE company_id=$1 AND run_id=$2 AND execution_id=$3",
    ).bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .fetch_optional(&mut *db).await?;
    if let Some(invocation) = invocation {
        super::receipts::cancel_message_deliveries_on(
            db,
            scope.company,
            scope.run,
            Some(invocation),
            DeliveryCancellationReason::QuestionClosed,
        )
        .await?;
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ProviderQuestion {
    wait_open: bool,
    source_current: bool,
    respondent_current: bool,
    company_id: Uuid,
    run_id: Uuid,
    execution_id: Uuid,
    source_message_id: Uuid,
    recipient_address: Option<String>,
    operation: serde_json::Value,
}

/// Provider admission holds the run's SHARE lock before the wait and source locks. A
/// completed/expired wait or changed source permits no new provider entry.
pub(in crate::adapters::persistence::workflow) async fn delivery_refusal_on(
    db: &mut PgConnection,
    delivery: Uuid,
) -> AppResult<Option<DeliveryCancellationReason>> {
    let question: Option<ProviderQuestion> = sqlx::query_as(
        "SELECT question.company_id,question.run_id,question.execution_id,question.source_message_id, \
           destination.recipient_address,intent.operation, \
           wait.state='waiting' AND wait.deadline>clock_timestamp() AS wait_open, \
           EXISTS(SELECT 1 FROM thread_messages AS association \
             JOIN messages AS source ON source.company_id=association.company_id AND source.id=association.message_id \
             WHERE association.company_id=question.company_id AND association.message_id=question.source_message_id \
               AND association.thread_id=destination.thread_id AND association.channel_id=destination.channel_id \
               FOR SHARE OF association, source) AS source_current, \
           EXISTS(SELECT 1 FROM messages AS source WHERE source.company_id=question.company_id \
             AND source.id=question.source_message_id AND source.author_principal_id=question.respondent_id \
             FOR SHARE OF source) AS respondent_current \
         FROM workflow_channel_questions AS question \
         JOIN workflow_message_destinations AS destination ON destination.company_id=question.company_id \
           AND destination.invocation_id=question.invocation_id AND destination.message_id=question.message_id \
         JOIN workflow_waits AS wait ON wait.company_id=question.company_id AND wait.id=question.wait_id \
         JOIN workflow_action_intents AS intent ON intent.company_id=question.company_id \
           AND intent.run_id=question.run_id AND intent.execution_id=question.execution_id AND intent.id=question.invocation_id \
         WHERE destination.delivery_id=$1 FOR SHARE OF wait",
    ).bind(delivery).fetch_optional(&mut *db).await?;
    let Some(question) = question else {
        return Ok(None);
    };
    if !question.wait_open {
        return Ok(Some(DeliveryCancellationReason::QuestionClosed));
    }
    if !question.source_current {
        return Ok(Some(DeliveryCancellationReason::WorkflowRevoked));
    }
    if !question.respondent_current {
        return Ok(Some(DeliveryCancellationReason::RecipientRevoked));
    }
    // Box current/frozen release evaluation; never manufacture a live workflow fence here.
    Box::pin(release_refusal_on(db, question)).await
}

async fn release_refusal_on(
    db: &mut PgConnection,
    question: ProviderQuestion,
) -> AppResult<Option<DeliveryCancellationReason>> {
    let action = FrozenAction::restore(question.operation)?;
    let scope = ActionScope {
        company: CompanyId::new(question.company_id),
        run: RunId::new(question.run_id),
        execution: ExecutionId::new(question.execution_id),
    };
    if action.scope() != scope
        || action.request().operation_key.as_str() != "message.ask"
        || action.request().context.origin != ToolCallOrigin::Workflow
        || action.request().arguments["source_message"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            != Some(question.source_message_id)
    {
        return Err(invalid());
    }
    let frozen = [MessageRecipient::reply(
        question.source_message_id,
        question
            .recipient_address
            .map(crate::entities::value_objects::EmailAddress::from),
    )];
    // Reuse the established MVCC audience snapshot boundary, including the exact retired-address
    // check. Later edits order after this release decision; no recipient/thread lock is invented.
    for recipients in [
        MessageRecipients::Current,
        MessageRecipients::Frozen(&frozen),
    ] {
        match knowledge_release::evaluate_on(db, &action, recipients).await {
            Ok(_) => {}
            Err(
                AppError::KnowledgeRelease { .. }
                | AppError::NotFound(_)
                | AppError::Conflict(_)
                | AppError::BadRequest(_),
            ) => {
                // The genuine scope/source was checked above. Missing or changed release facts
                // (including a retired frozen address) revoke this recipient's queued delivery.
                return Ok(Some(DeliveryCancellationReason::RecipientRevoked));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}
