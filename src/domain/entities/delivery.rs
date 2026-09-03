//! The delivery queue as a reader sees it: one attempt to put a message on an interface, what the
//! provider has done with it so far, and the paging that lists them.
//!
//! Written by whoever produced the message and drained by the delivery worker; nothing here writes
//! back. That is why this module is all reading: a filter, an entry, and the projection of its
//! parts.
//!
//! It replaces an outbox that could only be an email outbox. What made the old projection
//! email-shaped was not the name but the shape: one provider id per row, a subject and a recipient
//! dug out of a JSON payload with `payload.get("subject")`, and a status vocabulary with no word
//! for "the provider may or may not have this". A reader now sees the transport, the purpose, the
//! interface and the canonical subject as columns, and every provider key its parts earned.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::entities::{
    correlation::CorrelationId,
    message::CanonicalMessageId,
    transport::{
        DeliveryId, DeliveryPartId, DeliveryPartStatus, DeliveryPurpose, DeliveryStatus,
        FailureClass, TransportKind,
    },
};

/// One row of `message_deliveries`, with the joins a reader needs to name what it sees.
///
/// The subject and the interface label are joined rather than copied onto the row: the canonical
/// message owns what the delivery is about, and the binding owns what its interface is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryEntry {
    pub id: DeliveryId,
    pub company_id: Uuid,
    /// The channel this goes out as. Still present after the channel is renamed; the delivery is
    /// deleted with the channel, so this never dangles.
    pub channel_id: Uuid,
    pub message_id: CanonicalMessageId,
    /// The task whose work produced this, when one did. Nothing writes back through it -- it is
    /// the join that lets a queued delivery point at the work that created it.
    pub task_id: Option<Uuid>,
    pub correlation_id: CorrelationId,
    pub transport: TransportKind,
    pub purpose: DeliveryPurpose,
    pub status: DeliveryStatus,
    /// Stable across every attempt at the same logical delivery.
    pub idempotency_key: String,
    /// What the destination interface is called, from its binding.
    pub destination_label: String,
    /// The recipient named inside that interface, when one was named rather than the interface
    /// itself being the destination.
    pub external_destination: Option<String>,
    /// What the delivery is about, from the canonical message it carries.
    pub subject: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub last_error_class: Option<FailureClass>,
    pub last_error_detail: Option<String>,
    /// This delivery's frozen parts, in index order.
    pub parts: Vec<DeliveryPartEntry>,
    /// When the next attempt becomes eligible, while the delivery is still claimable.
    pub available_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DeliveryEntry {
    /// How many attempts remain before this is dead-lettered. Saturating, because a reaped row can
    /// reach its cap exactly.
    pub const fn attempts_remaining(&self) -> i32 {
        self.max_attempts.saturating_sub(self.attempt_count)
    }

    /// The provider keys this delivery earned, in part order.
    ///
    /// A list rather than one value: a long answer goes out as several provider messages, and the
    /// single `provider_message_id` column this replaces could only ever name the last one.
    pub fn provider_message_keys(&self) -> impl Iterator<Item = &str> {
        self.parts
            .iter()
            .filter_map(|part| part.provider_message_key.as_deref())
    }

    /// How much of the delivery the provider has confirmed, as `(delivered, total)`.
    pub fn part_progress(&self) -> (usize, usize) {
        let delivered = self
            .parts
            .iter()
            .filter(|part| part.status == DeliveryPartStatus::Delivered)
            .count();
        (delivered, self.parts.len())
    }
}

/// One frozen part of a delivery, as a reader sees it.
///
/// The rendered payload is deliberately absent. It is the transport's own wire shape, and a reader
/// that dug into it would be re-deriving the subject and recipient that the columns above already
/// state -- which is exactly what the projection this replaces did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryPartEntry {
    pub id: DeliveryPartId,
    pub index: u16,
    pub status: DeliveryPartStatus,
    pub provider_message_key: Option<String>,
    pub attempt_count: i32,
    pub last_error_class: Option<FailureClass>,
    pub last_error_detail: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Which page of the delivery queue a request means.
///
/// Mirrors [`crate::entities::task::TaskFilter`]: the same clamping, the same probe-for-next-page
/// trick, so the two queue views cannot disagree about what `?page=2` contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryFilter {
    pub channel_id: Option<Uuid>,
    pub status: Option<DeliveryStatus>,
    /// Narrows to one protocol, so "is Slack backing up?" is one click rather than a scan.
    pub transport: Option<TransportKind>,
    pub purpose: Option<DeliveryPurpose>,
    /// Oldest first when set; the list is newest first otherwise.
    pub sort_asc: bool,
    page: usize,
    limit: usize,
}

