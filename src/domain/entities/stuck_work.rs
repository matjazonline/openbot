//! Work that has stopped moving, counted so somebody can be told about it.
//!
//! Every kind of stall here is already visible in the queue tables -- a dead-lettered task, an
//! delivery that failed, an approval nobody answered. What was missing is anything that *looks*.
//! The dashboard shows these numbers to a human who opens the page; this is for the case where
//! nobody opens the page.
//!
//! # Why a census rather than events
//!
//! A stall is a state, not an occurrence. A task dead-letters once but stays dead-lettered, so
//! counting it on every sweep would inflate a counter without the underlying situation changing.
//! Each figure here is therefore a *gauge*: how much work is stuck right now. It goes to zero on
//! its own when the work is dealt with, which is exactly what an alert should do.

use std::time::Duration;

/// How late something has to be before it counts as stuck.
///
/// Held as a value rather than loose `Duration`s because the two are the same type and mean
/// opposite things: one measures a queue that is not draining, the other work that is waiting on a
/// human and is *expected* to sit for a while.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StuckWorkThresholds {
    /// How far past its `run_at` a queued task may be before the queue counts as not draining.
    /// Short: nothing should sit here, so anything that does is news.
    queue_overdue_after: Duration,
    /// How long work may sit parked on a human or a third party before it is worth mentioning.
    /// Long: parking is the normal, designed behaviour, and alerting at an hour would cry wolf
    /// every time somebody read their approval mail after lunch.
    parked_overdue_after: Duration,
}

impl StuckWorkThresholds {
    pub const fn new(queue_overdue_after: Duration, parked_overdue_after: Duration) -> Self {
        Self {
            queue_overdue_after,
            parked_overdue_after,
        }
    }

    pub const fn queue_overdue_after(&self) -> Duration {
        self.queue_overdue_after
    }

    pub const fn parked_overdue_after(&self) -> Duration {
        self.parked_overdue_after
    }
}

impl Default for StuckWorkThresholds {
    fn default() -> Self {
        Self::new(Duration::from_secs(5 * 60), Duration::from_secs(24 * 3600))
    }
}

/// One kind of stall. A closed set, because these become metric labels and an open-ended label is
/// how a metrics backend gets taken down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckWorkKind {
    /// Retries exhausted. Nothing will pick these up again without a human.
    DeadLettered,
    /// Queued and past due. The workers are not keeping up, or are not running.
    QueueOverdue,
    /// `processing` with a dead lease. The reaper turns these back into pending rows, so a figure
    /// that stays above zero across sweeps means the reaper itself is failing.
    LeaseExpired,
    /// Parked on an approval nobody has answered.
    ApprovalOverdue,
    /// Parked on a third party who has not replied, past the point the outreach allowed for.
    ReplyOverdue,
    /// Deliveries that will not go out without intervention.
    DeliveryDeadLettered,
    /// Deliveries queued and past due. The delivery worker is not draining.
    DeliveryOverdue,
    /// Deliveries whose provider outcome was never resolved. Nothing will retry these -- doing so
    /// is exactly how a duplicate is sent -- so they wait for a reconciler or for a human.
    DeliveryUnconfirmed,
}

impl StuckWorkKind {
    pub const ALL: [Self; 8] = [
        Self::DeadLettered,
        Self::QueueOverdue,
        Self::LeaseExpired,
        Self::ApprovalOverdue,
        Self::ReplyOverdue,
        Self::DeliveryDeadLettered,
        Self::DeliveryOverdue,
        Self::DeliveryUnconfirmed,
    ];

    /// The metric label, and the machine-readable name in the log line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeadLettered => "dead_lettered",
            Self::QueueOverdue => "queue_overdue",
            Self::LeaseExpired => "lease_expired",
            Self::ApprovalOverdue => "approval_overdue",
            Self::ReplyOverdue => "reply_overdue",
            Self::DeliveryDeadLettered => "delivery_dead_lettered",
            Self::DeliveryOverdue => "delivery_overdue",
            Self::DeliveryUnconfirmed => "delivery_unconfirmed",
        }
    }

    /// What an operator should understand from a non-zero count, in one line.
    pub const fn description(self) -> &'static str {
        match self {
            Self::DeadLettered => "tasks exhausted their retries and will not run again",
            Self::QueueOverdue => "tasks are queued past their run time and are not being claimed",
            Self::LeaseExpired => "tasks hold an expired lease and have not been reaped",
            Self::ApprovalOverdue => "tasks are parked on an approval nobody has answered",
            Self::ReplyOverdue => "tasks are parked on a third party past their reply deadline",
            Self::DeliveryDeadLettered => {
                "deliveries exhausted their attempts and will not be retried"
            }
            Self::DeliveryOverdue => {
                "deliveries are queued past their send time and are not going out"
            }
            Self::DeliveryUnconfirmed => {
                "deliveries reached no definite provider outcome and are awaiting reconciliation"
            }
        }
    }
}

