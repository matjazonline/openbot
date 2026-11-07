//! Bounded metadata for viewers already authorized through the owning run or exact thread.
use crate::adapters::persistence::PostgresPersistence;
use crate::app_error::{AppError, AppResult};
use crate::application::workflow::{WorkflowActor, channel_reply::*, reads::RunStateName};
use crate::domain::workflow::{CompanyId, RunId, WaitId};
use crate::entities::message::CanonicalMessageId;
use crate::entities::transport::{DeliveryId, DeliveryStatus};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct Row {
    run_id: Uuid,
    state: String,
    review_run: Option<Uuid>,
    source_message: Uuid,
    channel_id: Uuid,
    thread_id: Uuid,
    publication_state: Option<String>,
    final_message: Option<Uuid>,
    final_delivery: Option<Uuid>,
    final_delivery_state: Option<String>,
    prompt_run: Option<Uuid>,
    prompt_wait: Option<Uuid>,
    prompt_message: Option<Uuid>,
    prompt_deadline: Option<DateTime<Utc>>,
    prompt_waiting: Option<bool>,
    awaiting_viewer: Option<bool>,
    prompt_delivery: Option<Uuid>,
    prompt_delivery_state: Option<String>,
}
fn invalid() -> AppError {
    AppError::Internal("Invalid response outcome projection".into())
}
fn delivery(id: Option<Uuid>, state: Option<String>) -> AppResult<Option<ResponseDelivery>> {
    match (id, state) {
        (None, None) => Ok(None),
        (Some(id), Some(state)) => Ok(Some(ResponseDelivery {
            id: DeliveryId::new(id),
            state: state.parse::<DeliveryStatus>().map_err(|_| invalid())?,
        })),
        _ => Err(invalid()),
    }
}
struct ObservedOutcome {
    outcome: ChannelResponseOutcome,
    review_run: Option<Uuid>,
}

impl Row {
    fn outcome(self) -> AppResult<ObservedOutcome> {
        let final_response = self
            .publication_state
            .map(|state| {
                Ok::<_, AppError>(FinalResponseOutcome {
                    publication: FinalPublicationState::parse(&state).ok_or_else(invalid)?,
                    message: self.final_message.map(CanonicalMessageId::new),
                    delivery: delivery(self.final_delivery, self.final_delivery_state)?,
                })
            })
            .transpose()?;
        let prompt = self
            .prompt_wait
            .map(|wait| {
                Ok::<_, AppError>(HumanPromptOutcome {
                    run: RunId::new(self.prompt_run.ok_or_else(invalid)?),
                    wait: WaitId::new(wait),
                    message: CanonicalMessageId::new(self.prompt_message.ok_or_else(invalid)?),
                    deadline: self.prompt_deadline.ok_or_else(invalid)?,
                    waiting: self.prompt_waiting.ok_or_else(invalid)?,
                    awaiting_viewer: self.awaiting_viewer.ok_or_else(invalid)?,
                    delivery: delivery(self.prompt_delivery, self.prompt_delivery_state)?,
                })
            })
            .transpose()?;
        let outcome = ChannelResponseOutcome {
            run: RunId::new(self.run_id),
            source: CanonicalMessageId::new(self.source_message),
            channel: self.channel_id,
            thread: self.thread_id,
            execution: RunStateName::parse(&self.state).ok_or_else(invalid)?,
            waiting_for_review: false,
            final_response,
            prompt,
        };
        Ok(ObservedOutcome {
            outcome,
            review_run: self.review_run,
        })
    }
}

/// Channel/tenant access is inherited from the existing readers; classification is checked last.
pub(crate) async fn read(
    persistence: &PostgresPersistence,
    company: CompanyId,
    viewer: WorkflowActor,
    runs: &[Uuid],
) -> AppResult<Vec<ChannelResponseOutcome>> {
    if runs.len() > MAX_RESPONSE_OUTCOMES {
        return Err(AppError::BadRequest("Too many response outcomes".into()));
    }
    let rows = sqlx::query_as::<_, Row>(include_str!("channel_reply_views.sql"))
        .bind(company.as_uuid())
        .bind(runs)
        .bind(viewer.user_id())
        .fetch_all(persistence.pool())
        .await?;
    let outcomes = rows
        .into_iter()
        .map(Row::outcome)
        .collect::<AppResult<Vec<_>>>()?;
    visible(persistence, company, viewer, outcomes).await
}

async fn visible(
    persistence: &PostgresPersistence,
    company: CompanyId,
    viewer: WorkflowActor,
    outcomes: Vec<ObservedOutcome>,
) -> AppResult<Vec<ChannelResponseOutcome>> {
    let runs: Vec<_> = outcomes
        .iter()
        .flat_map(|observed| {
            let outcome = &observed.outcome;
            std::iter::once(outcome.run.as_uuid())
                .chain(outcome.prompt.as_ref().map(|prompt| prompt.run.as_uuid()))
                .chain(observed.review_run)
        })
        .collect();
    let messages: Vec<_> = outcomes
        .iter()
        .flat_map(|observed| {
            let outcome = &observed.outcome;
            std::iter::once(outcome.source.as_uuid())
                .chain(
                    outcome
                        .prompt
                        .as_ref()
                        .map(|prompt| prompt.message.as_uuid()),
                )
                .chain(
                    outcome
                        .final_response
                        .as_ref()
                        .and_then(|response| response.message)
                        .map(|message| message.as_uuid()),
                )
        })
        .collect();
    let levels =
        super::super::view_visibility::run_levels_on(persistence.pool(), company, viewer, &runs)
            .await?;
    let messages = super::super::view_visibility::message_levels_on(
        persistence.pool(),
        company,
        viewer,
        &messages,
    )
    .await?;
    let visible_run = |id| levels.get(&id).is_some_and(|level| level.is_visible());
    let visible_message = |id| messages.get(&id).is_some_and(|level| level.is_visible());
    Ok(outcomes
        .into_iter()
        .filter_map(|observed| {
            let mut outcome = observed.outcome;
            outcome.waiting_for_review = observed.review_run.is_some_and(visible_run);
            if !visible_run(outcome.run.as_uuid()) || !visible_message(outcome.source.as_uuid()) {
                return None;
            }
            if outcome.prompt.as_ref().is_some_and(|prompt| {
                !visible_run(prompt.run.as_uuid()) || !visible_message(prompt.message.as_uuid())
            }) {
                outcome.prompt = None;
            }
            if outcome
                .final_response
                .as_ref()
                .and_then(|response| response.message)
                .is_some_and(|message| !visible_message(message.as_uuid()))
            {
                return None;
            }
            Some(outcome)
        })
        .collect())
}

pub(crate) async fn thread_runs(
    persistence: &PostgresPersistence,
    company: Uuid,
    thread: Uuid,
) -> AppResult<(Vec<Uuid>, bool)> {
    let mut ids:Vec<Uuid>=sqlx::query_scalar("SELECT owner.run_id FROM channel_reply_runs AS owner JOIN channel_reply_contexts AS context ON context.company_id=owner.company_id AND context.intent_id=owner.intent_id JOIN workflow_runs AS run ON run.company_id=owner.company_id AND run.id=owner.run_id WHERE context.company_id=$1 AND context.thread_id=$2 ORDER BY run.created_at DESC,run.id DESC LIMIT 51")
        .bind(company).bind(thread).fetch_all(persistence.pool()).await?;
    let more = ids.len() > MAX_RESPONSE_OUTCOMES;
    ids.truncate(MAX_RESPONSE_OUTCOMES);
    Ok((ids, more))
}