/// What a request asked the delivery list for, before clamping.
///
/// A named struct rather than five positional arguments: `channel_id`, `status`, `transport` and
/// `purpose` are four optional filters in a row, and `src/AGENTS.md` is explicit that a call site
/// spelling `(None, None, Some(x), None, false, ..)` is where a transposition hides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryQuery {
    pub channel_id: Option<Uuid>,
    pub status: Option<DeliveryStatus>,
    pub transport: Option<TransportKind>,
    pub purpose: Option<DeliveryPurpose>,
    pub sort_asc: bool,
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

impl DeliveryFilter {
    pub const DEFAULT_PAGE_SIZE: usize = 50;
    pub const MAX_PAGE_SIZE: usize = 100;

    pub fn new(query: DeliveryQuery) -> Self {
        Self {
            channel_id: query.channel_id,
            status: query.status,
            transport: query.transport,
            purpose: query.purpose,
            sort_asc: query.sort_asc,
            page: query.page.unwrap_or(1).max(1),
            limit: query
                .limit
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
    pub fn split_probe(&self, mut entries: Vec<DeliveryEntry>) -> (Vec<DeliveryEntry>, bool) {
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

    fn entry(parts: Vec<DeliveryPartEntry>) -> DeliveryEntry {
        DeliveryEntry {
            id: DeliveryId::random(),
            company_id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            message_id: CanonicalMessageId::random(),
            task_id: None,
            correlation_id: CorrelationId::new(),
            transport: TransportKind::Email,
            purpose: DeliveryPurpose::Reply,
            status: DeliveryStatus::Pending,
            idempotency_key: "reply:key".to_string(),
            destination_label: "support@acme".to_string(),
            external_destination: Some("customer@example.com".to_string()),
            subject: "Re: order".to_string(),
            attempt_count: 1,
            max_attempts: 5,
            last_error_class: None,
            last_error_detail: None,
            parts,
            available_at: Utc::now(),
            delivered_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn part(index: u16, status: DeliveryPartStatus, key: Option<&str>) -> DeliveryPartEntry {
        DeliveryPartEntry {
            id: DeliveryPartId::random(),
            index,
            status,
            provider_message_key: key.map(str::to_string),
            attempt_count: 0,
            last_error_class: None,
            last_error_detail: None,
            delivered_at: None,
        }
    }

    /// The single `provider_message_id` column this replaces could name only one provider message.
    /// A multi-part delivery has one per part, and the reader has to be able to see all of them.
    #[test]
    fn every_part_contributes_its_own_provider_key() {
        let entry = entry(vec![
            part(0, DeliveryPartStatus::Delivered, Some("<a@example.com>")),
            part(1, DeliveryPartStatus::Delivered, Some("<b@example.com>")),
            part(2, DeliveryPartStatus::Prepared, None),
        ]);

        assert_eq!(
            vec!["<a@example.com>", "<b@example.com>"],
            entry.provider_message_keys().collect::<Vec<_>>()
        );
        assert_eq!((2, 3), entry.part_progress());
        assert_eq!(4, entry.attempts_remaining());
    }

    #[test]
    fn paging_clamps_what_a_request_asks_for() {
        let filter = DeliveryFilter::new(DeliveryQuery {
            page: Some(0),
            limit: Some(5_000),
            ..DeliveryQuery::default()
        });
        assert_eq!(1, filter.page());
        assert_eq!(DeliveryFilter::MAX_PAGE_SIZE, filter.limit());
        assert_eq!(0, filter.offset());

        let third = filter.on_page(3);
        assert_eq!(2 * DeliveryFilter::MAX_PAGE_SIZE as i64, third.offset());
    }

    #[test]
    fn paging_to_another_page_keeps_what_was_filtered() {
        let channel_id = Uuid::new_v4();
        let filter = DeliveryFilter::new(DeliveryQuery {
            channel_id: Some(channel_id),
            status: Some(DeliveryStatus::DeadLetter),
            transport: Some(TransportKind::Email),
            purpose: Some(DeliveryPurpose::Outreach),
            sort_asc: true,
            page: None,
            limit: None,
        });

        let next = filter.on_page(4);
        assert_eq!(Some(channel_id), next.channel_id);
        assert_eq!(Some(DeliveryStatus::DeadLetter), next.status);
        assert_eq!(Some(TransportKind::Email), next.transport);
        assert_eq!(Some(DeliveryPurpose::Outreach), next.purpose);
        assert!(next.sort_asc);
        assert_eq!(4, next.page());
    }

    #[test]
    fn probe_row_reports_a_next_page_without_being_shown() {
        let filter = DeliveryFilter::new(DeliveryQuery {
            limit: Some(2),
            ..DeliveryQuery::default()
        });
        let probed = vec![entry(Vec::new()), entry(Vec::new()), entry(Vec::new())];

        let (page, has_next) = filter.split_probe(probed);
        assert_eq!(2, page.len());
        assert!(has_next);

        let (page, has_next) = filter.split_probe(vec![entry(Vec::new())]);
        assert_eq!(1, page.len());
        assert!(!has_next);
    }
}
