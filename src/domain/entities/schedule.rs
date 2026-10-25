use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelSchedule {
    pub id: Uuid,
    pub company_id: Uuid,
    pub channel_id: Uuid,
    /// The workflow binding each occurrence admits a run of. The run acts as that binding's
    /// run-as member (p08 D1.1, D1.9).
    pub binding_id: Uuid,
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    /// IANA zone the cadence counts its days by.
    #[serde(default = "default_timezone")]
    pub timezone: ScheduleTimezone,
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

#[derive(Debug, Clone)]
pub struct ScheduleWrite {
    pub name: String,
    pub schedule_type: ScheduleType,
    pub interval_seconds: Option<i64>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub timezone: ScheduleTimezone,
    /// Checked against the schedule's channel when written (p08 D1.9): its association must be
    /// the company or this channel, its pinned input must accept the schedule trigger input, and
    /// its run-as must see the channel.
    pub binding_id: Uuid,
    pub enabled: bool,
}

/// A captured occurrence a materializer holds under its lease generation. The snapshot is the
/// schedule as it was when the occurrence was captured; the binding is the one captured with it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimedScheduleRun {
    pub id: Uuid,
    pub materialization_generation: Uuid,
    pub scheduled_for: DateTime<Utc>,
    pub binding_id: Uuid,
    pub schedule: ChannelSchedule,
}

/// Where one occurrence is in becoming a workflow run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Pending,
    Materializing,
    Materialized,
    Failed,
}

impl FromStr for OccurrenceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "materializing" => Ok(Self::Materializing),
            "materialized" => Ok(Self::Materialized),
            "failed" => Ok(Self::Failed),
            other => Err(format!("Unknown schedule occurrence status: {other}")),
        }
    }
}

impl OccurrenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Materializing => "materializing",
            Self::Materialized => "materialized",
            Self::Failed => "failed",
        }
    }
}

/// One occurrence of a schedule, with the workflow run it admitted once it has one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleRun {
    pub id: Uuid,
    pub scheduled_for: DateTime<Utc>,
    /// Started by run-now rather than by the cadence.
    pub manual: bool,
    pub status: OccurrenceStatus,
    pub run_id: Option<Uuid>,
    /// The run's state (`workflow_runs.state`), when there is a run.
    pub run_state: Option<String>,
    /// Why admission refused the occurrence (`unavailable`, `input_rejected`, `key_conflict`).
    pub refusal: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_labels_are_human_readable() {
        let mut schedule = ChannelSchedule {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            binding_id: Uuid::new_v4(),
            name: "Test".to_string(),
            schedule_type: ScheduleType::Interval,
            interval_seconds: Some(3600),
            timezone: ScheduleTimezone::utc(),
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
