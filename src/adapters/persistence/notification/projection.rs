use super::*;
use crate::entities::notification::NotificationTarget;

#[derive(Debug)]
pub(super) struct SourceSnapshot {
    pub(super) channel_id: Option<Uuid>,
    pub(super) thread_id: Option<Uuid>,
    pub(super) run_id: Option<Uuid>,
    pub(super) correlation_id: Option<Uuid>,
    pub(super) responsible_principal_id: Option<Uuid>,
    pub(super) lifecycle: SourceLifecycle,
    pub(super) target: NotificationTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceLifecycle {
    Active,
    Resolved,
    Withdrawn,
}

#[derive(Debug, sqlx::FromRow)]
struct DeliverySourceRow {
    channel_id: Uuid,
    thread_id: Uuid,
    run_id: Uuid,
    correlation_id: Uuid,
    status: String,
    actor_principal_id: Option<Uuid>,
}

pub(super) async fn resolve_projection_on(
    connection: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<NotificationProjection> {
    if event.source_kind == NotificationSourceKind::EscalationControl {
        return Ok(NotificationProjection {
            event: event.clone(),
            disposition: NotificationDisposition::Control,
        });
    }
    let Some(source) = source_snapshot(connection, event).await? else {
        return Ok(NotificationProjection {
            event: event.clone(),
            disposition: NotificationDisposition::Withdrawn,
        });
    };
    let newer_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1 FROM notification_events AS newer
                WHERE newer.company_id = $1 AND newer.source_kind = $2
                  AND newer.source_id = $3 AND newer.action_kind = $4
                  AND newer.source_generation > $5
           )"#,
    )
    .bind(event.company_id)
    .bind(event.source_kind.as_str())
    .bind(event.source_id)
    .bind(event.action_kind.as_str())
    .bind(i64::try_from(event.source_generation).unwrap_or(i64::MAX))
    .fetch_one(&mut *connection)
    .await
    .map_err(AppError::from)?;
    let disposition = if newer_exists {
        NotificationDisposition::Withdrawn
    } else {
        match source.lifecycle {
            SourceLifecycle::Resolved => NotificationDisposition::Resolved,
            SourceLifecycle::Withdrawn => NotificationDisposition::Withdrawn,
            SourceLifecycle::Active => match source.responsible_principal_id {
                Some(principal_id) => recipient_for_source(connection, event, source, principal_id)
                    .await?
                    .map(Box::new)
                    .map(NotificationDisposition::Active)
                    .unwrap_or(NotificationDisposition::Withdrawn),
                None => NotificationDisposition::Withdrawn,
            },
        }
    };
    Ok(NotificationProjection {
        event: event.clone(),
        disposition,
    })
}

