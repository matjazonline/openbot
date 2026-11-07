//! Trusted ingress for a prompt and its exact declared human wait.
use super::*;
use crate::application::workflow::{triggers::CHANNEL_QUESTION_REPLY_EVENT, waits::*};
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(sqlx::FromRow)]
struct Question {
    run_id: Uuid,
    execution_id: Uuid,
    wait_id: Uuid,
    respondent_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct Event {
    run_id: Uuid,
    event_name: String,
    correlation: String,
    payload: Value,
}

#[derive(sqlx::FromRow)]
struct Run {
    binding_id: Uuid,
    live: bool,
}

pub(super) async fn deliver_on(
    tx: &mut Transaction<'_, Postgres>,
    reply: &IngressReply,
) -> AppResult<Option<ReplyIngress>> {
    let question: Option<Question> = sqlx::query_as(
        "SELECT question.run_id, question.execution_id, question.wait_id, question.respondent_id \
         FROM workflow_channel_questions AS question \
         JOIN workflow_message_destinations AS destination ON destination.company_id=question.company_id \
           AND destination.invocation_id=question.invocation_id AND destination.message_id=question.message_id \
         WHERE question.company_id=$1 AND question.message_id=$2 AND destination.thread_id=$3",
    ).bind(reply.company.as_uuid()).bind(reply.answers.as_uuid()).bind(reply.thread)
        .fetch_optional(&mut **tx).await?;
    let Some(question) = question else {
        return Ok(None);
    };
    if reply.responder != Some(question.respondent_id) {
        return Ok(Some(ReplyIngress::Unmatched));
    }
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
        .execute(&mut **tx).await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('workflow_wait_events:' || $1::text || ':' || $2::text, 0))")
        .bind(reply.company.as_uuid()).bind(question.run_id).execute(&mut **tx).await?;
    Ok(Some(record_on(tx, reply, &question).await?))
}

async fn record_on(
    tx: &mut Transaction<'_, Postgres>,
    reply: &IngressReply,
    question: &Question,
) -> AppResult<ReplyIngress> {
    let run = RunId::new(question.run_id);
    let correlation = reply.answers.as_uuid().to_string();
    let saved: Option<Event> = sqlx::query_as("SELECT run_id,event_name,correlation,payload FROM workflow_wait_events WHERE company_id=$1 AND id=$2")
        .bind(reply.company.as_uuid()).bind(reply.event.0).fetch_optional(&mut **tx).await?;
    if let Some(saved) = saved {
        return Ok(
            if saved.run_id == question.run_id
                && saved.event_name == CHANNEL_QUESTION_REPLY_EVENT
                && saved.correlation == correlation
                && saved.payload == reply.payload
            {
                ReplyIngress::Duplicate(run)
            } else {
                ReplyIngress::Undeliverable("event_id_reused")
            },
        );
    }
    let current: Option<Run> = sqlx::query_as(
        "SELECT binding_id,state IN ('queued','running','waiting') AND deadline>clock_timestamp() AS live \
         FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR SHARE",
    ).bind(reply.company.as_uuid()).bind(question.run_id).fetch_optional(&mut **tx).await?;
    let Some(current) = current.filter(|run| run.live) else {
        return Ok(ReplyIngress::Unmatched);
    };
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await?;
    let open: Option<bool> = sqlx::query_scalar(
        "SELECT state='waiting' AND deadline>$5 FROM workflow_waits \
         WHERE company_id=$1 AND run_id=$2 AND execution_id=$3 AND id=$4 \
           AND reason='event' AND event_name='channel.question.reply' AND correlation=$6 FOR SHARE",
    )
    .bind(reply.company.as_uuid())
    .bind(question.run_id)
    .bind(question.execution_id)
    .bind(question.wait_id)
    .bind(now)
    .bind(&correlation)
    .fetch_optional(&mut **tx)
    .await?;
    if open != Some(true) {
        return Ok(ReplyIngress::Unmatched);
    }
    let answered: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND event_name=$3 AND correlation=$4)")
        .bind(reply.company.as_uuid()).bind(question.run_id).bind(CHANNEL_QUESTION_REPLY_EVENT)
        .bind(&correlation).fetch_one(&mut **tx).await?;
    if answered {
        return Ok(ReplyIngress::Unmatched);
    }
    let binding = admission_binding::read_saved_binding(
        tx,
        reply.company,
        WorkflowBindingId::new(current.binding_id),
        question.run_id,
    )
    .await?;
    let step: String = sqlx::query_scalar(
        "SELECT step_id FROM workflow_executions WHERE company_id=$1 AND run_id=$2 AND id=$3",
    )
    .bind(reply.company.as_uuid())
    .bind(question.run_id)
    .bind(question.execution_id)
    .fetch_one(&mut **tx)
    .await?;
    let step = StepId::parse(step).map_err(|_| invalid())?;
    if binding
        .bundle()
        .compiled()
        .validate_step_output(&step, &reply.payload)
        .is_err()
    {
        return Ok(ReplyIngress::Undeliverable("payload_rejected"));
    }
    insert_on(tx, reply, question, now).await
}

async fn insert_on(
    tx: &mut Transaction<'_, Postgres>,
    reply: &IngressReply,
    question: &Question,
    now: DateTime<Utc>,
) -> AppResult<ReplyIngress> {
    let unconsumed: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND consumed_execution_id IS NULL")
        .bind(reply.company.as_uuid()).bind(question.run_id).fetch_one(&mut **tx).await?;
    if unconsumed >= MAX_UNCONSUMED_EVENTS {
        return Ok(ReplyIngress::Undeliverable("event_inbox_full"));
    }
    let inserted = sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,id,event_name,correlation,payload,created_at) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(reply.company.as_uuid()).bind(question.run_id).bind(reply.event.0)
        .bind(CHANNEL_QUESTION_REPLY_EVENT).bind(reply.answers.as_uuid().to_string())
        .bind(&reply.payload).bind(now).execute(&mut **tx).await?;
    Ok(if inserted.rows_affected() == 1 {
        ReplyIngress::Recorded(RunId::new(question.run_id))
    } else {
        ReplyIngress::Undeliverable("event_id_reused")
    })
}
