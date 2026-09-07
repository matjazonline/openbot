use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    adapters::persistence::{
        PostgresPersistence, delivery::enqueue::insert_delivery_on, thread::insert_message_on,
    },
    app_error::{AppError, AppResult},
    entities::{
        message::{CanonicalMessageId, MessageAttachments},
        response_draft::{
            EvidenceAvailability, EvidenceSource, ResponseDraft, ResponseDraftId,
            ResponseDraftStatus, ResponseEvidence, ResponseReview, ResponseReviewDetail,
            ResponseReviewStatus,
        },
        transport::PrincipalId,
    },
    use_cases::response_review::{
        AgentReviewSubmission, DraftPublicationSnapshot, PreparedReviewDraft,
        ResponseReviewPersistence, ResponseReviewPolicy, ReviewAction, ReviewCommand,
        ReviewCommandResult,
    },
};

#[derive(sqlx::FromRow)]
struct DraftDb {
    id: Uuid,
    version: i32,
    company_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    task_id: Option<Uuid>,
    source_handoff_generation: Option<Uuid>,
    author_principal_id: Uuid,
    reviewer_principal_id: Uuid,
    proposed_message_id: Uuid,
    subject: String,
    body: String,
    attachment_snapshot: serde_json::Value,
    recipient_snapshot: serde_json::Value,
    transport_snapshot: serde_json::Value,
    status: String,
    created_by_principal_id: Uuid,
    updated_by_principal_id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

const DRAFT_COLUMNS: &str = r#"draft.id, draft.version, draft.company_id, draft.channel_id,
    draft.thread_id, draft.task_id, draft.source_handoff_generation,
    draft.author_principal_id, draft.reviewer_principal_id, draft.proposed_message_id,
    draft.subject, draft.body, draft.attachment_snapshot, draft.recipient_snapshot,
    draft.transport_snapshot, draft.status, draft.created_by_principal_id,
    draft.updated_by_principal_id, draft.created_at, draft.updated_at"#;

impl TryFrom<DraftDb> for ResponseDraft {
    type Error = AppError;

