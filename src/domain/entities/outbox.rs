//! The email outbox as a reader sees it: one queued send, what the transport has done with it so
//! far, and the paging that lists them.
//!
//! The outbox is written by whoever composed the mail and drained by the poller; nothing here
//! writes back. That is why this module is all reading: a status, an entry, and a filter.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where one queued email stands with the transport.
///
/// The column is a `TEXT` with a check constraint, so it is parsed into this once at the
/// persistence boundary and matched exhaustively everywhere above it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutboxStatus {
    /// Queued, waiting for `available_at` and a poller to claim it.
    Pending,
    /// Claimed by a poller and being handed to the provider right now.
    Sending,
    Sent,
    /// Every attempt was used up; this email will not be delivered without intervention.
    Failed,
}

impl OutboxStatus {
    /// The value stored in `email_outbox.status`, and what a filter puts in the query string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }

    /// One name per status, so an email reads the same in the filter, the list and the pane.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Queued",
            Self::Sending => "Sending",
            Self::Sent => "Sent",
            Self::Failed => "Failed",
        }
    }

    /// The statuses a filter offers, in the order an email moves through them.
    pub const ALL: [Self; 4] = [Self::Pending, Self::Sending, Self::Sent, Self::Failed];
}

impl FromStr for OutboxStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("Unknown outbox status: {value}")),
        }
    }
}

/// One row of `email_outbox`: an email that was composed, handed off, and is now the transport's
/// problem.
///
/// `payload` is the serialized `OutboundEmail` the poller will deliver. It is read through the
/// accessors below rather than dug into at each call site, so the JSON shape is known in one place.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEntry {
    pub id: Uuid,
    pub company_id: Uuid,
    /// The channel this email goes out as. `None` once that channel is deleted — the record of the
    /// send outlives it.
    pub channel_id: Option<Uuid>,
    /// The task whose run produced this email, when one did. Nothing writes back through it — it
    /// is the join that lets a queued email point at the work that created it.
    pub task_id: Option<Uuid>,
    pub status: OutboxStatus,
    /// Stable across every retry of the same logical send, and what the delivered Message-ID is
    /// derived from.
    pub idempotency_key: String,
    pub payload: serde_json::Value,
    pub retry_count: i32,
    pub last_error: Option<String>,
    /// The provider's id for the delivered message, once it has one.
    pub provider_message_id: Option<String>,
    /// When the next attempt becomes eligible, while the row is still pending.
    pub available_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OutboxEntry {
    pub fn subject(&self) -> Option<&str> {
        self.payload_str("subject")
    }

    pub fn recipient(&self) -> Option<&str> {
        self.payload_str("recipient_to")
    }

    /// What the channel was called when this email was composed. The display fallback for a row
    /// whose channel has since been deleted, so a name still shows where [`Self::channel_id`] no
    /// longer resolves.
    pub fn channel_name(&self) -> Option<&str> {
        self.payload_str("channel_name")
    }

    pub fn recipients_cc(&self) -> Vec<&str> {
        self.payload
            .get("recipients_cc")
            .and_then(|value| value.as_array())
            .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
            .unwrap_or_default()
    }

    fn payload_str(&self, key: &str) -> Option<&str> {
        self.payload
            .get(key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
    }
}

/// Which page of the outbox a request means.
///
/// Mirrors [`crate::entities::task::TaskFilter`]: the same clamping, the same probe-for-next-page
/// trick, so the two queue views cannot disagree about what `?page=2` contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxFilter {
    pub channel_id: Option<Uuid>,
    pub status: Option<OutboxStatus>,
    /// Oldest first when set; the list is newest first otherwise.
    pub sort_asc: bool,
    page: usize,
    limit: usize,
}

impl OutboxFilter {
    pub const DEFAULT_PAGE_SIZE: usize = 50;
    pub const MAX_PAGE_SIZE: usize = 100;

