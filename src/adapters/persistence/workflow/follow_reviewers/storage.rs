use super::*;

pub(super) async fn load_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: Uuid,
) -> AppResult<FollowDecision> {
    sqlx::query_as("SELECT company_id,decision_id,run_id,root_run_id,manifest_id,version_id,step,context_department_id,selectors,assignment_revision \
        FROM workflow_follow_decisions WHERE company_id=$1 AND decision_id=$2")
        .bind(company.as_uuid()).bind(decision).fetch_optional(db).await?.ok_or_else(missing)
}

pub(super) async fn assignment_on(
    db: &mut PgConnection,
    current: &FollowDecision,
) -> AppResult<ReviewerAssignment> {
    let value:Value=sqlx::query_scalar("SELECT content FROM workflow_reviewer_assignments WHERE company_id=$1 AND decision_id=$2 AND assignment_revision=$3")
        .bind(current.company_id).bind(current.decision_id).bind(current.assignment_revision).fetch_one(db).await?;
    ReviewerAssignment::decode(&serde_json::to_vec(&value).map_err(|_| invalid())?)
}

#[derive(sqlx::FromRow)]
struct ReviewerLabel {
    principal_id: Uuid,
    reviewer_label: String,
}

pub(super) async fn reviewers_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    revision: ReviewerAssignmentRevision,
    principals: &BTreeSet<PrincipalId>,
    previous: Option<&ReviewerAssignment>,
) -> AppResult<Vec<AssignmentReviewer>> {
    let mut reviewers: Vec<_> = previous
        .into_iter()
        .flat_map(|assignment| &assignment.reviewers)
        .filter(|reviewer| principals.contains(&reviewer.principal_id))
        .cloned()
        .collect();
    let new: Vec<_> = principals
        .iter()
        .filter(|id| {
            !reviewers
                .iter()
                .any(|reviewer| reviewer.principal_id == **id)
        })
        .map(|id| id.as_uuid())
        .collect();
    let labels: Vec<ReviewerLabel> = sqlx::query_as(
        "SELECT id AS principal_id,left(display_label,256) AS reviewer_label FROM principals \
        WHERE company_id=$1 AND id=ANY($2) AND kind='person' ORDER BY id FOR SHARE",
    )
    .bind(current.company_id)
    .bind(&new)
    .fetch_all(db)
    .await?;
    if labels.len() != new.len() {
        return Err(invalid());
    }
    for label in labels {
        let ordinal = (0..registry::MAX_REVIEWERS)
            .find(|ordinal| {
                !reviewers
                    .iter()
                    .any(|reviewer| usize::from(reviewer.ordinal) == *ordinal)
            })
            .ok_or_else(invalid)?;
        reviewers.push(AssignmentReviewer {
            principal_id: PrincipalId::new(label.principal_id),
            ordinal: u8::try_from(ordinal).map_err(|_| invalid())?,
            reviewer_label: label.reviewer_label,
            notice_event_id: NotificationEventId::random(),
            notice_assignment_revision: revision,
        });
    }
    reviewers.sort_unstable_by_key(|reviewer| reviewer.ordinal);
    Ok(reviewers)
}