    fn try_from(db: DraftDb) -> AppResult<Self> {
        let attachments = serde_json::from_value::<MessageAttachments>(db.attachment_snapshot)
            .map_err(|error| {
                AppError::Internal(format!(
                    "Unreadable attachment snapshot for draft {} v{}: {error}",
                    db.id, db.version
                ))
            })?
            .into_items();
        Ok(Self {
            id: ResponseDraftId::new(db.id),
            version: u32::try_from(db.version)
                .map_err(|_| AppError::Internal("Invalid response draft version".into()))?,
            company_id: db.company_id,
            channel_id: db.channel_id,
            thread_id: db.thread_id,
            task_id: db.task_id,
            source_handoff_generation: db.source_handoff_generation,
            author_principal_id: PrincipalId::new(db.author_principal_id),
            reviewer_principal_id: PrincipalId::new(db.reviewer_principal_id),
            proposed_message_id: CanonicalMessageId::new(db.proposed_message_id),
            subject: db.subject,
            body: db.body,
            attachments,
            recipients: decode_json(db.recipient_snapshot, "draft recipient snapshot")?,
            transport: decode_json(db.transport_snapshot, "draft transport snapshot")?,
            status: ResponseDraftStatus::from_str(&db.status).map_err(AppError::Internal)?,
            created_by_principal_id: PrincipalId::new(db.created_by_principal_id),
            updated_by_principal_id: PrincipalId::new(db.updated_by_principal_id),
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ReviewDb {
    company_id: Uuid,
    draft_id: Uuid,
    draft_version: i32,
    reviewer_principal_id: Uuid,
    status: String,
    feedback: Option<String>,
    reviewer_rationale: Option<String>,
    decided_by_principal_id: Option<Uuid>,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ReviewDb> for ResponseReview {
    type Error = AppError;

    fn try_from(db: ReviewDb) -> AppResult<Self> {
        Ok(Self {
            company_id: db.company_id,
            draft_id: ResponseDraftId::new(db.draft_id),
            draft_version: u32::try_from(db.draft_version)
                .map_err(|_| AppError::Internal("Invalid response review version".into()))?,
            reviewer_principal_id: PrincipalId::new(db.reviewer_principal_id),
            status: ResponseReviewStatus::from_str(&db.status).map_err(AppError::Internal)?,
            feedback: db.feedback,
            reviewer_rationale: db.reviewer_rationale,
            decided_by_principal_id: db.decided_by_principal_id.map(PrincipalId::new),
            expires_at: db.expires_at,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct EvidenceDb {
    id: Uuid,
    source_reference: serde_json::Value,
    source_version: String,
    content_digest: String,
    audience: String,
    support: String,
}

#[derive(sqlx::FromRow)]
struct ReviewCommandDb {
    draft_id: Uuid,
    resulting_draft_version: i32,
    command_fingerprint: String,
    published_message_id: Option<Uuid>,
    published_delivery_id: Option<Uuid>,
    published_delivery_created: Option<bool>,
}

fn decode_json<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    label: &str,
) -> AppResult<T> {
    serde_json::from_value(value)
        .map_err(|error| AppError::Internal(format!("Unreadable {label}: {error}")))
}

/// Resolve the effective policy without copying it into a task payload or draft.
pub(crate) async fn effective_review_required_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<bool> {
    let policy: Option<String> = sqlx::query_scalar(
        r#"SELECT COALESCE(channel.external_response_review_override,
                           company.external_response_review)
           FROM channels AS channel
           JOIN companies AS company ON company.id = channel.company_id
           WHERE channel.company_id = $1 AND channel.id = $2"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    match policy.as_deref() {
        Some("autonomous") => Ok(false),
        Some("review_all_external") => Ok(true),
        Some(other) => Err(AppError::Internal(format!(
            "Invalid effective response review policy '{other}'"
        ))),
        None => Err(AppError::NotFound("Channel not found.".into())),
    }
}

pub(crate) async fn create_review_draft_on(
    tx: &mut Transaction<'_, Postgres>,
    draft: &PreparedReviewDraft,
    assigned_reviewer: Option<PrincipalId>,
) -> AppResult<PrincipalId> {
    let reviewer = match assigned_reviewer {
        Some(reviewer)
            if reviewer_is_eligible_on(tx, draft.company_id, draft.channel_id, reviewer)
                .await? =>
        {
            reviewer
        }
        Some(_) => {
            return Err(AppError::Conflict(
                "The selected reviewer is no longer eligible for this channel.".into(),
            ));
        }
        None => resolve_reviewer_on(tx, draft).await?.ok_or_else(|| {
            AppError::Conflict(
                "Review is required, but this channel has no eligible human reviewer.".into(),
            )
        })?,
    };
    validate_evidence_on(tx, draft).await?;
    if !draft.publication.also_in_threads().is_empty() {
        let matching: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM threads
               WHERE company_id = $1 AND id = ANY($2)"#,
        )
        .bind(draft.company_id)
        .bind(draft.publication.also_in_threads())
        .fetch_one(&mut **tx)
        .await
        .map_err(AppError::from)?;
        if usize::try_from(matching).ok() != Some(draft.publication.also_in_threads().len()) {
            return Err(AppError::BadRequest(
                "A draft references a thread outside its company.".into(),
            ));
        }
    }

    let publication = serde_json::to_value(&draft.publication)
        .map_err(|error| AppError::Internal(format!("Could not encode publication: {error}")))?;
    let attachments = serde_json::to_value(draft.attachment_snapshot())
        .map_err(|error| AppError::Internal(format!("Could not encode attachments: {error}")))?;
    let recipients = serde_json::to_value(&draft.recipients)
        .map_err(|error| AppError::Internal(format!("Could not encode recipients: {error}")))?;
    let transport = serde_json::to_value(draft.transport_snapshot())
        .map_err(|error| AppError::Internal(format!("Could not encode transport: {error}")))?;
    let message = draft.publication.message();

    sqlx::query(
        r#"INSERT INTO response_drafts (
               id, version, company_id, channel_id, thread_id, task_id,
               source_handoff_generation, author_principal_id, reviewer_principal_id,
               proposed_message_id, subject, body, attachment_snapshot, recipient_snapshot,
               transport_snapshot, publication_snapshot, status, created_by_principal_id,
               updated_by_principal_id
           ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16, 'pending_review', $17, $17)"#,
    )
    .bind(draft.id.as_uuid())
    .bind(
        i32::try_from(draft.version)
            .map_err(|_| AppError::BadRequest("Draft version is out of range.".into()))?,
    )
    .bind(draft.company_id)
    .bind(draft.channel_id)
    .bind(draft.thread_id)
    .bind(draft.task_id)
    .bind(draft.source_handoff_generation)
    .bind(draft.author_principal_id.as_uuid())
    .bind(reviewer.as_uuid())
    .bind(message.id.as_uuid())
    .bind(&message.subject)
    .bind(&message.clean_text_body)
    .bind(attachments)
    .bind(recipients)
    .bind(transport)
    .bind(publication)
    .bind(draft.created_by_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;

    for (position, evidence) in draft.evidence.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO response_draft_evidence (
                   company_id, draft_id, draft_version, id, position, source_reference,
                   source_version, content_digest, audience, support
               ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        )
        .bind(draft.company_id)
        .bind(draft.id.as_uuid())
        .bind(i32::try_from(draft.version).unwrap_or(i32::MAX))
        .bind(evidence.id)
        .bind(i32::try_from(position).unwrap_or(i32::MAX))
        .bind(serde_json::to_value(&evidence.source).map_err(|error| {
            AppError::Internal(format!("Could not encode evidence source: {error}"))
        })?)
        .bind(&evidence.source_version)
        .bind(&evidence.content_digest)
        .bind(evidence.audience.as_str())
        .bind(evidence.support.as_str())
        .execute(&mut **tx)
        .await
        .map_err(AppError::from)?;
    }
    // The review row seals the evidence collection. The database rejects evidence inserts after
    // this point, so every reviewer sees the same response-level provenance for this version.
    sqlx::query(
        r#"INSERT INTO response_reviews (
               company_id, draft_id, draft_version, reviewer_principal_id, expires_at,
               notification_actor_principal_id
           ) VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(draft.company_id)
    .bind(draft.id.as_uuid())
    .bind(i32::try_from(draft.version).unwrap_or(i32::MAX))
    .bind(reviewer.as_uuid())
    .bind(draft.expires_at)
    .bind(draft.created_by_principal_id.as_uuid())
    .execute(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(reviewer)
}

async fn resolve_reviewer_on(
    tx: &mut Transaction<'_, Postgres>,
    draft: &PreparedReviewDraft,
) -> AppResult<Option<PrincipalId>> {
    let reviewer: Option<Uuid> = sqlx::query_scalar(
        r#"WITH candidates AS (
               SELECT task.owner_principal_id AS principal_id, 1 AS priority
               FROM background_tasks AS task
               WHERE task.company_id = $1 AND task.id = $3
                 AND task.owner_principal_kind = 'person'
               UNION ALL
               SELECT channel.preferred_reviewer_principal_id, 2
               FROM channels AS channel
               WHERE channel.company_id = $1 AND channel.id = $2
               UNION ALL
               SELECT principal.id, 3
               FROM companies AS company
               JOIN principals AS principal
                 ON principal.company_id = company.id AND principal.user_id = company.user_id
               WHERE company.id = $1 AND principal.kind = 'person'
           )
           SELECT candidate.principal_id
           FROM candidates AS candidate
           JOIN principals AS principal
             ON principal.company_id = $1 AND principal.id = candidate.principal_id
           JOIN channels AS channel ON channel.company_id = $1 AND channel.id = $2
           JOIN companies AS company ON company.id = $1
           WHERE principal.kind = 'person'
             AND (principal.user_id = company.user_id
                  OR (EXISTS (
                          SELECT 1 FROM company_members AS member
                          WHERE member.company_id = $1 AND member.user_id = principal.user_id
                      ) AND (
                          channel.access_mode IN ('team', 'public')
                          OR EXISTS (
                              SELECT 1 FROM channel_principal_grants AS channel_grant
                              WHERE channel_grant.company_id = $1
                                AND channel_grant.channel_id = $2
                                AND channel_grant.principal_id = principal.id
                                AND channel_grant.capability = 'view'
                          )
                      )))
           ORDER BY candidate.priority
           LIMIT 1"#,
    )
    .bind(draft.company_id)
    .bind(draft.channel_id)
    .bind(draft.task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)?;
    Ok(reviewer.map(PrincipalId::new))
}

async fn reviewer_is_eligible_on(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
    channel_id: Uuid,
    reviewer: PrincipalId,
) -> AppResult<bool> {
    sqlx::query_scalar(
        r#"SELECT EXISTS (
               SELECT 1
               FROM principals AS principal
               JOIN companies AS company ON company.id = principal.company_id
               JOIN channels AS channel
                 ON channel.company_id = company.id AND channel.id = $2
               WHERE principal.company_id = $1 AND principal.id = $3
                 AND principal.kind = 'person'
                 AND (principal.user_id = company.user_id
                      OR (EXISTS (
                              SELECT 1 FROM company_members AS member
                              WHERE member.company_id = $1
                                AND member.user_id = principal.user_id
                          ) AND (
                              channel.access_mode IN ('team', 'public')
                              OR EXISTS (
                                  SELECT 1 FROM channel_principal_grants AS channel_grant
                                  WHERE channel_grant.company_id = $1
                                    AND channel_grant.channel_id = $2
                                    AND channel_grant.principal_id = $3
                                    AND channel_grant.capability = 'view'
                              )
                          )))
           )"#,
    )
    .bind(company_id)
    .bind(channel_id)
    .bind(reviewer.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn validate_evidence_on(
    tx: &mut Transaction<'_, Postgres>,
    draft: &PreparedReviewDraft,
) -> AppResult<()> {
    for evidence in &draft.evidence {
        if evidence.source_version.trim().is_empty()
            || evidence.source_version.len() > 256
            || evidence.content_digest.trim().is_empty()
            || evidence.content_digest.len() > 128
        {
            return Err(AppError::BadRequest(
                "Evidence version and digest must be non-empty and bounded.".into(),
            ));
        }
        let exists = match &evidence.source {
            EvidenceSource::Message {
                message_id,
                thread_id,
            } => sqlx::query_scalar(
                r#"SELECT EXISTS (
                           SELECT 1 FROM messages AS message
                           JOIN thread_messages AS association
                             ON association.company_id = message.company_id
                            AND association.message_id = message.id
                           WHERE message.company_id = $1 AND message.id = $2
                             AND association.thread_id = $3
                       )"#,
            )
            .bind(draft.company_id)
            .bind(message_id.as_uuid())
            .bind(thread_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?,
            EvidenceSource::Note { note_id } => sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM internal_notes WHERE company_id = $1 AND id = $2)",
            )
            .bind(draft.company_id)
            .bind(note_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?,
            EvidenceSource::Attachment {
                message_id,
                sha256_hash,
            } => sqlx::query_scalar(
                r#"SELECT EXISTS (
                           SELECT 1 FROM messages AS message,
                                jsonb_array_elements(message.attachments->'items') AS attachment
                           WHERE message.company_id = $1 AND message.id = $2
                             AND attachment->>'sha256_hash' = $3
                       )"#,
            )
            .bind(draft.company_id)
            .bind(message_id.as_uuid())
            .bind(sha256_hash)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?,
            EvidenceSource::DelegatedResult {
                task_id,
                execution_generation,
            } => sqlx::query_scalar(
                r#"SELECT EXISTS (
                           SELECT 1 FROM task_attempts AS attempt
                           JOIN background_tasks AS task ON task.id = attempt.task_id
                           WHERE task.company_id = $1 AND task.id = $2
                             AND attempt.execution_generation = $3
                       )"#,
            )
            .bind(draft.company_id)
            .bind(task_id)
            .bind(execution_generation)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?,
            EvidenceSource::RetainedToolResult { task_id, result_id } => sqlx::query_scalar(
                r#"SELECT EXISTS(
                       SELECT 1 FROM background_tasks
                       WHERE company_id = $1 AND id = $2
                         AND EXISTS (
                             SELECT 1
                             FROM jsonb_array_elements(
                                 COALESCE(payload->'retained_tool_results', '[]'::jsonb)
                             ) AS result
                             WHERE result->>'id' = $3::text
                         )
                   )"#,
            )
            .bind(draft.company_id)
            .bind(task_id)
            .bind(result_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(AppError::from)?,
            EvidenceSource::ExternalUrl { url, .. } => {
                url.starts_with("https://") || url.starts_with("http://")
            }
        };
        if !exists {
            return Err(AppError::BadRequest(
                "An evidence source does not exist in this company or has an invalid URL.".into(),
            ));
        }
    }
    Ok(())
}

#[async_trait]
impl ResponseReviewPersistence for PostgresPersistence {
    async fn submit_agent_review_if_required(
        &self,
        submission: AgentReviewSubmission,
    ) -> AppResult<bool> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        if !effective_review_required_on(&mut tx, submission.company_id, submission.channel_id)
            .await?
        {
            tx.rollback().await.map_err(AppError::from)?;
            return Ok(false);
        }
        let author: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT id FROM principals
               WHERE company_id = $1 AND agent_id = $2 AND kind = 'agent'"#,
        )
        .bind(submission.company_id)
        .bind(submission.agent_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(AppError::from)?;
        let author = author.map(PrincipalId::new).ok_or_else(|| {
            AppError::Conflict("The response author has no agent principal.".into())
        })?;
        let draft = PreparedReviewDraft::new(
            submission.id,
            1,
            submission.company_id,
            submission.channel_id,
            submission.thread_id,
            None,
            author,
            author,
            submission.recipients,
            submission.evidence,
            submission.publication,
        )?;
        create_review_draft_on(&mut tx, &draft, None).await?;
        tx.commit().await.map_err(AppError::from)?;
        Ok(true)
    }

    async fn review_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
    ) -> AppResult<Option<ResponseReviewPolicy>> {
        let row: Option<(String, Option<String>, Option<Uuid>)> = sqlx::query_as(
            r#"SELECT company.external_response_review,
                      channel.external_response_review_override,
                      channel.preferred_reviewer_principal_id
               FROM companies AS company
               JOIN channels AS channel ON channel.company_id = company.id
               WHERE company.id = $1 AND channel.id = $2"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        row.map(|(company, channel, reviewer)| {
            let company_default = company.parse().map_err(AppError::Internal)?;
            let channel_override = channel
                .map(|value| value.parse().map_err(AppError::Internal))
                .transpose()?;
            Ok(ResponseReviewPolicy {
                company_default,
                effective: channel_override.unwrap_or(company_default),
                channel_override,
                preferred_reviewer_principal_id: reviewer.map(PrincipalId::new),
            })
        })
        .transpose()
    }

