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
    entities::{
        schedule::{
            ChannelSchedule, ScheduleDeliveryMode, ScheduleTimezone, ScheduleType, ScheduleWrite,
        },
        value_objects::EmailAddress,
    },
    use_cases::schedule::ScheduleUseCases,
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
pub struct ScheduleRequest {
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub subject_template: String,
    pub prompt_template: String,
    #[serde(default = "default_delivery_mode")]
    pub delivery_mode: ScheduleDeliveryMode,
    #[serde(default)]
    pub recipient_emails: Vec<String>,
    #[serde(default)]
    pub timezone: ScheduleTimezone,
    /// The team member the runs act as. Checked against what the caller may attribute a run to
    /// before it is stored, so a client naming somebody else is refused rather than obeyed.
    #[serde(default)]
    pub run_as_user_id: Option<Uuid>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_delivery_mode() -> ScheduleDeliveryMode {
    ScheduleDeliveryMode::MailboxOnly
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
            subject_template: self.subject_template,
            prompt_template: self.prompt_template,
            delivery_mode: self.delivery_mode,
            recipient_emails: self
                .recipient_emails
                .into_iter()
                .map(EmailAddress::from)
                .collect(),
            timezone: self.timezone,
            run_as_user_id: self.run_as_user_id,
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
    pub subject_template: String,
    pub prompt_template: String,
    pub delivery_mode: String,
    pub recipient_emails: Option<String>,
    pub timezone: Option<String>,
    /// The picked team member, as the select submits it: their account id, or the empty string
    /// for a run attributed to nobody.
    pub run_as_user_id: Option<String>,
}

impl UiScheduleForm {
    pub fn into_write(self) -> AppResult<ScheduleWrite> {
        let schedule_type = self
            .schedule_type
            .parse::<ScheduleType>()
            .map_err(AppError::BadRequest)?;
        let delivery_mode = self
            .delivery_mode
            .parse::<ScheduleDeliveryMode>()
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

        let run_as_user_id = match self.run_as_user_id.as_deref().map(str::trim) {
            Some(picked) if !picked.is_empty() => Some(Uuid::parse_str(picked).map_err(|_| {
                AppError::BadRequest("Run-as team member is not a valid account.".into())
            })?),
            _ => None,
        };

        let recipient_emails = self
            .recipient_emails
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(EmailAddress::from)
            .collect();

        Ok(ScheduleWrite {
            name: self.name,
            schedule_type,
            interval_seconds: self.interval_seconds,
            scheduled_at,
            subject_template: self.subject_template,
            prompt_template: self.prompt_template,
            delivery_mode,
            recipient_emails,
            timezone,
            run_as_user_id,
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
    Path((company_id, _channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Json<ChannelSchedule>> {
    let schedule = schedule_use_cases
        .get_schedule(user.id, company_id, id)
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
    let updated = schedule_use_cases
        .update_schedule(user.id, company_id, id, channel_id, body.into_write())
        .await?;
    Ok(Json(updated))
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn delete_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, _channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    schedule_use_cases
        .delete_schedule(user.id, company_id, id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn run_schedule_now_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, _channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<Json<ChannelSchedule>> {
    let schedule = schedule_use_cases
        .trigger_schedule_now(user.id, company_id, id)
        .await?;
    Ok(Json(schedule))
}

#[derive(Debug, Deserialize)]
pub struct ToggleBody {
    pub enabled: bool,
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn toggle_schedule_json(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((company_id, _channel_id, id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<ToggleBody>,
) -> AppResult<Json<bool>> {
    let res = schedule_use_cases
        .toggle_schedule(user.id, company_id, id, body.enabled)
        .await?;
    Ok(Json(res))
}

// ---------------------------------------------------------------------------
// UI / HTMX Routes
// ---------------------------------------------------------------------------

/// The card every write below re-renders: the channel's schedules as they now stand, beside the
/// team members this caller may attribute a new one to.
async fn schedules_card(
    schedule_use_cases: &ScheduleUseCases,
    user_id: Uuid,
    company_id: Uuid,
    channel_id: Uuid,
) -> AppResult<String> {
    let schedules = schedule_use_cases
        .list_channel_schedules(user_id, company_id, channel_id)
        .await?;
    let run_as = schedule_use_cases
        .run_as_choices(user_id, company_id)
        .await?;

    Ok(pages::channel_schedules_card(
        &pages::ChannelSchedulesCard {
            company_id,
            channel_id,
            schedules: &schedules,
            run_as: &run_as,
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

#[instrument(skip(schedule_use_cases, user))]
pub async fn ui_delete_schedule(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((channel_id, id)): Path<(Uuid, Uuid)>,
    Form(query): Form<UiScheduleActionQuery>,
) -> AppResult<impl IntoResponse> {
    schedule_use_cases
        .delete_schedule(user.id, query.company_id, id)
        .await?;

    schedules_card(&schedule_use_cases, user.id, query.company_id, channel_id).await
}

#[instrument(skip(schedule_use_cases, user))]
pub async fn ui_run_schedule_now(
    State(schedule_use_cases): State<Arc<ScheduleUseCases>>,
    user: AuthenticatedUser,
    Path((channel_id, id)): Path<(Uuid, Uuid)>,
    Form(query): Form<UiScheduleActionQuery>,
) -> AppResult<impl IntoResponse> {
    schedule_use_cases
        .trigger_schedule_now(user.id, query.company_id, id)
        .await?;

    schedules_card(&schedule_use_cases, user.id, query.company_id, channel_id).await
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
        .toggle_schedule(user.id, form.company_id, id, enabled)
        .await?;

    schedules_card(&schedule_use_cases, user.id, form.company_id, channel_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_schedule_form_parses_interval_correctly() {
        let form = UiScheduleForm {
            company_id: Uuid::new_v4(),
            name: "Hourly Triage".into(),
            schedule_type: "interval".into(),
            interval_seconds: Some(3600),
            scheduled_at: None,
            subject_template: "[Hourly] {{date}}".into(),
            prompt_template: "Scan tickets".into(),
            delivery_mode: "mailbox_only".into(),
            recipient_emails: None,
            timezone: None,
            run_as_user_id: None,
        };

        let write = form.into_write().unwrap();
        assert_eq!(write.name, "Hourly Triage");
        assert_eq!(write.schedule_type, ScheduleType::Interval);
        assert_eq!(write.interval_seconds, Some(3600));
        assert_eq!(write.delivery_mode, ScheduleDeliveryMode::MailboxOnly);
        assert!(write.recipient_emails.is_empty());
    }

    #[test]
    fn ui_schedule_form_parses_one_off_with_custom_emails() {
        let form = UiScheduleForm {
            company_id: Uuid::new_v4(),
            name: "One-Off Audit".into(),
            schedule_type: "one_off".into(),
            interval_seconds: None,
            scheduled_at: Some("2026-08-25T14:30".into()),
            subject_template: "[Audit] {{time}}".into(),
            prompt_template: "Run audit".into(),
            delivery_mode: "email_custom".into(),
            recipient_emails: Some("dev@example.com, ops@example.com".into()),
            timezone: Some("Europe/Ljubljana".into()),
            run_as_user_id: None,
        };

        let write = form.into_write().unwrap();
        assert_eq!(write.name, "One-Off Audit");
        assert_eq!(write.schedule_type, ScheduleType::OneOff);
        assert!(write.scheduled_at.is_some());
        assert_eq!(write.delivery_mode, ScheduleDeliveryMode::EmailCustom);
        assert_eq!(write.recipient_emails.len(), 2);
        assert_eq!(
            write.recipient_emails[0],
            EmailAddress::from("dev@example.com")
        );
        assert_eq!(
            write.recipient_emails[1],
            EmailAddress::from("ops@example.com")
        );
    }

    #[test]
    fn schedule_request_deserializes_json() {
        let json_data = serde_json::json!({
            "name": "Daily Digest",
            "schedule_type": "interval",
            "interval_seconds": 86400,
            "subject_template": "[Daily] {{date}}",
            "prompt_template": "Summarize inbox",
            "delivery_mode": "email_participants"
        });

        let req: ScheduleRequest = serde_json::from_value(json_data).unwrap();
        assert_eq!(req.name, "Daily Digest");
        assert_eq!(req.schedule_type, ScheduleType::Interval);
        assert_eq!(req.interval_seconds, Some(86400));
        assert_eq!(req.delivery_mode, ScheduleDeliveryMode::EmailParticipants);
        assert!(req.enabled);

        let write = req.into_write();
        assert_eq!(write.name, "Daily Digest");
    }
}
