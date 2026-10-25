use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use crate::{
    adapters::http::{app_state::AppState, auth::AuthenticatedUser, pages},
    app_error::{AppError, AppResult},
    entities::schedule::{
        ChannelSchedule, ScheduleRun, ScheduleTimezone, ScheduleType, ScheduleWrite,
    },
    use_cases::schedule::{ScheduleBindingChoice, ScheduleScope, ScheduleUseCases},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedules",
            get(list_channel_schedules_json).post(create_channel_schedule_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedules/{id}",
            get(get_schedule_json)
                .put(update_schedule_json)
                .delete(delete_schedule_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedules/{id}/run-now",
            post(run_schedule_now_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedules/{id}/runs",
            get(list_schedule_runs_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedule-bindings",
            get(list_schedule_bindings_json),
        )
        .route(
            "/api/companies/{company_id}/channels/{channel_id}/schedules/{id}/toggle",
            post(toggle_schedule_json),
        )
        .route(
            "/ui/channels/{channel_id}/schedules",
            post(ui_create_schedule),
        )
        .route(
            "/ui/channels/{channel_id}/schedules/{id}/delete",
            post(ui_delete_schedule),
        )
        .route(
            "/ui/channels/{channel_id}/schedules/{id}/run-now",
            post(ui_run_schedule_now),
        )
        .route(
            "/ui/channels/{channel_id}/schedules/{id}/toggle",
            post(ui_toggle_schedule),
        )
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRequest {
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub timezone: ScheduleTimezone,
    /// The workflow binding each occurrence runs; its run-as member is whom the runs act as.
    pub binding_id: Uuid,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl ScheduleRequest {
    pub fn into_write(self) -> ScheduleWrite {
        ScheduleWrite {
            name: self.name,
            schedule_type: self.schedule_type,
            interval_seconds: self.interval_seconds,
            scheduled_at: self.scheduled_at,
            timezone: self.timezone,
            binding_id: self.binding_id,
            enabled: self.enabled,
        }
    }
}

/// A urlencoded body carries every value as a string, and a `#[serde(flatten)]` around this form
/// (see `CreateScheduleForm`) hands those strings to the field types untouched — so a plain
/// `Option<i64>` rejects `interval_seconds=3600` as "invalid type: string", and the whole submit
/// 422s before the handler runs. Parse the cadence from the string the browser actually sends.
fn deserialize_interval_seconds<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IntervalSeconds {
        Number(i64),
        Text(String),
    }

    match Option::<IntervalSeconds>::deserialize(deserializer)? {
        None => Ok(None),
        Some(IntervalSeconds::Number(seconds)) => Ok(Some(seconds)),
        Some(IntervalSeconds::Text(text)) if text.trim().is_empty() => Ok(None),
        Some(IntervalSeconds::Text(text)) => text
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiScheduleForm {
    pub company_id: Uuid,
    pub name: String,
    pub schedule_type: String,
    #[serde(default, deserialize_with = "deserialize_interval_seconds")]
    pub interval_seconds: Option<i64>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
    /// The picked workflow binding, as the select submits it.
    pub binding_id: Option<String>,
}

impl UiScheduleForm {
    pub fn into_write(self) -> AppResult<ScheduleWrite> {
        let schedule_type = self
            .schedule_type
            .parse::<ScheduleType>()
            .map_err(AppError::BadRequest)?;
        let timezone = self
            .timezone
            .as_deref()
            .unwrap_or_default()
            .parse::<ScheduleTimezone>()
            .map_err(AppError::BadRequest)?;

        let scheduled_at = match self.scheduled_at {
            Some(ref s) if !s.trim().is_empty() => {
                // Parse datetime-local string (e.g. 2026-08-25T14:30) or RFC3339
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M")
                            .map(|naive| naive.and_utc())
                    })
                    .map(Some)
                    .map_err(|e| {
                        AppError::BadRequest(format!(
                            "Invalid scheduled execution date/time format: {e}"
                        ))
                    })?
            }
            _ => None,
        };

        let binding_id = self
            .binding_id
            .as_deref()
            .map(str::trim)
            .filter(|picked| !picked.is_empty())
            .ok_or_else(|| AppError::BadRequest("Choose the workflow this schedule runs.".into()))
            .and_then(|picked| {
                Uuid::parse_str(picked).map_err(|_| {
                    AppError::BadRequest("The chosen workflow binding is not valid.".into())
                })
            })?;

        Ok(ScheduleWrite {
            name: self.name,
            schedule_type,
            interval_seconds: self.interval_seconds,
            scheduled_at,
            timezone,
            binding_id,
            // Not the form's to decide: pausing and resuming is its own action, so an edit that
            // did not ask to change it must not resume a paused schedule.
            enabled: true,
        })
    }
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn list_channel_schedules_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<ChannelSchedule>>> {
    let list = schedule_use_cases
        .list_channel_schedules(user.id, company_id, channel_id)
        .await?;
    Ok(Json(list))
}

#[instrument(skip(schedule_use_cases, user, body))]
pub async fn create_channel_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ScheduleRequest>,
) -> AppResult<(StatusCode, Json<ChannelSchedule>)> {
    let schedule = schedule_use_cases
        .create_schedule(user.id, company_id, channel_id, body.into_write())
        .await?;
    Ok((StatusCode::CREATED, Json(schedule)))
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn get_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Json<ChannelSchedule>> {
    let schedule = schedule_use_cases
        .get_schedule(user.id, ScheduleScope::channel(company_id, channel_id), id)
        .await?
        .ok_or_else(|| AppError::NotFound("Schedule not found.".into()))?;
    Ok(Json(schedule))
}

#[instrument(skip(schedule_use_cases, user, body))]
pub async fn update_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<ScheduleRequest>,
) -> AppResult<Json<ChannelSchedule>> {
    // The URL channel scopes the existing schedule; it cannot select a new channel.
    let updated = schedule_use_cases
        .update_schedule(user.id, company_id, id, channel_id, body.into_write())
        .await?;
    Ok(Json(updated))
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn delete_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    schedule_use_cases
        .delete_schedule(user.id, ScheduleScope::channel(company_id, channel_id), id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunNowBody {
    /// The client's key: repeating it returns the same occurrence and run.
    pub command_key: String,
}

#[instrument(skip(schedule_use_cases, user, body))]
pub async fn run_schedule_now_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<RunNowBody>,
) -> AppResult<Json<ScheduleRun>> {
    let run = schedule_use_cases
        .trigger_schedule_now(
            user.id,
            ScheduleScope::channel(company_id, channel_id),
            id,
            &body.command_key,
        )
        .await?;
    Ok(Json(run))
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    #[serde(default)]
    pub offset: i64,
    #[serde(default = "default_runs_limit")]
    pub limit: i64,
}

fn default_runs_limit() -> i64 {
    20
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn list_schedule_runs_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
    axum::extract::Query(query): axum::extract::Query<RunsQuery>,
) -> AppResult<Json<Vec<ScheduleRun>>> {
    let runs = schedule_use_cases
        .list_schedule_runs(
            user.id,
            ScheduleScope::channel(company_id, channel_id),
            id,
            query.offset,
            query.limit,
        )
        .await?;
    Ok(Json(runs))
}

#[derive(Debug, Serialize)]
pub struct ScheduleBindingJson {
    pub binding_id: Uuid,
    pub channel_id: Option<Uuid>,
    pub title: String,
    pub active: bool,
}

impl From<ScheduleBindingChoice> for ScheduleBindingJson {
    fn from(choice: ScheduleBindingChoice) -> Self {
        Self {
            binding_id: choice.binding_id,
            channel_id: choice.channel_id,
            title: choice.title,
            active: choice.active,
        }
    }
}

/// The bindings a schedule on this channel may name.
#[instrument(skip(schedule_use_cases, user))]
pub async fn list_schedule_bindings_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<ScheduleBindingJson>>> {
    let choices = schedule_use_cases
        .binding_choices(user.id, company_id, Some(channel_id))
        .await?;
    Ok(Json(choices.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Deserialize)]
pub struct ToggleBody {
    pub enabled: bool,
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn toggle_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<ToggleBody>,
) -> AppResult<Json<bool>> {
    schedule_use_cases
        .toggle_schedule(
            user.id,
            ScheduleScope::channel(company_id, channel_id),
            id,
            body.enabled,
        )
        .await?;
    Ok(Json(body.enabled))
}

// ---------------------------------------------------------------------------
// UI / HTMX Routes
// ---------------------------------------------------------------------------

/// The card every write below re-renders: the channel's schedules as they now stand, beside the
/// workflow bindings a new one may name.
async fn schedules_card(
    schedule_use_cases: &ScheduleUseCases,
    user_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<String> {
    let schedules = schedule_use_cases
        .list_channel_schedules(user_id, company_id, channel_id)
        .await?;
    let bindings = schedule_use_cases
        .binding_choices(user_id, company_id, Some(channel_id))
        .await?;

    Ok(pages::channel_schedules_card(
        &pages::ChannelSchedulesCard {
            company_id,
            channel_id,
            schedules: &schedules,
            bindings: &bindings,
        },
    ))
}

#[instrument(skip(schedule_use_cases, user, form))]
pub async fn ui_create_schedule(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path(channel_id): Path<Uuid>,
    Form(form): Form<UiScheduleForm>,
) -> AppResult<impl IntoResponse> {
    let company_id = form.company_id;
    let write = form.into_write()?;

    schedule_use_cases
        .create_schedule(user.id, company_id, channel_id, write)
        .await?;

    schedules_card(&schedule_use_cases, user.id, company_id, channel_id).await
}

#[derive(Debug, Deserialize)]
pub struct UiScheduleActionQuery {
    pub company_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct UiRunNowForm {
    pub company_id: Uuid,
    /// Rendered fresh with every card, so a double submit captures one occurrence.
    pub command_key: Option<String>,
}

/// The form's key, or a fresh one for a client that sent none.
pub fn run_now_key(submitted: Option<String>) -> String {
    submitted
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn ui_delete_schedule(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((channel_id, id)): Path<(Uuid, Uuid)>,
    Form(query): Form<UiScheduleActionQuery>,
) -> AppResult<impl IntoResponse> {
    schedule_use_cases
        .delete_schedule(
            user.id,
            ScheduleScope::channel(query.company_id, channel_id),
            id,
        )
        .await?;

    schedules_card(&schedule_use_cases, user.id, query.company_id, channel_id).await
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn ui_run_schedule_now(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((channel_id, id)): Path<(Uuid, Uuid)>,
    Form(form): Form<UiRunNowForm>,
) -> AppResult<impl IntoResponse> {
    schedule_use_cases
        .trigger_schedule_now(
            user.id,
            ScheduleScope::channel(form.company_id, channel_id),
            id,
            &run_now_key(form.command_key),
        )
        .await?;

    schedules_card(&schedule_use_cases, user.id, form.company_id, channel_id).await
}

#[derive(Debug, Deserialize)]
pub struct UiToggleForm {
    pub company_id: Uuid,
    pub enabled: Option<String>,
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn ui_toggle_schedule(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((channel_id, id)): Path<(Uuid, Uuid)>,
    Form(form): Form<UiToggleForm>,
) -> AppResult<impl IntoResponse> {
    let enabled = form.enabled.as_deref() == Some("true") || form.enabled.as_deref() == Some("on");
    schedule_use_cases
        .toggle_schedule(
            user.id,
            ScheduleScope::channel(form.company_id, channel_id),
            id,
            enabled,
        )
        .await?;

    schedules_card(&schedule_use_cases, user.id, form.company_id, channel_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_schedule_form_parses_interval_correctly() {
        let binding = Uuid::new_v4();
        let form = UiScheduleForm {
            company_id: Uuid::new_v4(),
            name: "Hourly Triage".into(),
            schedule_type: "interval".into(),
            interval_seconds: Some(3600),
            scheduled_at: None,
            timezone: None,
            binding_id: Some(binding.to_string()),
        };

        let write = form.into_write().unwrap();
        assert_eq!(write.name, "Hourly Triage");
        assert_eq!(write.schedule_type, ScheduleType::Interval);
        assert_eq!(write.interval_seconds, Some(3600));
        assert_eq!(write.binding_id, binding);
    }

    #[test]
    fn ui_schedule_form_parses_one_off_and_requires_a_binding() {
        let form = UiScheduleForm {
            company_id: Uuid::new_v4(),
            name: "One-Off Audit".into(),
            schedule_type: "one_off".into(),
            interval_seconds: None,
            scheduled_at: Some("2026-08-25T14:30".into()),
            timezone: Some("Europe/Ljubljana".into()),
            binding_id: Some(Uuid::new_v4().to_string()),
        };
        let write = form.clone().into_write().unwrap();
        assert_eq!(write.schedule_type, ScheduleType::OneOff);
        assert!(write.scheduled_at.is_some());

        let missing = UiScheduleForm {
            binding_id: Some(String::new()),
            ..form.clone()
        };
        assert!(matches!(missing.into_write(), Err(AppError::BadRequest(_))));
        let invalid = UiScheduleForm {
            binding_id: Some("not-a-binding".into()),
            ..form
        };
        assert!(matches!(invalid.into_write(), Err(AppError::BadRequest(_))));
    }

    #[test]
    fn schedule_request_deserializes_json_and_refuses_removed_fields() {
        let binding = Uuid::new_v4();
        let req: ScheduleRequest = serde_json::from_value(serde_json::json!({
            "name": "Daily Digest",
            "schedule_type": "interval",
            "interval_seconds": 86400,
            "binding_id": binding,
        }))
        .unwrap();
        assert_eq!(req.name, "Daily Digest");
        assert!(req.enabled);
        assert_eq!(req.into_write().binding_id, binding);

        let legacy = serde_json::from_value::<ScheduleRequest>(serde_json::json!({
            "name": "Daily Digest",
            "schedule_type": "interval",
            "interval_seconds": 86400,
            "binding_id": binding,
            "prompt_template": "Summarize inbox",
        }));
        assert!(
            legacy.is_err(),
            "a prompt template is no longer a schedule field"
        );
    }

    #[test]
    fn run_now_keys_default_to_a_fresh_one() {
        assert_eq!(run_now_key(Some(" k-1 ".into())), "k-1");
        assert_ne!(run_now_key(None), run_now_key(None));
        assert_ne!(run_now_key(Some(" ".into())), "");
    }
}

#[cfg(test)]
#[path = "schedule_channel_tests.rs"]
mod schedule_channel_tests;
