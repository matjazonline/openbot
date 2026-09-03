use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use crate::entities::{
    company::CompanyTeamAccount,
    message::CanonicalMessageId,
    task::{TaskStatus, ThreadActivity},
    value_objects::{EmailAddress, MessageId},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    Interval,
    OneOff,
}

impl ScheduleType {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleType::Interval => "interval",
            ScheduleType::OneOff => "one_off",
        }
    }
}

impl FromStr for ScheduleType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "interval" => Ok(ScheduleType::Interval),
            "one_off" | "oneoff" | "one-off" => Ok(ScheduleType::OneOff),
            other => Err(format!("Unknown schedule type: {other}")),
        }
    }
}

impl std::fmt::Display for ScheduleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// The IANA zone a schedule reckons in, parsed once here so no caller has to decide what an
/// unknown zone name means. Serialises as the plain name (`"Europe/Ljubljana"`), which is also
/// what the `timezone` column stores and what Postgres reads back in `AT TIME ZONE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleTimezone(Tz);

impl ScheduleTimezone {
    pub fn utc() -> Self {
        Self(Tz::UTC)
    }

    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    /// Render an instant as this zone shows it. `%Z` expands to the zone's abbreviation.
    pub fn format(&self, at: DateTime<Utc>, format: &str) -> String {
        at.with_timezone(&self.0).format(format).to_string()
    }
}

impl Default for ScheduleTimezone {
    fn default() -> Self {
        Self::utc()
    }
}

impl FromStr for ScheduleTimezone {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Ok(Self::utc());
        }
        trimmed
            .parse::<Tz>()
            .map(Self)
            .map_err(|_| format!("Unknown timezone: {trimmed}"))
    }
}

impl std::fmt::Display for ScheduleTimezone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Serialize for ScheduleTimezone {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for ScheduleTimezone {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleDeliveryMode {
    /// Keep output inside the channel's mailbox thread only.
    MailboxOnly,
    /// Post to mailbox thread and email all configured channel participants.
    EmailParticipants,
    /// Post to mailbox thread and email a custom recipient list.
    EmailCustom,
}

impl ScheduleDeliveryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleDeliveryMode::MailboxOnly => "mailbox_only",
            ScheduleDeliveryMode::EmailParticipants => "email_participants",
            ScheduleDeliveryMode::EmailCustom => "email_custom",
        }
    }
}

impl FromStr for ScheduleDeliveryMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "mailbox_only" | "mailboxonly" => Ok(ScheduleDeliveryMode::MailboxOnly),
            "email_participants" | "emailparticipants" => {
                Ok(ScheduleDeliveryMode::EmailParticipants)
            }
            "email_custom" | "emailcustom" => Ok(ScheduleDeliveryMode::EmailCustom),
            other => Err(format!("Unknown schedule delivery mode: {other}")),
        }
    }
}

impl std::fmt::Display for ScheduleDeliveryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelSchedule {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    pub subject_template: String,
    pub prompt_template: String,
    pub delivery_mode: ScheduleDeliveryMode,
    #[serde(default)]
    pub recipient_emails: Vec<EmailAddress>,
    /// IANA zone the templates render in and the cadence counts its days by.
    #[serde(default = "default_timezone")]
    pub timezone: ScheduleTimezone,
    /// The team member every run acts as, or `None` for a run attributed to nobody. Stored as the
    /// account rather than the address so renaming a mailbox does not re-point the attribution;
    /// [`ScheduleRunAs`] is what the address is resolved into at execution.
    #[serde(default)]
    pub run_as_user_id: Option<Uuid>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    /// Why the last trigger did not launch, cleared by the next one that does.
    #[serde(default)]
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_true() -> bool {
    true
}

fn default_timezone() -> ScheduleTimezone {
    ScheduleTimezone::utc()
}

impl ChannelSchedule {
    /// Expand `{{date}}`, `{{time}}`, `{{datetime}}` in the subject template.
    pub fn render_subject(&self, now: DateTime<Utc>) -> String {
        render_template(&self.subject_template, now, &self.timezone)
    }

    /// Expand `{{date}}`, `{{time}}`, `{{datetime}}` in the prompt template.
    pub fn render_prompt(&self, now: DateTime<Utc>) -> String {
        render_template(&self.prompt_template, now, &self.timezone)
    }

    /// An instant as the schedule's own zone shows it, for display next to its cadence.
    pub fn in_zone(&self, at: DateTime<Utc>, format: &str) -> String {
        self.timezone.format(at, format)
    }