    pub fn new(
        channel_id: Option<Uuid>,
        status: Option<OutboxStatus>,
        sort_asc: bool,
        page: Option<usize>,
        limit: Option<usize>,
    ) -> Self {
        Self {
            channel_id,
            status,
            sort_asc,
            page: page.unwrap_or(1).max(1),
            limit: limit
                .unwrap_or(Self::DEFAULT_PAGE_SIZE)
                .clamp(1, Self::MAX_PAGE_SIZE),
        }
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// How many rows this page skips, saturating rather than wrapping on an absurd `?page=`.
    pub fn offset(&self) -> i64 {
        self.page
            .saturating_sub(1)
            .saturating_mul(self.limit)
            .min(i64::MAX as usize) as i64
    }

    /// One row more than the page needs, so whether a next page exists comes out of the same query
    /// instead of a second count.
    pub fn probe_limit(&self) -> i64 {
        self.limit.saturating_add(1) as i64
    }

    /// Splits a [`Self::probe_limit`]-sized read into the page itself and whether one follows it.
    pub fn split_probe(&self, mut entries: Vec<OutboxEntry>) -> (Vec<OutboxEntry>, bool) {
        let has_next = entries.len() > self.limit;
        entries.truncate(self.limit);
        (entries, has_next)
    }

    pub fn on_page(self, page: usize) -> Self {
        Self {
            page: page.max(1),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(payload: serde_json::Value) -> OutboxEntry {
        OutboxEntry {
            id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            channel_id: None,
            task_id: None,
            status: OutboxStatus::Pending,
            idempotency_key: "key".to_string(),
            payload,
            retry_count: 0,
            last_error: None,
            provider_message_id: None,
            available_at: Utc::now(),
            sent_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn status_round_trips_through_its_stored_string() {
        for status in OutboxStatus::ALL {
            assert_eq!(Ok(status), OutboxStatus::from_str(status.as_str()));
        }
        assert!(OutboxStatus::from_str("delivered").is_err());
    }

    #[test]
    fn payload_accessors_read_the_outbound_email_shape() {
        let entry = entry(serde_json::json!({
            "channel_name": "Support",
            "recipient_to": "customer@example.com",
            "recipients_cc": ["cc@example.com", "second@example.com"],
            "subject": "Re: order",
        }));

        assert_eq!(Some("Re: order"), entry.subject());
        assert_eq!(Some("customer@example.com"), entry.recipient());
        assert_eq!(Some("Support"), entry.channel_name());
        assert_eq!(
            vec!["cc@example.com", "second@example.com"],
            entry.recipients_cc()
        );
    }

    #[test]
    fn payload_accessors_treat_missing_and_empty_alike() {
        let entry = entry(serde_json::json!({ "subject": "" }));

        assert_eq!(None, entry.subject());
        assert_eq!(None, entry.recipient());
        assert_eq!(None, entry.channel_name());
        assert!(entry.recipients_cc().is_empty());
    }

    #[test]
    fn paging_clamps_what_a_request_asks_for() {
        let filter = OutboxFilter::new(None, None, false, Some(0), Some(5_000));
        assert_eq!(1, filter.page());
        assert_eq!(OutboxFilter::MAX_PAGE_SIZE, filter.limit());
        assert_eq!(0, filter.offset());

        let third = filter.on_page(3);
        assert_eq!(2 * OutboxFilter::MAX_PAGE_SIZE as i64, third.offset());
    }

    #[test]
    fn paging_to_another_page_keeps_what_was_filtered() {
        let channel_id = Uuid::new_v4();
        let filter = OutboxFilter::new(
            channel_id.into(),
            Some(OutboxStatus::Failed),
            true,
            None,
            None,
        );

        let next = filter.on_page(4);
        assert_eq!(Some(channel_id), next.channel_id);
        assert_eq!(Some(OutboxStatus::Failed), next.status);
        assert!(next.sort_asc);
        assert_eq!(4, next.page());
    }

    #[test]
    fn probe_row_reports_a_next_page_without_being_shown() {
        let filter = OutboxFilter::new(None, None, false, None, Some(2));
        let probed = vec![
            entry(serde_json::json!({})),
            entry(serde_json::json!({})),
            entry(serde_json::json!({})),
        ];

        let (page, has_next) = filter.split_probe(probed);
        assert_eq!(2, page.len());
        assert!(has_next);

        let (page, has_next) = filter.split_probe(vec![entry(serde_json::json!({}))]);
        assert_eq!(1, page.len());
        assert!(!has_next);
    }
}
