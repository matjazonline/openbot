//! An ordinary authenticated agent has direct participation authority without a notice exception.
use super::*;

#[tokio::test]
async fn relay_identity_actual_agent_to_private_agent_admits_ordinary_work() {
    let f = Box::pin(queued(Receiver::PrivateAgent)).await;
    let p = f.case.people.role.p();
    let company = f.case.people.role.company().as_uuid();
    let principal = f.target.principal_id().unwrap();
    let smtp = Box::pin(deliver(&f.case, relay_threads(p))).await;
    assert!(smtp.sent().is_empty());
    assert_eq!(delivered(&f.case, f.delivery).await, "delivered");
    let sender = f.case.created.principal_id.as_uuid();
    assert_eq!(
        received(&f).await,
        [ReceivedHop {
            author_principal: sender,
            sender_principal: sender
        }]
    );
    let message: Uuid = sqlx::query_scalar(
        "SELECT relay.message_id FROM message_internal_relays AS relay \
         JOIN workflow_message_intents AS intent ON intent.company_id=relay.company_id AND intent.message_id=relay.message_id \
         WHERE relay.company_id=$1 AND relay.delivery_id=$2 AND intent.channel_id IS NULL \
         AND intent.principal_id=$3 AND intent.origin='internal'",
    ).bind(company).bind(f.delivery.as_uuid()).bind(principal.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_agent_receipts WHERE company_id=$1 AND message_id=$2")
        .bind(company).bind(message).fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        receipts, 0,
        "ordinary relay must not borrow a platform-notice receipt"
    );
    let binding: Uuid = sqlx::query_scalar(
        "SELECT binding_id FROM channel_reply_settings WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(company)
    .bind(principal.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    for claim in claims(&f.case).await {
        process(&f.case, claim).await;
    }
    let run = admitted_from(&f.case, message, WorkflowBindingId::new(binding)).await;
    let exact: bool = sqlx::query_scalar(
        "SELECT run.channel_id IS NULL AND run.principal_id=$3 AND run.binding_id=$4 \
         AND NOT EXISTS(SELECT 1 FROM workflow_escalation_handling_roots AS handling \
             WHERE handling.company_id=run.company_id AND handling.run_id=run.id) \
         AND EXISTS(SELECT 1 FROM run_dependency_manifests AS ownership \
             JOIN agent_manifest_revisions AS selection ON selection.company_id=ownership.company_id AND selection.manifest_id=ownership.manifest_id \
             JOIN principals AS target ON target.company_id=selection.company_id AND target.agent_id=selection.agent_id \
             WHERE ownership.company_id=run.company_id AND ownership.run_id=run.id AND target.id=$3) \
         FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
    ).bind(company).bind(run.as_uuid()).bind(principal.as_uuid()).bind(binding).fetch_one(p.pool()).await.unwrap();
    assert!(
        exact,
        "the ordinary private agent's selected workflow must own the admitted run"
    );
}
