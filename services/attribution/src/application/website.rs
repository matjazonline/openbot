//! Untrusted observations and the immutable merchant checkout selection policy.
use super::{
    catalog::{ClickId, LinkId},
    identity::{Failure, Result},
    ids::entity_id,
    setup::InstallationId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
entity_id!(ObservationId);
pub const POLICY: &str = "last-eligible-click-30d-v1";
pub const RETRY_HOURS: i64 = 24;

pub fn timestamp<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<DateTime<Utc>, D::Error> {
    let raw = String::deserialize(d)?;
    parse_time(&raw).map_err(|_| serde::de::Error::custom("invalid RFC3339 timestamp"))
}
pub fn parse_time(raw: &str) -> Result<DateTime<Utc>> {
    if raw.len() > 40 || !matches!(raw.as_bytes().get(10), Some(b'T' | b't')) {
        return Err(Failure::Invalid);
    }
    DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| Failure::Invalid)
}
pub fn horizon(original: DateTime<Utc>, now: DateTime<Utc>) -> Result<()> {
    if original < now - chrono::Duration::hours(RETRY_HOURS) {
        Err(Failure::Gone)
    } else if original > now + chrono::Duration::seconds(30) {
        Err(Failure::Invalid)
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    ImageLoad,
    LandingPageLoad,
    VisiblePlacement,
}
impl ObservationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ImageLoad => "image_load",
            Self::LandingPageLoad => "landing_page_load",
            Self::VisiblePlacement => "visible_placement",
        }
    }
}
fn nullable<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub installation_id: InstallationId,
    pub event_id: ObservationId,
    pub kind: ObservationKind,
    #[serde(deserialize_with = "nullable")]
    pub click_id: Option<ClickId>,
    #[serde(deserialize_with = "nullable")]
    pub link_id: Option<LinkId>,
    #[serde(deserialize_with = "timestamp")]
    pub observed_at: DateTime<Utc>,
    pub consent: bool,
}
impl Observation {
    pub fn validate(&self) -> Result<()> {
        if !self.consent || (self.click_id.is_none() && self.link_id.is_none()) {
            Err(Failure::Invalid)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub struct CheckoutId(pub String);
impl TryFrom<String> for CheckoutId {
    type Error = &'static str;
    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        if value.is_empty()
            || value.chars().count() > 128
            || value.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("invalid checkout identifier");
        }
        Ok(Self(value))
    }
}
impl From<CheckoutId> for String {
    fn from(id: CheckoutId) -> String {
        id.0
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckoutWrite {
    pub checkout_id: CheckoutId,
    #[serde(deserialize_with = "timestamp")]
    pub order_created_at: DateTime<Utc>,
    pub consent: bool,
    pub candidate_click_ids: Vec<ClickId>,
}
impl CheckoutWrite {
    pub fn normalize(mut self) -> Result<Self> {
        if self.candidate_click_ids.len() > 32 {
            return Err(Failure::Invalid);
        }
        self.candidate_click_ids.sort_by_key(|id| id.0);
        if self.candidate_click_ids.windows(2).any(|w| w[0] == w[1]) {
            return Err(Failure::Invalid);
        }
        Ok(self)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttributionReason {
    Attributed,
    Missing,
    NoConsent,
    Expired,
    Future,
    Bot,
    ForeignOrMissing,
    EvidenceUnavailable,
}
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Checkout {
    pub checkout_id: CheckoutId,
    pub order_created_at: DateTime<Utc>,
    pub click_id: Option<ClickId>,
    pub reason: AttributionReason,
    pub policy: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: ClickId,
    pub clicked_at: DateTime<Utc>,
    pub is_bot: bool,
    pub is_prefetch: bool,
    pub classifier_version: i16,
    pub dimensions: Value,
}
/// Candidates are already restricted to the credential's company/store by SQL.
pub fn select(w: &CheckoutWrite, candidates: Vec<Candidate>) -> (Checkout, Option<Candidate>) {
    let mut result = Checkout {
        checkout_id: w.checkout_id.clone(),
        order_created_at: w.order_created_at,
        click_id: None,
        reason: AttributionReason::Missing,
        policy: POLICY.into(),
    };
    if !w.consent {
        result.reason = AttributionReason::NoConsent;
        return (result, None);
    }
    let earliest = w.order_created_at - chrono::Duration::days(30);
    let selected = candidates
        .iter()
        .filter(|c| {
            c.clicked_at >= earliest
                && c.clicked_at <= w.order_created_at
                && !c.is_bot
                && !c.is_prefetch
        })
        .max_by_key(|c| (c.clicked_at, c.id.0))
        .cloned();
    result.reason = if let Some(c) = &selected {
        result.click_id = Some(c.id);
        AttributionReason::Attributed
    } else if w.candidate_click_ids.is_empty() {
        AttributionReason::Missing
    } else if candidates.len() < w.candidate_click_ids.len() {
        AttributionReason::ForeignOrMissing
    } else if candidates.iter().any(|c| c.clicked_at > w.order_created_at) {
        AttributionReason::Future
    } else if candidates.iter().any(|c| c.clicked_at < earliest) {
        AttributionReason::Expired
    } else {
        AttributionReason::Bot
    };
    (result, selected)
}
