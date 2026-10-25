//! Deliveries that have stopped moving, counted so somebody can be told about them.
//!
//! Every kind of stall here is already visible in the delivery queue -- a dead-lettered delivery,
//! one queued past its send time, one whose provider outcome nobody resolved. What was missing is
//! anything that *looks*. The dashboard shows these numbers to a human who opens the page; this is
//! for the case where nobody opens the page. The delivery worker publishes it on every reap.
//!
//! # Why a census rather than events
//!
//! A stall is a state, not an occurrence. A delivery dead-letters once but stays dead-lettered, so
//! counting it on every sweep would inflate a counter without the underlying situation changing.
//! Each figure here is therefore a *gauge*: how much is stuck right now. It goes to zero on its
//! own when the work is dealt with, which is exactly what an alert should do.

use std::time::Duration;

/// How far past its send time a queued delivery may be before the queue counts as not draining.
/// Short: nothing should sit here, so anything that does is news.
pub const DELIVERY_OVERDUE_AFTER: Duration = Duration::from_secs(5 * 60);

/// One kind of stall. A closed set, because these become metric labels and an open-ended label is
/// how a metrics backend gets taken down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StuckWorkKind {
    /// Deliveries that will not go out without intervention.
    DeliveryDeadLettered,
    /// Deliveries queued and past due. The delivery worker is not draining.
    DeliveryOverdue,
    /// Deliveries whose provider outcome was never resolved. Nothing will retry these -- doing so
    /// is exactly how a duplicate is sent -- so they wait for a reconciler or for a human.
    DeliveryUnconfirmed,
}

impl StuckWorkKind {
    pub const ALL: [Self; 3] = [
        Self::DeliveryDeadLettered,
        Self::DeliveryOverdue,
        Self::DeliveryUnconfirmed,
    ];

    /// The metric label, and the machine-readable name in the log line.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeliveryDeadLettered => "delivery_dead_lettered",
            Self::DeliveryOverdue => "delivery_overdue",
            Self::DeliveryUnconfirmed => "delivery_unconfirmed",
        }
    }

    /// What an operator should understand from a non-zero count, in one line.
    pub const fn description(self) -> &'static str {
        match self {
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

/// How many deliveries are stuck, by kind, at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StuckWorkCensus {
    pub delivery_dead_lettered: i64,
    pub delivery_overdue: i64,
    pub delivery_unconfirmed: i64,
}

impl StuckWorkCensus {
    pub fn count(&self, kind: StuckWorkKind) -> i64 {
        match kind {
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
            delivery_dead_lettered: 2,
            ..StuckWorkCensus::default()
        };
        assert_eq!(census.gauges().count(), StuckWorkKind::ALL.len());
        assert_eq!(
            census.alerts().collect::<Vec<_>>(),
            vec![(StuckWorkKind::DeliveryDeadLettered, 2)]
        );
    }

    #[test]
    fn every_kind_is_reachable_from_the_census() {
        // Guards against a field being added to the census and silently never reported.
        for kind in StuckWorkKind::ALL {
            let mut census = StuckWorkCensus::default();
            match kind {
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