    async fn set_company_review_policy(
        &self,
        company_id: Uuid,
        policy: crate::entities::response_draft::ExternalResponseReview,
    ) -> AppResult<()> {
        let changed =
            sqlx::query("UPDATE companies SET external_response_review = $2 WHERE id = $1")
                .bind(company_id)
                .bind(policy.as_str())
                .execute(&self.pool)
                .await
                .map_err(AppError::from)?;
        if changed.rows_affected() != 1 {
            return Err(AppError::NotFound("Company not found.".into()));
        }
        Ok(())
    }

    async fn set_channel_review_policy(
        &self,
        company_id: Uuid,
        channel_id: Uuid,
        policy_override: Option<crate::entities::response_draft::ExternalResponseReview>,
        preferred_reviewer_principal_id: Option<PrincipalId>,
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(AppError::from)?;
        if let Some(reviewer) = preferred_reviewer_principal_id
            && !reviewer_is_eligible_on(&mut tx, company_id, channel_id, reviewer).await?
        {
            return Err(AppError::BadRequest(
                "The preferred reviewer must be a person who can view this channel.".into(),
            ));
        }
        let changed = sqlx::query(
            r#"UPDATE channels
               SET external_response_review_override = $3,
                   preferred_reviewer_principal_id = $4
               WHERE company_id = $1 AND id = $2"#,
        )
        .bind(company_id)
        .bind(channel_id)
        .bind(policy_override.map(|policy| policy.as_str()))
        .bind(preferred_reviewer_principal_id.map(PrincipalId::as_uuid))
        .execute(&mut *tx)
        .await
        .map_err(AppError::from)?;
        if changed.rows_affected() != 1 {
            return Err(AppError::NotFound("Channel not found.".into()));
        }
        tx.commit().await.map_err(AppError::from)?;
        Ok(())
    }

