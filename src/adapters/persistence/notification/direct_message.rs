//! Incoming personal receipts rehydrate their exact association and captured membership.
use super::projection::{SourceLifecycle, SourceSnapshot};
use super::*;
use crate::entities::{address::AddressTarget, notification::NotificationTarget};

#[derive(sqlx::FromRow)]
struct Source {
    principal_id: Uuid,
    thread_id: Uuid,
    correlation_id: Uuid,
    visible: bool,
}

pub(super) async fn source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<SourceSnapshot>> {
    let row = sqlx::query_as::<_, Source>(
        "SELECT association.principal_id,association.thread_id,message.correlation_id, \
         direct_message_notification_can_view($1,association.id,association.principal_id) AS visible \
         FROM thread_messages AS association \
         JOIN messages AS message ON message.company_id=association.company_id AND message.id=association.message_id \
         WHERE association.company_id=$1 AND association.id=$2 AND association.principal_id IS NOT NULL \
           AND association.entry_kind='conversation' AND message.direction='inbound'"
    ).bind(event.company_id).bind(event.source_id).fetch_optional(db).await?;
    Ok(row.map(|row| SourceSnapshot {
        inbox: Some(AddressTarget::Principal(PrincipalId::new(row.principal_id))),
        thread_id: Some(row.thread_id),
        run_id: None,
        correlation_id: Some(row.correlation_id),
        responsible_principal_id: Some(row.principal_id),
        lifecycle: if row.visible {
            SourceLifecycle::Active
        } else {
            SourceLifecycle::Withdrawn
        },
        target: NotificationTarget::DirectThread {
            principal_id: PrincipalId::new(row.principal_id),
            thread_id: row.thread_id,
        },
    }))
}

/// Called after the existing account/company projection prefix. Never observes identities.
pub(super) async fn lock_source_on(
    db: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<()> {
    sqlx::query(
        "SELECT target.id FROM thread_messages AS association \
         JOIN principals AS target ON target.company_id=association.company_id AND target.id=association.principal_id \
         WHERE association.company_id=$1 AND association.id=$2 FOR SHARE OF target"
    ).bind(event.company_id).bind(event.source_id).fetch_all(&mut *db).await?;
    sqlx::query(
        "SELECT association.id FROM thread_messages AS association \
         JOIN messages AS message ON message.company_id=association.company_id AND message.id=association.message_id \
         WHERE association.company_id=$1 AND association.id=$2 FOR SHARE OF association,message"
    ).bind(event.company_id).bind(event.source_id).fetch_all(&mut *db).await?;
    Ok(())
}
