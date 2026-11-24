//! The caller's trail of a channel call (agent-workflow-call doc 04 C.2; gate AWC-P04-D1 r2 D6).
//!
//! When a call's result is first disclosed to the calling run, one internal note is filed on the
//! thread that run answers, if it has one. The note names the calling agent, the destination
//! desk, the child run and the closure, never the response itself, so it is stamped `Public`
//! and authored by the platform. It is written in the transaction that inserts the call's
//! consumption fact, and only on that insert, so a replay writes nothing twice. Nothing reads it
//! back: the fact is the source of truth (C.4).
use super::*;
use crate::adapters::persistence::thread::{MessageStampSource, insert_message_on};
use crate::application::ports::thread::{MessageAuthorWrite, MessageWrite};
use crate::application::workflow::activation::ActivationRequest;
use crate::entities::message::{MessageDirection, MessageRole, ThreadEntryKind};

#[derive(sqlx::FromRow)]
struct NoteFacts {
    thread_id: Option<Uuid>,
    correlation_id: Uuid,
    desk: String,
    agent: String,
    child_run_id: Uuid,
}

/// File the caller's note for `call`, whose result just closed as `closure` (with `code` for
/// an error closure).
pub(super) async fn file_on(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    call: Uuid,
    closure: &str,
    code: Option<&str>,
) -> AppResult<()> {
    let facts = sqlx::query_as::<_, NoteFacts>(
        "SELECT run.thread_id, run.correlation_id, desk.name AS desk, author.display_label AS agent, call.child_run_id \
         FROM workflow_channel_calls AS call \
         JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id \
         JOIN channels AS desk ON desk.company_id=call.company_id AND desk.id=call.channel_id \
         JOIN messages AS source ON source.company_id=call.company_id AND source.id=call.source_message_id \
         JOIN principals AS author ON author.company_id=source.company_id AND author.id=source.author_principal_id \
         WHERE call.company_id=$1 AND call.run_id=$2 AND call.execution_id=$3 AND call.call_id=$4",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(call)
    .fetch_one(&mut **tx)
    .await?;
    // A caller without a thread (a schedule, a company-wide binding) records nothing here; its
    // run page shows the lineage.
    let Some(thread) = facts.thread_id else {
        return Ok(());
    };
    let outcome = match code {
        Some(code) => format!("{closure} ({code})"),
        None => closure.to_owned(),
    };
    let note = MessageWrite::internal(
        thread,
        MessageAuthorWrite::Platform,
        format!("Call to {}", facts.desk),
        format!(
            "{} asked {}. The desk's run {} closed: {}.",
            facts.agent, facts.desk, facts.child_run_id, outcome
        ),
        MessageDirection::Inbound,
        MessageRole::System,
        crate::entities::correlation::CorrelationId::from(facts.correlation_id),
    )
    .with_entry_kind(ThreadEntryKind::Note);
    insert_message_on(tx, &note, MessageStampSource::Public).await?;
    Ok(())
}
