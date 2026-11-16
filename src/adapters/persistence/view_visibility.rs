//! Final viewer/stamp statements: all payload queries must finish before these are called.
use super::participant;
use crate::app_error::{AppError, AppResult};
use crate::application::{knowledge::PayloadVisibility, workflow::WorkflowActor};
use crate::domain::workflow::CompanyId;
use crate::entities::access_level::AccessLevel;
use sqlx::Postgres;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct VisibilityRow {
    id: Uuid,
    content_level: i16,
    membership: Option<String>,
    access_level: Option<i16>,
    is_owner: bool,
}

fn levels(rows: Vec<VisibilityRow>) -> AppResult<HashMap<Uuid, PayloadVisibility>> {
    rows.into_iter()
        .map(|row| {
            let membership = participant::membership_from_db(row.membership.as_deref())?;
            if membership.is_owner() != row.is_owner {
                return Err(AppError::Internal(
                    "Inconsistent stored viewer ownership".into(),
                ));
            }
            let viewer = participant::reader_level(row.id, membership, row.access_level)?;
            let content = AccessLevel::try_from(row.content_level)
                .map_err(|error| AppError::Internal(error.into()))?;
            Ok((row.id, PayloadVisibility { viewer, content }))
        })
        .collect()
}

/// One MVCC query reads each current run stamp jointly with the current membership/level.
/// Monotone run classification covers all earlier-loaded payloads, including historical entries.
pub(super) async fn run_levels_on<'e>(
    db: impl sqlx::Executor<'e, Database = Postgres>,
    company: CompanyId,
    viewer: WorkflowActor,
    runs: &[Uuid],
) -> AppResult<HashMap<Uuid, PayloadVisibility>> {
    let rows = sqlx::query_as::<_, VisibilityRow>(
        r#"
        SELECT run.id, run.content_level::smallint AS content_level,
               member.role AS membership, member.access_level::smallint AS access_level,
               company.user_id = account.id AS is_owner
        FROM workflow_runs AS run
        JOIN companies AS company ON company.id = run.company_id
        JOIN users AS account ON account.id = $3
        LEFT JOIN LATERAL membership_of(account.id, run.company_id) AS member ON true
        WHERE run.company_id = $1 AND run.id = ANY($2)
    "#,
    )
    .bind(company.as_uuid())
    .bind(runs)
    .bind(viewer.user_id())
    .fetch_all(db)
    .await?;
    levels(rows)
}

/// Delivery pages expose message data; the worker's transport reads are separate ports.
pub(super) async fn message_levels_on<'e>(
    db: impl sqlx::Executor<'e, Database = Postgres>,
    company: CompanyId,
    viewer: WorkflowActor,
    messages: &[Uuid],
) -> AppResult<HashMap<Uuid, PayloadVisibility>> {
    let rows = sqlx::query_as::<_, VisibilityRow>(
        r#"
        SELECT message.id, message.access_level::smallint AS content_level,
               member.role AS membership, member.access_level::smallint AS access_level,
               company.user_id = account.id AS is_owner
        FROM messages AS message
        JOIN companies AS company ON company.id = message.company_id
        JOIN users AS account ON account.id = $3
        LEFT JOIN LATERAL membership_of(account.id, message.company_id) AS member ON true
        WHERE message.company_id = $1 AND message.id = ANY($2)
    "#,
    )
    .bind(company.as_uuid())
    .bind(messages)
    .bind(viewer.user_id())
    .fetch_all(db)
    .await?;
    levels(rows)
}

/// Recheck the exact inbox and content ceiling together after delivery parts/diagnostics load.
pub(super) async fn delivery_levels_on<'e>(
    db: impl sqlx::Executor<'e, Database = Postgres>,
    company: CompanyId,
    viewer: WorkflowActor,
    deliveries: &[Uuid],
) -> AppResult<HashMap<Uuid, PayloadVisibility>> {
    let rows = sqlx::query_as::<_, VisibilityRow>(
        r#"SELECT delivery.id, message.access_level::smallint AS content_level,
                  member.role AS membership, member.access_level::smallint AS access_level,
                  company.user_id=account.id AS is_owner
           FROM message_deliveries AS delivery
           JOIN messages AS message ON (message.company_id,message.id)=(delivery.company_id,delivery.message_id)
           JOIN companies AS company ON company.id=delivery.company_id
           JOIN users AS account ON account.id=$3
           LEFT JOIN LATERAL membership_of(account.id, delivery.company_id) AS member ON true
           LEFT JOIN principals AS principal ON principal.company_id=delivery.company_id
               AND principal.user_id=account.id AND principal.kind='person'
           WHERE delivery.company_id=$1 AND delivery.id=ANY($2)
               AND (company.user_id=account.id OR member.user_id IS NOT NULL)
               AND (delivery.principal_id IS NULL OR principal_direct_can_view(delivery.company_id,delivery.principal_id,principal.id))"#,
    )
    .bind(company.as_uuid())
    .bind(deliveries)
    .bind(viewer.user_id())
    .fetch_all(db)
    .await?;
    levels(rows)
}
