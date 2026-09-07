use super::*;

pub(super) async fn execute_command(
    persistence: &PostgresPersistence,
    command: ReviewCommand,
) -> AppResult<ReviewCommandResult> {
    let fingerprint = command.fingerprint();
    let mut tx = persistence.pool().begin().await.map_err(AppError::from)?;
    let mut lock_bytes = [0_u8; 8];
    lock_bytes.copy_from_slice(&command.command_id.as_bytes()[..8]);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(i64::from_be_bytes(lock_bytes))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
    if let Some(result) = replayed_command(&mut tx, &command, &fingerprint).await? {
        tx.commit().await.map_err(AppError::from)?;
        return Ok(result);
    }
    let version = i32::try_from(command.expected_draft_version)
        .map_err(|_| AppError::BadRequest("Draft version is out of range.".into()))?;
    let current: Option<(String, String, Uuid, Uuid, DateTime<Utc>, serde_json::Value)> =
        sqlx::query_as(
            r#"SELECT draft.status, review.status, draft.channel_id,
                      review.reviewer_principal_id, review.expires_at,
                      draft.publication_snapshot
               FROM response_drafts AS draft
               JOIN response_reviews AS review
                 ON (review.company_id, review.draft_id, review.draft_version) =
                    (draft.company_id, draft.id, draft.version)
               WHERE draft.company_id = $1 AND draft.id = $2 AND draft.version = $3
                 AND draft.version = (
                     SELECT MAX(latest.version) FROM response_drafts AS latest
                     WHERE latest.company_id = $1 AND latest.id = $2
                 )
               FOR UPDATE OF draft, review"#,
        )
        .bind(command.company_id)
        .bind(command.draft_id.as_uuid())
        .bind(version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
    let Some((draft_status, review_status, channel_id, assigned_reviewer, expires_at, snapshot)) =
        current
    else {
        return Err(AppError::Conflict(
            "The draft version is stale; refresh and try again.".into(),
        ));
    };
    if draft_status != "pending_review" || review_status != "pending" {
        return Err(AppError::Conflict(
            "This draft is no longer pending review.".into(),
        ));
    }
    if expires_at <= Utc::now() {
        expire_locked_review(&mut tx, &command).await?;
        tx.commit().await.map_err(AppError::from)?;
        return Err(AppError::Conflict("This review has expired.".into()));
    }
    let assigned = PrincipalId::new(assigned_reviewer);
    let actor_is_assigned = assigned == command.actor_principal_id;
    let actor_is_owner =
        company_owner_is(&mut tx, command.company_id, command.actor_principal_id).await?;
    if !(actor_is_assigned
        || matches!(command.action, ReviewAction::Reassign { .. }) && actor_is_owner)
    {
        return Err(AppError::NotFound("Response review not found.".into()));
    }
    if !reviewer_is_eligible_on(
        &mut tx,
        command.company_id,
        channel_id,
        command.actor_principal_id,
    )
    .await?
        && !actor_is_owner
    {
        return Err(AppError::Conflict(
            "The assigned reviewer is no longer eligible.".into(),
        ));
    }

    let result =
        match &command.action {
            ReviewAction::Approve { rationale } => {
                approve_on(&mut tx, &command, snapshot, rationale.as_deref()).await?
            }
            ReviewAction::Reject { feedback } => {
                reject_on(&mut tx, &command, feedback.trim()).await?;
                ReviewCommandResult {
                    draft_id: command.draft_id,
                    draft_version: command.expected_draft_version,
                    published_message_id: None,
                    delivery: None,
                }
            }
            ReviewAction::Reassign {
                reviewer_principal_id,
            } => {
                if !reviewer_is_eligible_on(
                    &mut tx,
                    command.company_id,
                    channel_id,
                    *reviewer_principal_id,
                )
                .await?
                {
                    return Err(AppError::BadRequest(
                        "The selected reviewer cannot view this channel.".into(),
                    ));
                }
                reassign_on(&mut tx, &command, *reviewer_principal_id).await?;
                ReviewCommandResult {
                    draft_id: command.draft_id,
                    draft_version: command.expected_draft_version,
                    published_message_id: None,
                    delivery: None,
                }
            }
            ReviewAction::Edit { replacement } => {
                sqlx::query(
                    r#"UPDATE response_drafts SET status = 'superseded',
                          updated_by_principal_id = $4, updated_at = CURRENT_TIMESTAMP
                   WHERE company_id = $1 AND id = $2 AND version = $3"#,
                )
                .bind(command.company_id)
                .bind(command.draft_id.as_uuid())
                .bind(version)
                .bind(command.actor_principal_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(AppError::from)?;
                sqlx::query(
                r#"UPDATE response_reviews SET status = 'superseded', updated_at = CURRENT_TIMESTAMP
                   WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3"#,
            )
            .bind(command.company_id).bind(command.draft_id.as_uuid()).bind(version)
            .execute(&mut *tx).await.map_err(AppError::from)?;
                create_review_draft_on(&mut tx, replacement, Some(assigned)).await?;
                ReviewCommandResult {
                    draft_id: command.draft_id,
                    draft_version: replacement.version,
                    published_message_id: None,
                    delivery: None,
                }
            }
        };
    record_command(&mut tx, &command, &fingerprint, &result).await?;
    tx.commit().await.map_err(AppError::from)?;
    Ok(result)
}

async fn replayed_command(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
    fingerprint: &str,
) -> AppResult<Option<ReviewCommandResult>> {
    let prior = sqlx::query_as::<_, ReviewCommandDb>(
        r#"SELECT draft_id, resulting_draft_version, command_fingerprint,
                  published_message_id, published_delivery_id, published_delivery_created
           FROM response_review_commands
           WHERE company_id = $1 AND command_id = $2 FOR UPDATE"#,
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let Some(prior) = prior else {
        return Ok(None);
    };
    if prior.command_fingerprint != fingerprint || prior.draft_id != command.draft_id.as_uuid() {
        return Err(AppError::Conflict(
            "This command UUID was already used for another review action.".into(),
        ));
    }
    Ok(Some(ReviewCommandResult {
        draft_id: ResponseDraftId::new(prior.draft_id),
        draft_version: u32::try_from(prior.resulting_draft_version)
            .map_err(|_| AppError::Internal("Invalid command result version".into()))?,
        published_message_id: prior.published_message_id.map(CanonicalMessageId::new),
        delivery: prior
            .published_delivery_id
            .zip(prior.published_delivery_created)
            .map(|(id, created)| {
                if created {
                    crate::transport::DeliveryCreation::Created(
                        crate::entities::transport::DeliveryId::new(id),
                    )
                } else {
                    crate::transport::DeliveryCreation::Absorbed(
                        crate::entities::transport::DeliveryId::new(id),
                    )
                }
            }),
    }))
}

async fn approve_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
    snapshot: serde_json::Value,
    rationale: Option<&str>,
) -> AppResult<ReviewCommandResult> {
    let publication: DraftPublicationSnapshot = decode_json(snapshot, "draft publication")?;
    let message = publication.message();
    let delivery = publication.delivery();
    if message.id != delivery.message_id || delivery.company_id != command.company_id {
        return Err(AppError::Internal(
            "Stored draft publication scope is inconsistent.".into(),
        ));
    }
    let stored = insert_message_on(tx, message).await?;
    for &thread_id in publication.also_in_threads() {
        crate::adapters::persistence::thread::associate_message_on(
            tx,
            thread_id,
            stored.canonical_id,
            message.entry_kind,
        )
        .await?;
    }
    let delivery_creation = insert_delivery_on(tx, delivery).await?;
    let delivery_id = delivery_creation.delivery_id();
    let version = i32::try_from(command.expected_draft_version).unwrap_or(i32::MAX);
    sqlx::query(
        r#"UPDATE response_drafts SET status = 'published', updated_by_principal_id = $4,
                  updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE response_reviews SET status = 'published', reviewer_rationale = $4,
                  decided_by_principal_id = $5, updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(rationale.map(str::trim).filter(|value| !value.is_empty()))
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"INSERT INTO response_draft_publications (
               company_id, draft_id, draft_version, message_id, delivery_id,
               published_by_principal_id
           ) VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(stored.canonical_id.as_uuid())
    .bind(delivery_id.as_uuid())
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE background_tasks AS task
              SET status = 'completed', transition_reason = 'approval_accepted',
                  transition_actor_kind = 'human', transition_actor_id = $4,
                  transition_approval_id = NULL, transition_outreach_id = NULL,
                  worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                  lock_expires_at = NULL, wait_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
             FROM response_drafts AS draft
            WHERE (draft.company_id, draft.id, draft.version) = ($1, $2, $3)
              AND task.company_id = draft.company_id AND task.id = draft.task_id
              AND task.status = 'pending_approval'"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(ReviewCommandResult {
        draft_id: command.draft_id,
        draft_version: command.expected_draft_version,
        published_message_id: Some(stored.canonical_id),
        delivery: Some(delivery_creation),
    })
}

