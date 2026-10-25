//! Ingress replies to workflow-published messages (p08 D1.5). A reply resumes a `message.reply`
//! wait only when that wait is open for its correlation and the expected responder sent it;
//! everything else is ordinary ingress, so a waiting run never swallows an unrelated message.
use super::*;
use crate::application::workflow::triggers::MESSAGE_REPLY_EVENT;
use crate::application::workflow::waits::*;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::time::Duration;

/// The answered message as one run published it, and the step that published it.
#[derive(sqlx::FromRow)]
struct Destination {
    run_id: Uuid,
    kind: String,
    principal_id: Option<Uuid>,
    destination_id: Uuid,
    origin: String,
    step_id: String,
}

#[derive(sqlx::FromRow)]
struct LiveRun {
    binding_id: Uuid,
    live: bool,
}

#[derive(sqlx::FromRow)]
struct AwaitedWait {
    open: bool,
}

/// One delivery's wall-clock budget; each statement is also bounded by the lock and statement
/// timeouts set in `deliver_on`.
const REPLY_DELIVERY_BUDGET: Duration = Duration::from_secs(2);

pub(super) async fn deliver(
    persistence: &PostgresPersistence,
    delivery: IngressReply,
) -> AppResult<ReplyIngress> {
    deliver_within(persistence, delivery, REPLY_DELIVERY_BUDGET).await
}

/// Running out of `budget` is a transient failure, retried like a lock timeout; only the
/// outcomes of `deliver_on` are final (p08 F028).
pub(super) async fn deliver_within(
    persistence: &PostgresPersistence,
    delivery: IngressReply,
    budget: Duration,
) -> AppResult<ReplyIngress> {
    tokio::time::timeout(budget, async {
        let mut tx = persistence.pool.begin().await?;
        let result = deliver_on(&mut tx, &delivery).await?;
        if matches!(result, ReplyIngress::Recorded(_)) {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        Ok(result)
    })
    .await
    .map_err(|_| AppError::DatabaseTimeout("Workflow reply delivery ran out of time".into()))?
}

async fn deliver_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &IngressReply,
) -> AppResult<ReplyIngress> {
    let Some(destination) = destination_on(tx, delivery).await? else {
        return Ok(ReplyIngress::Unmatched);
    };
    // A model tool's send is not a step output, so no wait can correlate on it.
    if destination.origin != "step" || !expected_responder(tx, delivery, &destination).await? {
        return Ok(ReplyIngress::Unmatched);
    }
    let run = RunId::new(destination.run_id);
    let correlation = delivery.answers.as_uuid().to_string();
    sqlx::query("SELECT set_config('lock_timeout','1000ms',true),set_config('statement_timeout','1000ms',true)")
        .execute(&mut **tx).await?;
    // The producer lock `record_event` takes, so the one-response check below is exact.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('workflow_wait_events:' || $1::text || ':' || $2::text, 0))")
        .bind(delivery.company.as_uuid()).bind(run.as_uuid()).execute(&mut **tx).await?;
    // Producer identity first: a reprocessed intent whose event was recorded is a duplicate,
    // never a second answer (p08 F004).
    let saved: Option<(Uuid, String, String, Value)> = sqlx::query_as(
        "SELECT run_id, event_name, correlation, payload FROM workflow_wait_events WHERE company_id=$1 AND id=$2",
    )
    .bind(delivery.company.as_uuid())
    .bind(delivery.event.0)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((saved_run, name, saved_correlation, payload)) = saved {
        return Ok(
            if saved_run == run.as_uuid()
                && name == MESSAGE_REPLY_EVENT
                && saved_correlation == correlation
                && payload == delivery.payload
            {
                ReplyIngress::Duplicate(run)
            } else {
                ReplyIngress::Undeliverable("event_id_reused")
            },
        );
    }
    // Run row before wait rows, the order `resume_on`/`expire_on` lock them in (p08 F019).
    let Some(live) = sqlx::query_as::<_, LiveRun>(
        "SELECT binding_id, state IN ('queued','running','waiting') AND deadline>clock_timestamp() AS live \
         FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR KEY SHARE",
    )
    .bind(delivery.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .filter(|live| live.live) else {
        return Ok(ReplyIngress::Unmatched);
    };
    let binding = admission_binding::read_saved_binding(
        tx,
        delivery.company,
        WorkflowBindingId::new(live.binding_id),
        run.as_uuid(),
    )
    .await?;
    let compiled = binding.bundle().compiled();
    let publishing = StepId::parse(&destination.step_id).map_err(|_| invalid())?;
    let awaiting = awaited_reply_steps(compiled.graph().definition(), &publishing);
    let Some(first) = awaiting.first() else {
        return Ok(ReplyIngress::Unmatched);
    };
    // One clock value decides openness and stamps the event, so a reply judged before the
    // deadline is also recorded before it (p08 F020).
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await?;
    let steps: Vec<String> = awaiting.iter().map(ToString::to_string).collect();
    let waits: Vec<AwaitedWait> = sqlx::query_as(
        "SELECT wait.state='waiting' AND wait.deadline>$5 AS open \
         FROM workflow_waits AS wait JOIN workflow_executions AS execution \
           ON execution.company_id=wait.company_id AND execution.run_id=wait.run_id AND execution.id=wait.execution_id \
         WHERE wait.company_id=$1 AND wait.run_id=$2 AND wait.reason='event' AND wait.event_name=$3 \
           AND wait.correlation=$4 AND execution.step_id = ANY($6) \
         ORDER BY wait.created_at, wait.id FOR SHARE OF wait",
    )
    .bind(delivery.company.as_uuid())
    .bind(run.as_uuid())
    .bind(MESSAGE_REPLY_EVENT)
    .bind(&correlation)
    .bind(now)
    .bind(&steps)
    .fetch_all(&mut **tx)
    .await?;
    if !waits.iter().any(|wait| wait.open) {
        return Ok(if waits.is_empty() && now < delivery.park_grace_until {
            ReplyIngress::NotYet
        } else {
            ReplyIngress::Unmatched
        });
    }
    // One response per wait: a second answer to the same message is ordinary ingress.
    let answered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND event_name=$3 AND correlation=$4)",
    )
    .bind(delivery.company.as_uuid())
    .bind(run.as_uuid())
    .bind(MESSAGE_REPLY_EVENT)
    .bind(&correlation)
    .fetch_one(&mut **tx)
    .await?;
    if answered {
        return Ok(ReplyIngress::Unmatched);
    }
    if compiled
        .validate_step_output(first, &delivery.payload)
        .is_err()
    {
        return Ok(ReplyIngress::Undeliverable("payload_rejected"));
    }
    let unconsumed: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_wait_events WHERE company_id=$1 AND run_id=$2 AND consumed_execution_id IS NULL")
        .bind(delivery.company.as_uuid()).bind(run.as_uuid()).fetch_one(&mut **tx).await?;
    if unconsumed >= MAX_UNCONSUMED_EVENTS {
        return Ok(ReplyIngress::Undeliverable("event_inbox_full"));
    }
    sqlx::query("INSERT INTO workflow_wait_events(company_id,run_id,id,event_name,correlation,payload,created_at) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(delivery.company.as_uuid()).bind(run.as_uuid()).bind(delivery.event.0)
        .bind(MESSAGE_REPLY_EVENT).bind(&correlation).bind(&delivery.payload).bind(now)
        .execute(&mut **tx).await?;
    Ok(ReplyIngress::Recorded(run))
}

