//! Information sources have no reviewer authority. Recipient work is deferred until the
//! notification worker can own its sorted account prefix before company/run/decision locks.
use super::*;

#[derive(sqlx::FromRow)]
struct Withdrawn {
    id: Uuid,
    email_delivery_id: Option<Uuid>,
}

pub(super) async fn refresh_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: Uuid,
) -> AppResult<()> {
    let members: Vec<Uuid> = sqlx::query_scalar(
        "SELECT principal_id FROM workflow_decision_information_members($1,$2) ORDER BY principal_id",
    ).bind(company.as_uuid()).bind(decision).fetch_all(&mut *db).await?;
    let removed: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE workflow_decision_information_notices SET active=false \
         WHERE company_id=$1 AND decision_id=$2 AND active AND NOT(principal_id=ANY($3)) RETURNING id",
    ).bind(company.as_uuid()).bind(decision).bind(&members).fetch_all(&mut *db).await?;
    withdraw_on(db, company, &removed).await?;
    let revision: i64 = sqlx::query_scalar(
        "SELECT coalesce((SELECT assignment_revision FROM workflow_follow_decisions \
         WHERE company_id=$1 AND decision_id=$2),0)",
    )
    .bind(company.as_uuid())
    .bind(decision)
    .fetch_one(&mut *db)
    .await?;
    for principal in members {
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workflow_decision_information_notices \
             WHERE company_id=$1 AND decision_id=$2 AND principal_id=$3 AND active)",
        )
        .bind(company.as_uuid())
        .bind(decision)
        .bind(principal)
        .fetch_one(&mut *db)
        .await?;
        if current {
            continue;
        }
        let source = Uuid::new_v4();
        let event = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO notification_events(id,company_id,source_kind,source_id,action_kind,source_generation) \
             VALUES($1,$2,'decision_information',$3,'decision_information',$4)",
        ).bind(event).bind(company.as_uuid()).bind(source).bind(revision.max(1)).execute(&mut *db).await?;
        sqlx::query(
            "INSERT INTO workflow_decision_information_notices \
             (company_id,id,decision_id,principal_id,assignment_revision,event_id) VALUES($1,$2,$3,$4,$5,$6)",
        ).bind(company.as_uuid()).bind(source).bind(decision).bind(principal).bind(revision).bind(event)
            .execute(&mut *db).await?;
    }
    Ok(())
}

async fn withdraw_on(db: &mut PgConnection, company: CompanyId, sources: &[Uuid]) -> AppResult<()> {
    if sources.is_empty() {
        return Ok(());
    }
    let notices: Vec<Withdrawn> = sqlx::query_as(
        "SELECT notification.id,notification.email_delivery_id FROM workflow_decision_information_notices AS source \
         JOIN notifications AS notification ON notification.company_id=source.company_id AND notification.event_id=source.event_id \
         WHERE source.company_id=$1 AND source.id=ANY($2) ORDER BY notification.id FOR UPDATE OF notification",
    ).bind(company.as_uuid()).bind(sources).fetch_all(&mut *db).await?;
    let ids: Vec<_> = notices.iter().map(|notice| notice.id).collect();
    let deliveries: Vec<_> = notices
        .iter()
        .filter_map(|notice| notice.email_delivery_id)
        .collect();
    sqlx::query("UPDATE notifications SET state='withdrawn',state_changed_at=clock_timestamp(),updated_at=clock_timestamp() \
        WHERE company_id=$1 AND id=ANY($2) AND state='active'")
        .bind(company.as_uuid()).bind(ids).execute(&mut *db).await?;
    Box::pin(
        crate::adapters::persistence::delivery::cancellation::cancel_deliveries_on(
            db,
            company.as_uuid(),
            &deliveries,
            crate::entities::transport::DeliveryCancellationReason::RecipientRevoked,
        ),
    )
    .await?;
    Ok(())
}