async fn reject_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
    feedback: &str,
) -> AppResult<()> {
    let version = i32::try_from(command.expected_draft_version).unwrap_or(i32::MAX);
    sqlx::query(
        r#"UPDATE response_drafts SET status = 'rejected', updated_by_principal_id = $4,
                  updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE response_reviews SET status = 'rejected', feedback = $4,
                  decided_by_principal_id = $5, updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(feedback)
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE background_tasks AS task
              SET status = 'pending', transition_reason = 'approval_rejected',
                  transition_actor_kind = 'human', transition_actor_id = $4,
                  transition_approval_id = NULL, transition_outreach_id = NULL,
                  worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                  lock_expires_at = NULL, wait_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
             FROM response_drafts AS draft
            WHERE (draft.company_id, draft.id, draft.version) = ($1, $2, $3)
              AND task.company_id = draft.company_id AND task.id = draft.task_id
              AND task.status = 'pending_approval'"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn reassign_on(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
    reviewer: PrincipalId,
) -> AppResult<()> {
    let version = i32::try_from(command.expected_draft_version).unwrap_or(i32::MAX);
    sqlx::query(
        r#"UPDATE response_reviews SET reviewer_principal_id = $4,
                  notification_actor_principal_id = $5,
                  updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(reviewer.as_uuid())
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE response_drafts SET reviewer_principal_id = $4,
                  updated_by_principal_id = $5, updated_at = CURRENT_TIMESTAMP
           WHERE company_id = $1 AND id = $2 AND version = $3"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .bind(reviewer.as_uuid())
    .bind(command.actor_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn expire_locked_review(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
) -> AppResult<()> {
    let version = i32::try_from(command.expected_draft_version).unwrap_or(i32::MAX);
    sqlx::query("UPDATE response_reviews SET status = 'expired', updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3")
        .bind(command.company_id).bind(command.draft_id.as_uuid()).bind(version)
        .execute(&mut **tx).await.map_err(AppError::from)?;
    sqlx::query("UPDATE response_drafts SET status = 'expired', updated_by_principal_id = $4, updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND id = $2 AND version = $3")
        .bind(command.company_id).bind(command.draft_id.as_uuid()).bind(version)
        .bind(command.actor_principal_id.as_uuid()).execute(&mut **tx).await.map_err(AppError::from)?;
    sqlx::query(
        r#"UPDATE background_tasks AS task
              SET status = 'pending', transition_reason = 'approval_rejected',
                  transition_actor_kind = 'system', transition_actor_id = NULL,
                  transition_approval_id = NULL, transition_outreach_id = NULL,
                  worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                  lock_expires_at = NULL, wait_expires_at = NULL,
                  updated_at = CURRENT_TIMESTAMP
             FROM response_drafts AS draft
            WHERE (draft.company_id, draft.id, draft.version) = ($1, $2, $3)
              AND task.company_id = draft.company_id AND task.id = draft.task_id
              AND task.status = 'pending_approval'"#,
    )
    .bind(command.company_id)
    .bind(command.draft_id.as_uuid())
    .bind(version)
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn company_owner_is(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    actor: PrincipalId,
) -> AppResult<bool> {
    sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM companies AS company
               JOIN principals AS principal
                 ON principal.company_id = company.id AND principal.user_id = company.user_id
               WHERE company.id = $1 AND principal.id = $2 AND principal.kind = 'person'
           )"#,
    )
    .bind(company_id)
    .bind(actor.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn record_command(
    tx: &mut Transaction<'_, Postgres>,
    command: &ReviewCommand,
    fingerprint: &str,
    result: &ReviewCommandResult,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO response_review_commands (
               company_id, command_id, draft_id, expected_draft_version, action,
               command_fingerprint, resulting_draft_version, published_message_id,
               published_delivery_id, published_delivery_created
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(command.company_id)
    .bind(command.command_id)
    .bind(command.draft_id.as_uuid())
    .bind(i32::try_from(command.expected_draft_version).unwrap_or(i32::MAX))
    .bind(command.action.name())
    .bind(fingerprint)
    .bind(i32::try_from(result.draft_version).unwrap_or(i32::MAX))
    .bind(result.published_message_id.map(CanonicalMessageId::as_uuid))
    .bind(
        result
            .delivery
            .map(|delivery| delivery.delivery_id().as_uuid()),
    )
    .bind(result.delivery.map(|delivery| delivery.was_created()))
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(())
}
