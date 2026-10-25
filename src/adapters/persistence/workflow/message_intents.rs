//! The workflow-admission intent queue (p08 D1.3/D1.4). Claims are fair per company and fenced by
//! a per-claim generation; every later write names it, so a stale worker changes nothing.
use super::*;
use crate::application::workflow::message_admission::*;
use crate::application::workflow::triggers::MessageTriggerInput;
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::entities::message::CanonicalMessageId;
use crate::domain::entities::transport::RecipientRole;
use chrono::{DateTime, Utc};

#[derive(sqlx::FromRow)]
struct IntentRow {
    company_id: Uuid,
    id: Uuid,
    claim_generation: Uuid,
    message_id: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    role: String,
    mentioned: bool,
    admissible: bool,
    origin: String,
    reply_to_message_id: Option<Uuid>,
    reply_settled: bool,
    responder_principal_id: Option<Uuid>,
    delivery_policy: String,
    correlation_id: Uuid,
    attempts: i16,
    created_at: DateTime<Utc>,
    replies_to_workflow: bool,
}

impl IntentRow {
    fn claimed(self, worker: MessageIntentWorkerId) -> AppResult<ClaimedIntent> {
        let role = match self.role.as_str() {
            "to" => RecipientRole::To,
            "cc" => RecipientRole::Cc,
            _ => return Err(invalid()),
        };
        Ok(ClaimedIntent {
            claim: IntentClaim {
                company: CompanyId::new(self.company_id),
                intent: self.id,
                worker,
                generation: self.claim_generation,
            },
            message: CanonicalMessageId::new(self.message_id),
            channel: self.channel_id,
            thread: self.thread_id,
            facts: MessageFacts {
                role,
                mentioned: self.mentioned,
                replies_to_workflow: self.replies_to_workflow,
            },
            admissible: self.admissible,
            in_app: self.origin == "application",
            reply_to: self.reply_to_message_id.map(CanonicalMessageId::new),
            reply_settled: self.reply_settled,
            responder: self.responder_principal_id,
            delivery_policy: DeliveryPolicy::parse(&self.delivery_policy)?,
            correlation: CorrelationId::from(self.correlation_id),
            attempts: self.attempts,
            accepted_at: self.created_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct CandidateRow {
    id: Uuid,
    configuration_revision: i64,
    run_as_user_id: Uuid,
    message_trigger: String,
}

impl CandidateRow {
    fn candidate(self) -> AppResult<CandidateBinding> {
        Ok(CandidateBinding {
            id: WorkflowBindingId::new(self.id),
            revision: BindingRevision::new(
                u64::try_from(self.configuration_revision).map_err(|_| invalid())?,
            )
            .ok_or_else(invalid)?,
            run_as: WorkflowActor::authenticated(self.run_as_user_id).map_err(|_| invalid())?,
            trigger: self.message_trigger.parse().map_err(|_| invalid())?,
        })
    }
}

/// The eligibility read; `$4` narrows it to one binding for a `Stale` re-read.
const CANDIDATES: &str = "SELECT binding.id, binding.configuration_revision, \
       configuration.run_as_user_id, configuration.message_trigger \
     FROM workflow_bindings AS binding \
     JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
       AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
     JOIN workflow_message_intents AS intent ON intent.company_id = binding.company_id AND intent.id = $2 \
     WHERE binding.company_id = $1 AND binding.active AND binding.activated_at <= intent.created_at \
       AND configuration.message_trigger IS NOT NULL \
       AND binding.channel_id = intent.channel_id \
       AND (binding.thread_id IS NULL OR binding.thread_id = intent.thread_id) \
       AND ($3::uuid IS NULL OR binding.id = $3) \
       AND NOT EXISTS (SELECT 1 FROM workflow_message_intent_bindings AS outcome \
         WHERE outcome.company_id = binding.company_id AND outcome.intent_id = intent.id \
           AND outcome.binding_id = binding.id) \
     ORDER BY binding.id";

/// The claim's fence, held for the statement that writes under it.
const FENCE: &str = "SELECT id FROM workflow_message_intents WHERE company_id = $1 AND id = $2 \
     AND state = 'claimed' AND worker_id = $3 AND claim_generation = $4 \
     AND lease_expires_at > clock_timestamp() FOR UPDATE";

#[async_trait]
impl MessageIntentQueue for PostgresPersistence {
    async fn fail_expired_intents(&self, page: u16) -> AppResult<Vec<FailedIntent>> {
        let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "UPDATE workflow_message_intents SET state = 'failed', worker_id = NULL, \
               claim_generation = NULL, lease_expires_at = NULL, updated_at = clock_timestamp() \
             WHERE ctid IN (SELECT ctid FROM workflow_message_intents WHERE state = 'claimed' \
               AND attempts >= $1 AND lease_expires_at <= clock_timestamp() LIMIT $2 \
               FOR UPDATE SKIP LOCKED) \
             RETURNING company_id, id",
        )
        .bind(MAX_INTENT_ATTEMPTS)
        .bind(i64::from(page))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(company, intent)| FailedIntent {
                company: CompanyId::new(company),
                intent,
            })
            .collect())
    }

    async fn claim_intents(
        &self,
        worker: MessageIntentWorkerId,
        page: u16,
    ) -> AppResult<Vec<ClaimedIntent>> {
        // Rank per company before the page limit, so one tenant's backlog cannot fill every page;
        // the outer lock skips rows another worker holds (p08 F008). A claim whose last lease
        // lapsed is not claimable: `fail_expired_intents` ends it.
        let rows = sqlx::query_as::<_, IntentRow>(
            "WITH available AS ( \
               SELECT company_id, id, available_at, \
                 row_number() OVER (PARTITION BY company_id ORDER BY available_at, id) AS rank \
               FROM workflow_message_intents \
               WHERE (state = 'pending' AND available_at <= clock_timestamp()) \
                  OR (state = 'claimed' AND attempts < $4 AND lease_expires_at <= clock_timestamp()) \
             ), picked AS ( \
               SELECT company_id, id FROM available WHERE rank <= $3 \
               ORDER BY available_at, id LIMIT $2 \
             ), locked AS ( \
               SELECT intent.company_id, intent.id FROM workflow_message_intents AS intent \
               JOIN picked ON picked.company_id = intent.company_id AND picked.id = intent.id \
               WHERE (intent.state = 'pending' AND intent.available_at <= clock_timestamp()) \
                  OR (intent.state = 'claimed' AND intent.attempts < $4 \
                      AND intent.lease_expires_at <= clock_timestamp()) \
               FOR UPDATE OF intent SKIP LOCKED \
             ) \
             UPDATE workflow_message_intents AS intent SET state = 'claimed', worker_id = $1, \
               claim_generation = gen_random_uuid(), attempts = intent.attempts + 1, \
               lease_expires_at = clock_timestamp() + make_interval(secs => $5), \
               updated_at = clock_timestamp() \
             FROM locked WHERE intent.company_id = locked.company_id AND intent.id = locked.id \
             RETURNING intent.company_id, intent.id, intent.claim_generation, intent.message_id, \
               intent.channel_id, intent.thread_id, intent.role, intent.mentioned, intent.admissible, \
               intent.origin, intent.reply_to_message_id, intent.reply_settled, \
               intent.responder_principal_id, \
               intent.delivery_policy, intent.correlation_id, intent.attempts, intent.created_at, \
               EXISTS (SELECT 1 FROM workflow_message_destinations AS target \
                 WHERE target.company_id = intent.company_id \
                   AND target.message_id = intent.reply_to_message_id \
                   AND target.thread_id = intent.thread_id) AS replies_to_workflow",
        )
        .bind(worker.0)
        .bind(i64::from(page))
        .bind(i64::from(MESSAGE_INTENTS_PER_COMPANY))
        .bind(MAX_INTENT_ATTEMPTS)
        .bind(MESSAGE_INTENT_LEASE.as_secs_f64())
        .fetch_all(&self.pool)
        .await?;
        let mut claimed: Vec<_> = rows
            .into_iter()
            .map(|row| row.claimed(worker))
            .collect::<AppResult<_>>()?;
        claimed.sort_by_key(|intent| intent.accepted_at);
        Ok(claimed)
    }

    async fn candidate_bindings(&self, intent: &ClaimedIntent) -> AppResult<Vec<CandidateBinding>> {
        sqlx::query_as::<_, CandidateRow>(CANDIDATES)
            .bind(intent.claim.company.as_uuid())
            .bind(intent.claim.intent)
            .bind(None::<Uuid>)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(CandidateRow::candidate)
            .collect()
    }

    async fn candidate_binding(
        &self,
        intent: &ClaimedIntent,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<CandidateBinding>> {
        sqlx::query_as::<_, CandidateRow>(CANDIDATES)
            .bind(intent.claim.company.as_uuid())
            .bind(intent.claim.intent)
            .bind(Some(binding.as_uuid()))
            .fetch_optional(&self.pool)
            .await?
            .map(CandidateRow::candidate)
            .transpose()
    }

    async fn trigger_input(&self, intent: &ClaimedIntent) -> AppResult<MessageTriggerInput> {
        let (subject, body, sender): (String, String, Option<String>) = sqlx::query_as(
            "SELECT message.subject, message.clean_text_body, \
               COALESCE((SELECT identity.subject FROM message_participants AS participant \
                  JOIN participant_identities AS identity ON identity.company_id = participant.company_id \
                    AND identity.id = participant.participant_identity_id \
                 WHERE participant.company_id = message.company_id AND participant.message_id = message.id \
                   AND participant.kind = 'sender' ORDER BY participant.position LIMIT 1), \
                 (SELECT principal.display_label FROM principals AS principal \
                 WHERE principal.company_id = message.company_id AND principal.id = message.author_principal_id)) \
             FROM messages AS message WHERE message.company_id = $1 AND message.id = $2",
        )
        .bind(intent.claim.company.as_uuid())
        .bind(intent.message.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(missing)?;
        Ok(MessageTriggerInput::new(
            intent.message,
            &body,
            &subject,
            sender.as_deref().unwrap_or_default(),
        ))
    }

    async fn record_binding_outcome(
        &self,
        claim: IntentClaim,
        binding: &CandidateBinding,
        outcome: BindingOutcome,
    ) -> AppResult<bool> {
        let (name, run, reason) = match outcome {
            BindingOutcome::Admitted(run) => ("admitted", Some(run.as_uuid()), None),
            BindingOutcome::Refused(reason) => ("refused", None, Some(reason)),
            BindingOutcome::Failed(reason) => ("failed", None, Some(reason)),
        };
        let written = sqlx::query(&format!(
            "WITH fence AS ({FENCE}) \
             INSERT INTO workflow_message_intent_bindings \
               (company_id, intent_id, binding_id, binding_revision, outcome, run_id, reason) \
             SELECT $1, fence.id, $5, $6, $7, $8, $9 FROM fence \
             ON CONFLICT (company_id, intent_id, binding_id) DO NOTHING"
        ))
        .bind(claim.company.as_uuid())
        .bind(claim.intent)
        .bind(claim.worker.0)
        .bind(claim.generation)
        .bind(binding.id.as_uuid())
        .bind(revision(binding.revision.get())?)
        .bind(name)
        .bind(run)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if written.rows_affected() == 1 {
            return Ok(true);
        }
        // Either the claim was lost, or this binding already has an outcome under this claim's
        // own earlier write (a retried statement); only the fence decides.
        fence_holds(&self.pool, claim).await
    }

    async fn settle_reply(&self, claim: IntentClaim) -> AppResult<bool> {
        let settled = sqlx::query(&format!(
            "WITH fence AS ({FENCE}) \
             UPDATE workflow_message_intents AS intent SET reply_settled = true, \
               updated_at = clock_timestamp() \
             FROM fence WHERE intent.company_id = $1 AND intent.id = fence.id"
        ))
        .bind(claim.company.as_uuid())
        .bind(claim.intent)
        .bind(claim.worker.0)
        .bind(claim.generation)
        .execute(&self.pool)
        .await?;
        Ok(settled.rows_affected() == 1)
    }

    async fn finish_intent(&self, claim: IntentClaim, matched: Option<RunId>) -> AppResult<bool> {
        let finished = sqlx::query(&format!(
            "WITH fence AS ({FENCE}) \
             UPDATE workflow_message_intents AS intent SET state = 'done', worker_id = NULL, \
               claim_generation = NULL, lease_expires_at = NULL, updated_at = clock_timestamp(), \
               matched_run_id = $5, \
               outcome = CASE WHEN $5::uuid IS NOT NULL THEN 'matched' \
                 WHEN EXISTS (SELECT 1 FROM workflow_message_intent_bindings AS outcome \
                   WHERE outcome.company_id = intent.company_id AND outcome.intent_id = intent.id \
                     AND outcome.outcome = 'admitted') THEN 'admitted' ELSE 'none' END \
             FROM fence WHERE intent.company_id = $1 AND intent.id = fence.id"
        ))
        .bind(claim.company.as_uuid())
        .bind(claim.intent)
        .bind(claim.worker.0)
        .bind(claim.generation)
        .bind(matched.map(RunId::as_uuid))
        .execute(&self.pool)
        .await?;
        Ok(finished.rows_affected() == 1)
    }

    async fn defer_intent(&self, claim: IntentClaim, deferral: IntentDeferral) -> AppResult<bool> {
        // Spends the attempt: exponential backoff from 2 s, and the last attempt fails.
        let transient = format!(
            "WITH fence AS ({FENCE}) \
             UPDATE workflow_message_intents AS intent SET worker_id = NULL, \
               claim_generation = NULL, lease_expires_at = NULL, updated_at = clock_timestamp(), \
               state = CASE WHEN intent.attempts >= $5 THEN 'failed' ELSE 'pending' END, \
               available_at = clock_timestamp() + make_interval(secs => 2 * power(2, intent.attempts - 1)) \
             FROM fence WHERE intent.company_id = $1 AND intent.id = fence.id"
        );
        // Attempt-neutral: returns the claim's attempt and doubles the delay with the reply's age,
        // never past its park grace (p08 F014, F020).
        let not_yet = format!(
            "WITH fence AS ({FENCE}) \
             UPDATE workflow_message_intents AS intent SET worker_id = NULL, \
               claim_generation = NULL, lease_expires_at = NULL, updated_at = clock_timestamp(), \
               state = 'pending', attempts = intent.attempts - 1, \
               available_at = least(clock_timestamp() + greatest(interval '2 seconds', \
                 clock_timestamp() - intent.created_at), \
                 intent.created_at + make_interval(secs => $5)) \
             FROM fence WHERE intent.company_id = $1 AND intent.id = fence.id"
        );
        let deferred = match deferral {
            IntentDeferral::Transient => sqlx::query(&transient)
                .bind(claim.company.as_uuid())
                .bind(claim.intent)
                .bind(claim.worker.0)
                .bind(claim.generation)
                .bind(MAX_INTENT_ATTEMPTS),
            IntentDeferral::NotYet => sqlx::query(&not_yet)
                .bind(claim.company.as_uuid())
                .bind(claim.intent)
                .bind(claim.worker.0)
                .bind(claim.generation)
                .bind(REPLY_PARK_GRACE.as_secs_f64()),
        }
        .execute(&self.pool)
        .await?;
        Ok(deferred.rows_affected() == 1)
    }
}

async fn fence_holds(pool: &sqlx::PgPool, claim: IntentClaim) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM workflow_message_intents WHERE company_id = $1 AND id = $2 \
           AND state = 'claimed' AND worker_id = $3 AND claim_generation = $4 \
           AND lease_expires_at > clock_timestamp())",
    )
    .bind(claim.company.as_uuid())
    .bind(claim.intent)
    .bind(claim.worker.0)
    .bind(claim.generation)
    .fetch_one(pool)
    .await?)
}