    async fn get_for_reviewer(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<ResponseReviewDetail>> {
        expire_due(&self.pool, company_id, Some(draft_id)).await?;
        load_detail(&self.pool, company_id, draft_id, reviewer_principal_id).await
    }

    async fn list_pending_for_reviewer(
        &self,
        company_id: Uuid,
        reviewer_principal_id: PrincipalId,
        limit: usize,
    ) -> AppResult<Vec<ResponseReviewDetail>> {
        if !(1..=100).contains(&limit) {
            return Err(AppError::BadRequest(
                "Review list limit must be 1 through 100.".into(),
            ));
        }
        expire_due(&self.pool, company_id, None).await?;
        let ids: Vec<Uuid> = sqlx::query_scalar(
            r#"SELECT review.draft_id
               FROM response_reviews AS review
               JOIN response_drafts AS draft
                 ON (draft.company_id, draft.id, draft.version) =
                    (review.company_id, review.draft_id, review.draft_version)
               WHERE review.company_id = $1 AND review.reviewer_principal_id = $2
                 AND review.status = 'pending' AND draft.status = 'pending_review'
               ORDER BY review.created_at, review.draft_id
               LIMIT $3"#,
        )
        .bind(company_id)
        .bind(reviewer_principal_id.as_uuid())
        .bind(i64::try_from(limit).unwrap_or(100))
        .fetch_all(&self.pool)
        .await
        .map_err(AppError::from)?;
        let mut details = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(detail) = load_detail(
                &self.pool,
                company_id,
                ResponseDraftId::new(id),
                reviewer_principal_id,
            )
            .await?
            {
                details.push(detail);
            }
        }
        Ok(details)
    }