    /// A human-readable summary of the cadence (e.g. "Every 1 hour", "One-off on 2026-08-25 14:00 UTC").
    pub fn cadence_label(&self) -> String {
        match self.schedule_type {
            ScheduleType::Interval => match self.interval_seconds {
                Some(secs) if secs % 86400 == 0 => {
                    let days = secs / 86400;
                    if days == 1 {
                        "Every day".to_string()
                    } else if days == 7 {
                        "Every week".to_string()
                    } else {
                        format!("Every {days} days")
                    }
                }
                Some(secs) if secs % 3600 == 0 => {
                    let hours = secs / 3600;
                    if hours == 1 {
                        "Every hour".to_string()
                    } else {
                        format!("Every {hours} hours")
                    }
                }
                Some(secs) if secs % 60 == 0 => {
                    let mins = secs / 60;
                    format!("Every {mins} min")
                }
                Some(secs) => format!("Every {secs}s"),
                None => "Interval (not set)".to_string(),
            },
            ScheduleType::OneOff => match self.next_run_at {
                Some(next) => format!("One-off on {}", self.in_zone(next, "%b %d, %Y %H:%M %Z")),
                None => match self.last_run_at {
                    Some(last) => format!("One-off (ran on {})", self.in_zone(last, "%b %d, %Y")),
                    None => "One-off".to_string(),
                },
            },
        }
    }
}

fn render_template(template: &str, now: DateTime<Utc>, zone: &ScheduleTimezone) -> String {
    let date_str = zone.format(now, "%Y-%m-%d");
    let time_str = zone.format(now, "%H:%M %Z");
    let datetime_str = zone.format(now, "%Y-%m-%d %H:%M %Z");

    template
        .replace("{{date}}", &date_str)
        .replace("{{ date }}", &date_str)
        .replace("{{time}}", &time_str)
        .replace("{{ time }}", &time_str)
        .replace("{{datetime}}", &datetime_str)
        .replace("{{ datetime }}", &datetime_str)
}

#[derive(Debug, Clone)]
pub struct ScheduleWrite {
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub subject_template: String,
    pub prompt_template: String,
    pub delivery_mode: ScheduleDeliveryMode,
    pub recipient_emails: Vec<EmailAddress>,
    pub timezone: ScheduleTimezone,
    /// Whom the runs act as. Never taken from a form untouched: only
    /// [`crate::use_cases::schedule::ScheduleUseCases`] decides whether the submitter may
    /// attribute runs to this account.
    pub run_as_user_id: Option<Uuid>,
    pub enabled: bool,
}

/// The identity one scheduled run acts as, resolved from the schedule's `run_as_user_id` when the
/// run is materialized.
///
/// The account and its address travel together because both are needed and neither is derivable
/// from the other in the worker: the address keys user-scoped memory and stands as the prompt's
/// sender, and the account id is what the run is traced by.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleRunAs {
    pub user_id: Uuid,
    pub email: EmailAddress,
}

/// Who a schedule may be made to run as, and which of those this caller may choose.
///
/// One value rather than two lists, because every place that renders the picker also has to name
/// a member the caller cannot pick: an admin sees that a schedule runs as the owner and may leave
/// it that way, without being able to attribute a new run to anybody but themselves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScheduleRunAsChoices {
    /// Everyone on the company's team, for naming whoever a stored schedule already runs as.
    pub team: Vec<CompanyTeamAccount>,
    /// The one account this caller may attribute runs to, or `None` when they may pick anyone.
    pub restricted_to: Option<Uuid>,
}

impl ScheduleRunAsChoices {
    /// Whether this caller may store `user_id` as a schedule's run-as member.
    pub fn may_choose(&self, user_id: Uuid) -> bool {
        self.account(user_id).is_some()
            && self
                .restricted_to
                .is_none_or(|restricted| restricted == user_id)
    }

    /// The team member behind an id, whether or not this caller may choose them.
    pub fn account(&self, user_id: Uuid) -> Option<&CompanyTeamAccount> {
        self.team.iter().find(|account| account.user_id == user_id)
    }

    /// The members to offer in a picker.
    pub fn choosable(&self) -> impl Iterator<Item = &CompanyTeamAccount> {
        self.team
            .iter()
            .filter(|account| self.may_choose(account.user_id))
    }

    /// How a picker has to present what a schedule already stores, which is not the same question
    /// as who may be chosen: a member the caller cannot pick is still one they must be able to
    /// leave in place, and an id whose account has left the team is one they can only replace.
    pub fn selection(&self, stored: Option<Uuid>) -> RunAsSelection<'_> {
        let Some(user_id) = stored else {
            return RunAsSelection::System;
        };

        match self.account(user_id) {
            None => RunAsSelection::Departed,
            Some(account) if self.may_choose(user_id) => RunAsSelection::Choosable(account),
            Some(account) => RunAsSelection::Locked(account),
        }
    }
}

