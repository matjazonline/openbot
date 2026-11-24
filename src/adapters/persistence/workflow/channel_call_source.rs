//! A channel call's fresh destination thread and its one canonical source message
//! (agent-workflow-call gate AWC-P03-D1 r1 D2).
//!
//! The message is stored the way a workflow `message.send` to a desk stores its own: canonical
//! content in a new desk thread, stamped with the calling run's classification, with no
//! delivery. It writes no `workflow_message_intents` row, so no observer, responder admission,
//! reply context or transport follows: it is never ingress. The producer seal accepts it
//! through the call's invocation record, written later in the same transaction.
use super::*;
use crate::application::ports::thread::{MessageAuthorWrite, MessageWrite};
use crate::application::workflow::channel_call::ChannelCallRequest;
use crate::application::workflow::message::{
    DestinationKind, NewThread, PlannedDestination, RouteCheck,
};
use crate::application::workflow::{actions::ActionScope, triggers::MessageTriggerInput};
use crate::entities::address::AddressTarget;
use crate::entities::message::{MessageDirection, MessageRole, ThreadEntryKind};

/// Where the call's request was stored.
#[derive(Debug, Clone, Copy)]
pub(super) struct CallSource {
    pub thread: Uuid,
    pub message: crate::entities::message::CanonicalMessageId,
}

/// Store the request on a fresh thread of `desk`, authored by the calling agent, and derive the
/// child's trigger input from the stored message through the existing normalizer.
pub(super) async fn store_on(
    tx: &mut Transaction<'_, Postgres>,
    caller: ActionScope,
    desk: Uuid,
    request: &ChannelCallRequest,
    author: crate::entities::transport::PrincipalId,
) -> AppResult<(CallSource, MessageTriggerInput)> {
    let correlation: Uuid = sqlx::query_scalar(
        "SELECT correlation_id FROM workflow_runs WHERE company_id=$1 AND id=$2",
    )
    .bind(caller.company.as_uuid())
    .bind(caller.run.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    let thread = Uuid::new_v4();
    // The thread carries the calling run's goal, so a later reply on it serves the same goal.
    let goal = super::goals::for_thread_of_on(tx, caller.company, caller.run).await?;
    let message = MessageWrite::internal(
        thread,
        MessageAuthorWrite::Principal(author),
        request.content.subject().to_string(),
        request.content.body_text().to_string(),
        MessageDirection::Inbound,
        MessageRole::Agent,
        crate::entities::correlation::CorrelationId::from(correlation),
    )
    // A request delegated to the desk, as an internal relay files it.
    .with_entry_kind(ThreadEntryKind::Delegation);
    let destination = PlannedDestination {
        kind: DestinationKind::Channel,
        destination_id: desk,
        new_thread: Some(NewThread {
            goal_unit_id: goal,
            id: thread,
            target: AddressTarget::Desk(desk),
            subject: request.content.subject().to_string(),
        }),
        target: AddressTarget::Desk(desk),
        thread_id: thread,
        message,
        delivery: None,
        principal_id: None,
        recipient: None,
        check: RouteCheck::Channel { channel: desk },
    };
    let receipt = super::message::writer::write_destination_on(tx, caller, &destination).await?;
    let input = super::message_intents::trigger_input_on(tx, caller.company, receipt.message)
        .await?
        .with_fields(request.fields.clone());
    Ok((
        CallSource {
            thread,
            message: receipt.message,
        },
        input,
    ))
}

/// The source message of the nearest call enclosing `run`: the run itself when it is a call's
/// child, else its closest such ancestor. `None` outside any call subtree. A call subtree's
/// correspondent is the calling agent and its context is this message, never the tree root's
/// ingress (AWC-P04-B1-F3, F4).
pub(super) async fn enclosing_call_source_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE chain(id, depth) AS ( \
           SELECT $2::uuid, 0 \
           UNION ALL \
           SELECT run.parent_run_id, chain.depth + 1 FROM chain \
           JOIN workflow_runs AS run ON run.company_id = $1 AND run.id = chain.id \
           WHERE run.parent_run_id IS NOT NULL AND chain.depth < 64) \
         SELECT call.source_message_id FROM chain JOIN workflow_channel_calls AS call \
           ON call.company_id = $1 AND call.child_run_id = chain.id \
         ORDER BY chain.depth LIMIT 1",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(db)
    .await?)
}