    async fn publication_for_reviewer(
        &self,
        company_id: Uuid,
        draft_id: ResponseDraftId,
        draft_version: u32,
        reviewer_principal_id: PrincipalId,
    ) -> AppResult<Option<DraftPublicationSnapshot>> {
        let stored: Option<(serde_json::Value, Uuid)> = sqlx::query_as(
            r#"SELECT publication_snapshot, channel_id
               FROM response_drafts
               WHERE company_id = $1 AND id = $2 AND version = $3
                 AND reviewer_principal_id = $4 AND status = 'pending_review'
                 AND version = (SELECT MAX(latest.version) FROM response_drafts AS latest
                                WHERE latest.company_id = $1 AND latest.id = $2)"#,
        )
        .bind(company_id)
        .bind(draft_id.as_uuid())
        .bind(
            i32::try_from(draft_version)
                .map_err(|_| AppError::BadRequest("Draft version is out of range.".into()))?,
        )
        .bind(reviewer_principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(AppError::from)?;
        let Some((snapshot, channel_id)) = stored else {
            return Ok(None);
        };
        if !reviewer_is_eligible_pool(&self.pool, company_id, channel_id, reviewer_principal_id)
            .await?
        {
            return Ok(None);
        }
        Ok(Some(decode_json(snapshot, "draft publication")?))
    }

    async fn execute_review_command(
        &self,
        command: ReviewCommand,
    ) -> AppResult<ReviewCommandResult> {
        command.validate()?;
        execute_command(self, command).await
    }
}

async fn expire_due(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    draft_id: Option<ResponseDraftId>,
) -> AppResult<()> {
    sqlx::query(
        r#"WITH expired AS (
               UPDATE response_reviews AS review
                  SET status = 'expired', updated_at = CURRENT_TIMESTAMP
                WHERE review.company_id = $1 AND review.status = 'pending'
                  AND review.expires_at <= CURRENT_TIMESTAMP
                  AND ($2::uuid IS NULL OR review.draft_id = $2)
               RETURNING review.draft_id, review.draft_version
           ), reset_tasks AS (
               UPDATE background_tasks AS task
                  SET status = 'pending', transition_reason = 'approval_rejected',
                      transition_actor_kind = 'system', transition_actor_id = NULL,
                      transition_approval_id = NULL, transition_outreach_id = NULL,
                      worker_id = NULL, execution_generation = NULL, locked_at = NULL,
                      lock_expires_at = NULL, wait_expires_at = NULL,
                      updated_at = CURRENT_TIMESTAMP
                 FROM response_drafts AS source, expired
                WHERE (source.company_id, source.id, source.version) =
                      ($1, expired.draft_id, expired.draft_version)
                  AND task.company_id = source.company_id AND task.id = source.task_id
                  AND task.status = 'pending_approval'
           )
           UPDATE response_drafts AS draft
              SET status = 'expired', updated_at = CURRENT_TIMESTAMP
             FROM expired
            WHERE (draft.company_id, draft.id, draft.version) =
                  ($1, expired.draft_id, expired.draft_version)
              AND draft.status = 'pending_review'"#,
    )
    .bind(company_id)
    .bind(draft_id.map(ResponseDraftId::as_uuid))
    .execute(pool)
    .await
    .map_err(AppError::from)?;
    Ok(())
}

async fn load_detail(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    draft_id: ResponseDraftId,
    reviewer: PrincipalId,
) -> AppResult<Option<ResponseReviewDetail>> {
    let query = format!(
        r#"SELECT {DRAFT_COLUMNS}
           FROM response_drafts AS draft
           WHERE draft.company_id = $1 AND draft.id = $2
             AND draft.reviewer_principal_id = $3
           ORDER BY draft.version DESC LIMIT 1"#
    );
    let Some(draft_db) = sqlx::query_as::<_, DraftDb>(&query)
        .bind(company_id)
        .bind(draft_id.as_uuid())
        .bind(reviewer.as_uuid())
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(None);
    };
    if !reviewer_is_eligible_pool(pool, company_id, draft_db.channel_id, reviewer).await? {
        return Ok(None);
    }
    let version = draft_db.version;
    let draft = ResponseDraft::try_from(draft_db)?;
    let review = sqlx::query_as::<_, ReviewDb>(
        r#"SELECT company_id, draft_id, draft_version, reviewer_principal_id, status,
                  feedback, reviewer_rationale, decided_by_principal_id, expires_at,
                  created_at, updated_at
           FROM response_reviews
           WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3"#,
    )
    .bind(company_id)
    .bind(draft_id.as_uuid())
    .bind(version)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)?
    .try_into()?;
    let evidence_rows = sqlx::query_as::<_, EvidenceDb>(
        r#"SELECT id, source_reference, source_version, content_digest, audience, support
           FROM response_draft_evidence
           WHERE company_id = $1 AND draft_id = $2 AND draft_version = $3
           ORDER BY position"#,
    )
    .bind(company_id)
    .bind(draft_id.as_uuid())
    .bind(version)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)?;
    let mut evidence = Vec::with_capacity(evidence_rows.len());
    for row in evidence_rows {
        let item = evidence_from_db(row)?;
        let openable = evidence_openable(pool, company_id, reviewer, &item.source).await?;
        evidence.push(EvidenceAvailability {
            evidence: item,
            openable,
            unavailable_reason: (!openable).then(|| {
                "This source is unavailable or no longer authorized for the current reviewer."
                    .into()
            }),
        });
    }
    let history_query = format!(
        r#"SELECT {DRAFT_COLUMNS}
           FROM response_drafts AS draft
           WHERE draft.company_id = $1 AND draft.id = $2
           ORDER BY draft.version"#
    );
    let history = sqlx::query_as::<_, DraftDb>(&history_query)
        .bind(company_id)
        .bind(draft_id.as_uuid())
        .fetch_all(pool)
        .await
        .map_err(AppError::from)?
        .into_iter()
        .map(TryInto::try_into)
        .collect::<AppResult<Vec<_>>>()?;
    let visibility_warnings = evidence
        .iter()
        .filter(|item| item.evidence.audience != crate::entities::response_draft::EvidenceAudience::ExternalConversation)
        .map(|_| "Private evidence supports this response and must not be copied to recipients unless separately authorized.".into())
        .collect();
    Ok(Some(ResponseReviewDetail {
        draft,
        review,
        evidence,
        visibility_warnings,
        history,
    }))
}