/// What a notification event is about (p08 D4.3). After B4 the only source is a workflow
/// delivery: it resolves to its run through the destination, and its run actor is responsible
/// while they are a person of the company. A delivery with no run (a person's mailbox send) is no
/// source at all, so it notifies nobody.
async fn source_snapshot(
    connection: &mut PgConnection,
    event: &NotificationEvent,
) -> AppResult<Option<SourceSnapshot>> {
    match (event.source_kind, event.action_kind) {
        (NotificationSourceKind::Delivery, NotificationActionKind::DeliveryFailure) => {
            let row = sqlx::query_as::<_, DeliverySourceRow>(
                r#"SELECT delivery.channel_id, destination.thread_id, run.id AS run_id,
                          delivery.correlation_id, delivery.status,
                          actor.id AS actor_principal_id
                     FROM message_deliveries AS delivery
                     JOIN workflow_message_destinations AS destination
                       ON destination.company_id = delivery.company_id
                      AND destination.delivery_id = delivery.id
                     JOIN workflow_runs AS run
                       ON run.company_id = destination.company_id AND run.id = destination.run_id
                     LEFT JOIN principals AS actor
                       ON actor.company_id = run.company_id AND actor.user_id = run.actor_id
                      AND actor.kind = 'person'
                    WHERE delivery.company_id = $1 AND delivery.id = $2
                    "#,
            )
            .bind(event.company_id)
            .bind(event.source_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(AppError::from)?;
            Ok(row.map(|row| SourceSnapshot {
                channel_id: Some(row.channel_id),
                thread_id: Some(row.thread_id),
                run_id: Some(row.run_id),
                correlation_id: Some(row.correlation_id),
                responsible_principal_id: row.actor_principal_id,
                target: NotificationTarget::DeliveryRecovery,
                lifecycle: if matches!(row.status.as_str(), "dead_letter" | "outcome_unknown") {
                    if row.actor_principal_id.is_some() {
                        SourceLifecycle::Active
                    } else {
                        SourceLifecycle::Withdrawn
                    }
                } else {
                    SourceLifecycle::Resolved
                },
            }))
        }
        (NotificationSourceKind::DecisionReviewer, NotificationActionKind::DecisionReview) => {
            super::reviewer_notice::source_on(connection, event).await
        }
        (NotificationSourceKind::EscalationNotice, NotificationActionKind::EscalationReview) => {
            super::escalation_notice::source_on(connection, event).await
        }
        _ => Err(AppError::BadRequest(
            "Invalid notification source/action pair".into(),
        )),
    }
}

async fn recipient_for_source(
    connection: &mut PgConnection,
    event: &NotificationEvent,
    source: SourceSnapshot,
    principal_id: Uuid,
) -> AppResult<Option<NotificationRecipient>> {
    #[derive(sqlx::FromRow)]
    struct RecipientRow {
        user_id: Uuid,
        email: String,
        company_label: String,
        channel_label: String,
        delivery_failure_email_enabled: bool,
        decision_email_enabled: bool,
    }
    let row = sqlx::query_as::<_, RecipientRow>(
        r#"SELECT principal.user_id, account.email,
                  company.name AS company_label, COALESCE(channel.name,'Company workflow') AS channel_label,
                  COALESCE(preference.delivery_failure_email_enabled, TRUE)
                      AS delivery_failure_email_enabled,
                  COALESCE(preference.decision_email_enabled, TRUE)
                      AS decision_email_enabled
             FROM principals AS principal
             JOIN users AS account ON account.id = principal.user_id
             JOIN company_members AS member
               ON member.company_id = principal.company_id
              AND member.user_id = principal.user_id
             JOIN companies AS company ON company.id = principal.company_id
             LEFT JOIN channels AS channel
               ON channel.company_id = principal.company_id AND channel.id = $3
             LEFT JOIN user_notification_preferences AS preference
               ON preference.user_id = account.id
            WHERE principal.company_id = $1 AND principal.id = $2
              AND principal.kind = 'person'
              AND notification_principal_can_view($1, $3, $2)
            "#,
    )
    .bind(event.company_id)
    .bind(principal_id)
    .bind(source.channel_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(AppError::from)?;
    Ok(row.map(|row| {
        let preferences = NotificationPreferences {
            delivery_failure_email_enabled: row.delivery_failure_email_enabled,
            decision_email_enabled: row.decision_email_enabled,
        };
        NotificationRecipient {
            principal_id: PrincipalId::new(principal_id),
            user_id: row.user_id,
            email: EmailAddress::from(row.email),
            company_label: row.company_label,
            channel_label: row.channel_label,
            channel_id: source.channel_id,
            thread_id: source.thread_id,
            run_id: source.run_id,
            correlation_id: source
                .correlation_id
                .map(Into::into)
                .unwrap_or_else(|| event.id.as_uuid().into()),
            email_enabled: event.action_kind.email_preference(preferences),
            target: source.target,
        }
    }))
}

pub(super) async fn apply_projection_on(
    tx: &mut Transaction<'_, Postgres>,
    projection: &NotificationProjection,
    email: Option<&NewStandaloneDelivery>,
) -> AppResult<NotificationProjectionResult> {
    if projection.disposition == NotificationDisposition::Control {
        return Err(AppError::BadRequest(
            "A control event cannot create a recipient notification".into(),
        ));
    }
    let state = match projection.disposition {
        NotificationDisposition::Active(_) => NotificationState::Withdrawn,
        NotificationDisposition::Resolved => NotificationState::Resolved,
        NotificationDisposition::Withdrawn => NotificationState::Withdrawn,
        NotificationDisposition::Control => {
            return Err(AppError::BadRequest(
                "A control event has no recipient state".into(),
            ));
        }
    };
    let active_identity = match &projection.disposition {
        NotificationDisposition::Active(recipient) => Some(recipient.user_id),
        NotificationDisposition::Resolved
        | NotificationDisposition::Withdrawn
        | NotificationDisposition::Control => None,
    };
    let settled: Vec<DateTime<Utc>> = sqlx::query_scalar(
        r#"UPDATE notifications
              SET state = $5, state_changed_at = CURRENT_TIMESTAMP,
                  updated_at = CURRENT_TIMESTAMP
            WHERE company_id = $1 AND source_kind = $2 AND source_id = $3
              AND action_kind = $4 AND state = 'active'
              AND ($6::uuid IS NULL OR recipient_user_id <> $6 OR source_generation <> $7)
            RETURNING created_at"#,
    )
    .bind(projection.event.company_id)
    .bind(projection.event.source_kind.as_str())
    .bind(projection.event.source_id)
    .bind(projection.event.action_kind.as_str())
    .bind(state.as_str())
    .bind(active_identity)
    .bind(i64::try_from(projection.event.source_generation).unwrap_or(i64::MAX))
    .fetch_all(&mut **tx)
    .await
    .map_err(AppError::from)?;
    let mut result = NotificationProjectionResult {
        resolved: if state == NotificationState::Resolved {
            settled.len() as u64
        } else {
            0
        },
        withdrawn: if state == NotificationState::Withdrawn {
            settled.len() as u64
        } else {
            0
        },
        action_latency_seconds: settled
            .into_iter()
            .min()
            .map(|created| (Utc::now() - created).num_milliseconds().max(0) as f64 / 1_000.0),
        ..Default::default()
    };

    let NotificationDisposition::Active(recipient) = &projection.disposition else {
        return Ok(result);
    };
    let inserted_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO notifications (
               id, company_id, recipient_user_id, recipient_principal_id, event_id,
               source_kind, source_id, action_kind, source_generation, channel_id
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           ON CONFLICT ON CONSTRAINT notifications_identity_key DO UPDATE
             SET recipient_principal_id = EXCLUDED.recipient_principal_id,
                 event_id = EXCLUDED.event_id, channel_id = EXCLUDED.channel_id,
                 state = 'active', state_changed_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
           RETURNING id"#,
    )
    .bind(projection.event.notification_id.as_uuid())
    .bind(projection.event.company_id)
    .bind(recipient.user_id)
    .bind(recipient.principal_id.as_uuid())
    .bind(projection.event.id.as_uuid())
    .bind(projection.event.source_kind.as_str())
    .bind(projection.event.source_id)
    .bind(projection.event.action_kind.as_str())
    .bind(i64::try_from(projection.event.source_generation).unwrap_or(i64::MAX))
    .bind(recipient.channel_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)?;
    result.created = 1;

    if projection.should_email() {
        let delivery = email.ok_or_else(|| {
            AppError::Internal("Enabled notification email was not composed".into())
        })?;
        let creation =
            crate::adapters::persistence::delivery::enqueue::insert_standalone_delivery_on(
                tx, delivery,
            )
            .await?;
        let delivery_id = match creation {
            DeliveryCreation::Created(id) | DeliveryCreation::Absorbed(id) => id,
        };
        sqlx::query(
            r#"UPDATE notifications SET email_delivery_id = $2, updated_at = CURRENT_TIMESTAMP
                WHERE id = $1 AND email_delivery_id IS NULL"#,
        )
        .bind(inserted_id)
        .bind(delivery_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    } else if email.is_some() {
        return Err(AppError::Internal(
            "A suppressed notification email was supplied for projection".into(),
        ));
    }
    Ok(result)
}

pub(super) fn source_href(event: &NotificationEvent, recipient: &NotificationRecipient) -> String {
    match recipient.target {
        NotificationTarget::DeliveryRecovery => {
            format!("/ui/deliveries?company_id={}", event.company_id)
        }
        NotificationTarget::WorkflowDecision { decision_id } => {
            format!("/ui/workflow-decisions/{decision_id}")
        }
        NotificationTarget::WorkflowEscalation { decision_id } => {
            format!("/ui/workflow-decisions/escalations/{decision_id}")
        }
    }
}