async fn destination_on(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &IngressReply,
) -> AppResult<Option<Destination>> {
    Ok(sqlx::query_as(
        "SELECT target.run_id, target.kind, target.principal_id, target.destination_id, action.origin, execution.step_id \
         FROM workflow_message_destinations AS target \
         JOIN workflow_message_actions AS action ON action.company_id=target.company_id AND action.invocation_id=target.invocation_id \
         JOIN workflow_executions AS execution ON execution.company_id=action.company_id \
           AND execution.run_id=action.run_id AND execution.id=action.execution_id \
         WHERE target.company_id=$1 AND target.message_id=$2 AND target.thread_id=$3",
    )
    .bind(delivery.company.as_uuid())
    .bind(delivery.answers.as_uuid())
    .bind(delivery.thread)
    .fetch_optional(&mut **tx)
    .await?)
}

/// The responder must be whom the destination addressed (p08 D1.5.4): the member of a `user`
/// send, the sender of the message a `reply` answered, or a member posting in-app to a
/// `channel` post.
async fn expected_responder(
    tx: &mut Transaction<'_, Postgres>,
    delivery: &IngressReply,
    destination: &Destination,
) -> AppResult<bool> {
    let Some(responder) = delivery.responder else {
        return Ok(false);
    };
    Ok(match destination.kind.as_str() {
        "user" => destination.principal_id == Some(responder),
        "reply" => {
            let author: Option<Uuid> = sqlx::query_scalar(
                "SELECT author_principal_id FROM messages WHERE company_id=$1 AND id=$2",
            )
            .bind(delivery.company.as_uuid())
            .bind(destination.destination_id)
            .fetch_optional(&mut **tx)
            .await?;
            author == Some(responder)
        }
        "channel" => delivery.in_app,
        _ => false,
    })
}