fn evidence_from_db(row: EvidenceDb) -> AppResult<ResponseEvidence> {
    use crate::entities::response_draft::{EvidenceAudience, EvidenceSupport};
    let audience = match row.audience.as_str() {
        "external_conversation" => EvidenceAudience::ExternalConversation,
        "internal_only" => EvidenceAudience::InternalOnly,
        "company_restricted" => EvidenceAudience::CompanyRestricted,
        other => {
            return Err(AppError::Internal(format!(
                "Invalid evidence audience '{other}'"
            )));
        }
    };
    let support = match row.support.as_str() {
        "direct_evidence" => EvidenceSupport::DirectEvidence,
        "inference" => EvidenceSupport::Inference,
        other => {
            return Err(AppError::Internal(format!(
                "Invalid evidence support '{other}'"
            )));
        }
    };
    Ok(ResponseEvidence {
        id: row.id,
        source: decode_json(row.source_reference, "evidence source")?,
        source_version: row.source_version,
        content_digest: row.content_digest,
        audience,
        support,
    })
}

async fn evidence_openable(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    reviewer: PrincipalId,
    source: &EvidenceSource,
) -> AppResult<bool> {
    let target = match source {
        EvidenceSource::Message { thread_id, .. } => Some(("thread", *thread_id)),
        EvidenceSource::Note { note_id } => sqlx::query_scalar(
            "SELECT thread_id FROM internal_notes WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?
        .map(|thread_id| ("thread", thread_id)),
        EvidenceSource::Attachment {
            message_id,
            sha256_hash,
        } => {
            return sqlx::query_scalar(
                r#"SELECT EXISTS (
                       SELECT 1 FROM messages AS message
                       JOIN thread_messages AS association
                         ON association.company_id = message.company_id
                        AND association.message_id = message.id
                       JOIN threads AS thread ON thread.id = association.thread_id
                       JOIN principals AS reviewer
                         ON reviewer.company_id = message.company_id AND reviewer.id = $3
                       JOIN companies AS company ON company.id = message.company_id
                       JOIN channels AS channel ON channel.id = thread.channel_id
                       CROSS JOIN LATERAL jsonb_array_elements(message.attachments->'items') AS attachment
                       WHERE message.company_id = $1 AND message.id = $2
                         AND attachment->>'sha256_hash' = $4
                         AND attachment->>'storage_key' IS NOT NULL
                         AND (reviewer.user_id = company.user_id OR (
                             EXISTS (SELECT 1 FROM company_members AS member
                                     WHERE member.company_id = $1 AND member.user_id = reviewer.user_id)
                             AND (channel.access_mode IN ('team', 'public') OR EXISTS (
                                 SELECT 1 FROM channel_principal_grants AS channel_grant
                                 WHERE channel_grant.company_id = $1
                                   AND channel_grant.channel_id = channel.id
                                   AND channel_grant.principal_id = $3
                                   AND channel_grant.capability = 'view'))))
                   )"#,
            )
            .bind(company_id)
            .bind(message_id.as_uuid())
            .bind(reviewer.as_uuid())
            .bind(sha256_hash)
            .fetch_one(pool)
            .await
            .map_err(AppError::from);
        }
        EvidenceSource::DelegatedResult { task_id, .. }
        | EvidenceSource::RetainedToolResult { task_id, .. } => Some(("task", *task_id)),
        EvidenceSource::ExternalUrl { .. } => return Ok(true),
    };
    let Some((kind, id)) = target else {
        return Ok(false);
    };
    let channel_id: Option<Uuid> = match kind {
        "thread" => {
            sqlx::query_scalar("SELECT channel_id FROM threads WHERE company_id = $1 AND id = $2")
                .bind(company_id)
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(AppError::from)?
        }
        _ => sqlx::query_scalar(
            "SELECT channel_id FROM background_tasks WHERE company_id = $1 AND id = $2",
        )
        .bind(company_id)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::from)?,
    };
    let Some(channel_id) = channel_id else {
        return Ok(false);
    };
    reviewer_is_eligible_pool(pool, company_id, channel_id, reviewer).await
}

async fn reviewer_is_eligible_pool(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    channel_id: Uuid,
    reviewer: PrincipalId,
) -> AppResult<bool> {
    let mut tx = pool.begin().await.map_err(AppError::from)?;
    let eligible = reviewer_is_eligible_on(&mut tx, company_id, channel_id, reviewer).await?;
    tx.rollback().await.map_err(AppError::from)?;
    Ok(eligible)
}

mod commands;

use commands::execute_command;