/// What a schedule's stored run-as member is to the caller looking at it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunAsSelection<'a> {
    /// No member named: the run acts as nobody.
    System,
    /// A member this caller may keep or change.
    Choosable(&'a CompanyTeamAccount),
    /// A member only the company owner may change, offered back unchanged.
    Locked(&'a CompanyTeamAccount),
    /// An account that has left the team, which any caller may replace but nobody may keep.
    Departed,
}

/// What one `scheduled_agent_run` task carries. The worker deserialises this rather than reaching
/// into the JSON by key, so a payload written by an older build fails the task loudly instead of
/// running the agent with an empty prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduledRunPayload {
    /// Stable identity of the logical schedule slot. Older queued payloads predate durable runs.
    #[serde(default)]
    pub schedule_run_id: Option<Uuid>,
    pub schedule_id: Uuid,
    pub schedule_name: String,
    pub channel_id: Uuid,
    pub company_id: Uuid,
    pub thread_id: Uuid,
    pub subject: String,
    pub prompt: String,
    pub delivery_mode: ScheduleDeliveryMode,
    #[serde(default)]
    pub recipient_emails: Vec<EmailAddress>,
    /// Whom this run acts as. Absent both for an unattributed run and in payloads queued before
    /// attribution existed, which are the same thing to the worker.
    #[serde(default)]
    pub run_as: Option<ScheduleRunAs>,
    /// The `Message-ID` the answer email threads onto. Email transport only -- it is not how the
    /// prompt message is identified.
    pub trigger_message_id: MessageId,
    /// The canonical prompt message this run answers.
    ///
    /// The worker's idempotency guard asks "did a previous attempt already answer this?", which is
    /// a question about canonical messages in a thread, not about mail headers -- so the prompt is
    /// named by its canonical id and a run over a transport with no headers at all works the same.
    pub prompt_message_id: CanonicalMessageId,
}

/// A durable logical slot waiting to be materialized into a thread, prompt, and task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimedScheduleRun {
    pub id: Uuid,
    pub materialization_generation: Uuid,
    pub scheduled_for: DateTime<Utc>,
    pub schedule: ChannelSchedule,
    pub thread_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
}

impl ScheduledRunPayload {
    /// Who this run's answer is emailed to, or `None` when it stays in the mailbox. The channel's
    /// own participants are not resolved here — the worker holds the channel, this does not.
    pub fn custom_recipients(&self) -> Option<&[EmailAddress]> {
        match self.delivery_mode {
            ScheduleDeliveryMode::EmailCustom => Some(&self.recipient_emails),
            ScheduleDeliveryMode::MailboxOnly | ScheduleDeliveryMode::EmailParticipants => None,
        }
    }

    pub fn wants_email(&self) -> bool {
        !matches!(self.delivery_mode, ScheduleDeliveryMode::MailboxOnly)
    }

    /// The address this run's memory is scoped to and its prompt is attributed to, or `None` when
    /// the run acts as nobody.
    pub fn run_as_email(&self) -> Option<&EmailAddress> {
        self.run_as.as_ref().map(|run_as| &run_as.email)
    }
}

/// One execution of a schedule, joining the created thread with its background task state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleRun {
    pub thread_id: Uuid,
    pub task_id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub task_status: TaskStatus,
    pub lock_expires_at: Option<DateTime<Utc>>,
    pub latest_response: Option<String>,
    pub message_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ScheduleRun {
    pub fn activity(&self, now: DateTime<Utc>) -> Option<ThreadActivity> {
        ThreadActivity::from_task(self.task_status, self.lock_expires_at, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_template_variables() {
        let schedule = ChannelSchedule {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            name: "Morning Report".to_string(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(86400),
            subject_template: "[Daily Report] {{date}} Summary".to_string(),
            prompt_template: "Generate summary for {{date}} at {{time}}".to_string(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            run_as_user_id: None,
            enabled: true,
            last_run_at: None,
            next_run_at: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let now = DateTime::parse_from_rfc3339("2026-08-25T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(
            schedule.render_subject(now),
            "[Daily Report] 2026-08-25 Summary"
        );
        assert_eq!(
            schedule.render_prompt(now),
            "Generate summary for 2026-08-25 at 09:30 UTC"
        );
    }

    #[test]
    fn cadence_labels_are_human_readable() {
        let mut schedule = ChannelSchedule {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            name: "Test".to_string(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            subject_template: "".to_string(),
            prompt_template: "".to_string(),
            delivery_mode: ScheduleDeliveryMode::MailboxOnly,
            recipient_emails: vec![],
            timezone: ScheduleTimezone::utc(),
            run_as_user_id: None,
            enabled: true,
            last_run_at: None,
            next_run_at: None,
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert_eq!(schedule.cadence_label(), "Every hour");

        schedule.interval_seconds = Some(86400);
        assert_eq!(schedule.cadence_label(), "Every day");

        schedule.interval_seconds = Some(86400 * 7);
        assert_eq!(schedule.cadence_label(), "Every week");

        schedule.interval_seconds = Some(1800);
        assert_eq!(schedule.cadence_label(), "Every 30 min");
    }
}