/// How much work is stuck, by kind, at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StuckWorkCensus {
    pub dead_lettered: i64,
    pub queue_overdue: i64,
    pub lease_expired: i64,
    pub approval_overdue: i64,
    pub reply_overdue: i64,
    pub delivery_dead_lettered: i64,
    pub delivery_overdue: i64,
    pub delivery_unconfirmed: i64,
}

impl StuckWorkCensus {
    pub fn count(&self, kind: StuckWorkKind) -> i64 {
        match kind {
            StuckWorkKind::DeadLettered => self.dead_lettered,
            StuckWorkKind::QueueOverdue => self.queue_overdue,
            StuckWorkKind::LeaseExpired => self.lease_expired,
            StuckWorkKind::ApprovalOverdue => self.approval_overdue,
            StuckWorkKind::ReplyOverdue => self.reply_overdue,
            StuckWorkKind::DeliveryDeadLettered => self.delivery_dead_lettered,
            StuckWorkKind::DeliveryOverdue => self.delivery_overdue,
            StuckWorkKind::DeliveryUnconfirmed => self.delivery_unconfirmed,
        }
    }

    /// Every kind and its count, including the zeroes.
    ///
    /// The zeroes matter: a gauge that stops being reported is not the same as a gauge reporting
    /// zero, and an alert that silently stops firing because nothing published the metric is worse
    /// than no alert at all.
    pub fn gauges(&self) -> impl Iterator<Item = (StuckWorkKind, i64)> + '_ {
        StuckWorkKind::ALL
            .into_iter()
            .map(|kind| (kind, self.count(kind)))
    }

    /// Only the kinds worth writing a line about.
    pub fn alerts(&self) -> impl Iterator<Item = (StuckWorkKind, i64)> + '_ {
        self.gauges().filter(|(_, count)| *count > 0)
    }

    /// Whether there is nothing to report -- the common case, and the one that must stay silent.
    pub fn is_quiet(&self) -> bool {
        self.gauges().all(|(_, count)| count == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_system_says_nothing() {
        let census = StuckWorkCensus::default();
        assert!(census.is_quiet());
        assert_eq!(census.alerts().count(), 0);
    }

    #[test]
    fn zeroes_are_still_published_as_gauges() {
        // An alert that fires on "> 0" needs the zero to arrive, or it can never clear.
        let census = StuckWorkCensus {
            dead_lettered: 2,
            ..StuckWorkCensus::default()
        };
        assert_eq!(census.gauges().count(), StuckWorkKind::ALL.len());
        assert_eq!(
            census.alerts().collect::<Vec<_>>(),
            vec![(StuckWorkKind::DeadLettered, 2)]
        );
    }

    #[test]
    fn every_kind_is_reachable_from_the_census() {
        // Guards against a field being added to the census and silently never reported.
        for kind in StuckWorkKind::ALL {
            let mut census = StuckWorkCensus::default();
            match kind {
                StuckWorkKind::DeadLettered => census.dead_lettered = 1,
                StuckWorkKind::QueueOverdue => census.queue_overdue = 1,
                StuckWorkKind::LeaseExpired => census.lease_expired = 1,
                StuckWorkKind::ApprovalOverdue => census.approval_overdue = 1,
                StuckWorkKind::ReplyOverdue => census.reply_overdue = 1,
                StuckWorkKind::DeliveryDeadLettered => census.delivery_dead_lettered = 1,
                StuckWorkKind::DeliveryOverdue => census.delivery_overdue = 1,
                StuckWorkKind::DeliveryUnconfirmed => census.delivery_unconfirmed = 1,
            }
            assert_eq!(census.count(kind), 1, "{} is not wired up", kind.as_str());
            assert!(!census.is_quiet());
        }
    }

    #[test]
    fn labels_are_distinct_so_metrics_do_not_collide() {
        let mut seen = std::collections::HashSet::new();
        for kind in StuckWorkKind::ALL {
            assert!(seen.insert(kind.as_str()), "duplicate label");
            assert!(!kind.description().is_empty());
        }
    }
}