pub(super) async fn persist_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    assignment: &ReviewerAssignment,
    previous: Option<&ReviewerAssignment>,
) -> AppResult<()> {
    let revision = i64::try_from(assignment.assignment_revision.get()).map_err(|_| limit())?;
    let content: Value =
        serde_json::from_slice(&assignment.canonical_content()?).map_err(|_| invalid())?;
    if previous.is_some() {
        sqlx::query("UPDATE workflow_follow_decisions SET assignment_revision=$3 WHERE company_id=$1 AND decision_id=$2")
            .bind(current.company_id).bind(current.decision_id).bind(revision).execute(&mut *db).await?;
    }
    sqlx::query("INSERT INTO workflow_reviewer_assignments(company_id,decision_id,assignment_revision,content) VALUES($1,$2,$3,$4)")
        .bind(current.company_id).bind(current.decision_id).bind(revision).bind(&content).execute(&mut *db).await?;
    let principals: Vec<_> = assignment
        .reviewers
        .iter()
        .map(|reviewer| reviewer.principal_id.as_uuid())
        .collect();
    withdraw_on(db, current.company(), current.decision_id, &principals).await?;
    sqlx::query("DELETE FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 AND NOT (principal_id=ANY($3))")
        .bind(current.company_id).bind(current.decision_id).bind(&principals).execute(&mut *db).await?;
    for reviewer in &assignment.reviewers {
        if reviewer.notice_assignment_revision != assignment.assignment_revision {
            continue;
        }
        insert_reviewer_on(db, current, reviewer).await?;
    }
    Ok(())
}

async fn insert_reviewer_on(
    db: &mut PgConnection,
    current: &FollowDecision,
    reviewer: &AssignmentReviewer,
) -> AppResult<()> {
    let source = Uuid::new_v4();
    let revision = i64::try_from(reviewer.notice_assignment_revision.get()).map_err(|_| limit())?;
    sqlx::query("INSERT INTO notification_events(id,company_id,source_kind,source_id,action_kind,source_generation) VALUES($1,$2,'decision_reviewer',$3,'decision_review',$4)")
        .bind(reviewer.notice_event_id.as_uuid()).bind(current.company_id).bind(source).bind(revision).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_reviewer_notices(company_id,id,decision_id,principal_id,assignment_revision,event_id) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(current.company_id).bind(source).bind(current.decision_id).bind(reviewer.principal_id.as_uuid()).bind(revision)
        .bind(reviewer.notice_event_id.as_uuid()).execute(&mut *db).await?;
    sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,ordinal,principal_id,notice_event_id,notice_assignment_revision) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(current.company_id).bind(current.decision_id).bind(i16::from(reviewer.ordinal)).bind(reviewer.principal_id.as_uuid())
        .bind(reviewer.notice_event_id.as_uuid()).bind(revision).execute(db).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct WithdrawnNotice {
    id: Uuid,
    email_delivery_id: Option<Uuid>,
}

/// Only current proofs are considered, so the operation never scans retained assignments.
pub(in crate::adapters::persistence::workflow) async fn withdraw_on(
    db: &mut PgConnection,
    company: CompanyId,
    decision: Uuid,
    remaining: &[Uuid],
) -> AppResult<()> {
    let notices:Vec<WithdrawnNotice>=sqlx::query_as(
        "SELECT notification.id,notification.email_delivery_id FROM workflow_decision_reviewers AS reviewer \
         JOIN workflow_reviewer_notices AS source ON source.company_id=reviewer.company_id AND source.decision_id=reviewer.decision_id \
           AND source.principal_id=reviewer.principal_id AND source.event_id=reviewer.notice_event_id \
           AND source.assignment_revision=reviewer.notice_assignment_revision \
         JOIN notifications AS notification ON notification.company_id=source.company_id AND notification.event_id=source.event_id \
         WHERE reviewer.company_id=$1 AND reviewer.decision_id=$2 AND NOT (reviewer.principal_id=ANY($3)) \
         ORDER BY notification.id FOR UPDATE OF notification")
        .bind(company.as_uuid()).bind(decision).bind(remaining).fetch_all(&mut *db).await?;
    let ids: Vec<_> = notices.iter().map(|notice| notice.id).collect();
    let deliveries: Vec<_> = notices
        .iter()
        .filter_map(|notice| notice.email_delivery_id)
        .collect();
    sqlx::query("UPDATE notifications SET state='withdrawn',state_changed_at=clock_timestamp(),updated_at=clock_timestamp() WHERE company_id=$1 AND id=ANY($2) AND state='active'")
        .bind(company.as_uuid()).bind(&ids).execute(&mut *db).await?;
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
