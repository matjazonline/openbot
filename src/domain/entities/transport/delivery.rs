//! The state machine one durable delivery moves through, and the classification of why a send
//! did not succeed.
//!
//! A delivery is one attempt to expose one canonical message through one protocol interface. It
//! owns the lease; its parts own the provider results. The vocabulary lives in the domain because
//! both the queue that leases the row and the reader that renders it need the same words, and
//! because the one decision worth stating once -- what a parent's status is, given its parts --
//! is a pure function of them.
//!
//! Every enum here is written twice: as a variant list and as a SQL `CHECK`. `ALL` is what the
//! database-equivalence test iterates so the two cannot drift.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::{BindingDeliveryPolicy, InvalidTransportValue, stored_enum};

stored_enum! {
    /// Why this delivery exists.
    ///
    /// A closed set, because "should this message go out?" must be answered by an explicit policy
    /// entry rather than inferred from direction or role -- the heuristic route is how a system
    /// starts mirroring a category nobody decided to mirror.
    DeliveryPurpose as "delivery purpose" {
        /// The answer to something that arrived on this interface.
        Reply => "reply",
        /// The same message exposed through another eligible interface.
        Mirror => "mirror",
        /// The agent opening a conversation with a third party.
        Outreach => "outreach",
        /// A platform notice: a bounce, an approval request, a stop notice, a schedule digest.
        Notification => "notification",
    }
}

impl DeliveryPurpose {
    /// Whether this purpose starts a conversation rather than continuing one.
    ///
    /// A binding whose policy is [`BindingDeliveryPolicy::ReplyOnly`] carries the continuing kinds
    /// and refuses the starting kind; that is the entire meaning of the policy, stated once.
    pub const fn initiates_conversation(self) -> bool {
        match self {
            Self::Outreach => true,
            Self::Reply | Self::Mirror | Self::Notification => false,
        }
    }

    pub const fn permitted_by(self, policy: BindingDeliveryPolicy) -> bool {
        match policy {
            BindingDeliveryPolicy::ReplyAndInitiate => true,
            BindingDeliveryPolicy::ReplyOnly => !self.initiates_conversation(),
        }
    }

    /// What a reader calls this, so one delivery reads the same in a filter, a list and a pane.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reply => "Reply",
            Self::Mirror => "Mirror",
            Self::Outreach => "Outreach",
            Self::Notification => "Notification",
        }
    }
}

stored_enum! {
    /// Where one delivery stands.
    ///
    /// Only [`Self::Sending`] holds a lease; the SQL `CHECK` states the same rule, so a terminal
    /// row cannot retain the worker id that last touched it and be mistaken for work in flight.
    DeliveryStatus as "delivery status" {
        /// Queued, waiting for `available_at` and a claimant.
        Pending => "pending",
        /// Claimed under a live lease and being handed to a provider right now.
        Sending => "sending",
        /// A definite failure. It comes back after its backoff, and each return costs an attempt.
        Retryable => "retryable",
        /// Every part was accepted by its provider.
        Delivered => "delivered",
        /// At least one part may or may not have been accepted. Reconcile; never blind-retry --
        /// this is the state a duplicate would come from.
        OutcomeUnknown => "outcome_unknown",
        /// Poison, or out of attempts. Needs a human.
        DeadLetter => "dead_letter",
    }
}

impl DeliveryStatus {
    /// Whether a claimant may take this row.
    ///
    /// `retryable` is claimable and `pending` is claimable; nothing else is. In particular
    /// [`Self::OutcomeUnknown`] is not: re-sending an ambiguous part is how one message becomes
    /// two.
    pub const fn is_claimable(self) -> bool {
        matches!(self, Self::Pending | Self::Retryable)
    }

    /// Whether this row still holds a lease. Exactly one status does.
    pub const fn holds_lease(self) -> bool {
        matches!(self, Self::Sending)
    }

    /// Whether the queue is finished with this row, one way or another.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Delivered | Self::OutcomeUnknown | Self::DeadLetter
        )
    }

    /// Whether a human should be looking at this row.
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::OutcomeUnknown | Self::DeadLetter)
    }

    /// One name per status, so a delivery reads the same in the filter, the list and the pane.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pending => "Queued",
            Self::Sending => "Sending",
            Self::Retryable => "Retrying",
            Self::Delivered => "Delivered",
            Self::OutcomeUnknown => "Unconfirmed",
            Self::DeadLetter => "Dead letter",
        }
    }
}

stored_enum! {
    /// Where one frozen part stands with its provider.
    ///
    /// Parts own no lease of their own. The parent delivery holds one execution, and every part
    /// transition is fenced on it -- two competing ownership state machines for one provider call
    /// is the shape this deliberately avoids.
    DeliveryPartStatus as "delivery part status" {
        /// Rendered and frozen. Nothing has been sent.
        Prepared => "prepared",
        /// The parent's live execution is inside the provider call for this part.
        Sending => "sending",
        /// The provider accepted it and, where it returns one, named its own key for it.
        Delivered => "delivered",
        /// The request may or may not have been accepted.
        OutcomeUnknown => "outcome_unknown",
        /// Definitely not accepted, and worth trying again.
        Retryable => "retryable",
        /// Definitively rejected, or out of attempts. Re-sending these bytes cannot succeed.
        Dead => "dead",
    }
}

impl DeliveryPartStatus {
    /// Whether this part still needs a provider call. Delivery resumes at the first one that does.
    pub const fn is_unfinished(self) -> bool {
        matches!(self, Self::Prepared | Self::Retryable)
    }
}

/// The parent's status, given every one of its parts.
///
/// The one decision this module exists to state once, because it is the difference between a
/// delivery that reports itself delivered on partial success and one that does not:
///
/// - a parent is [`DeliveryStatus::Delivered`] only when *every* part is delivered;
/// - one dead part is terminal poison for the whole delivery, and outranks everything;
/// - one ambiguous part keeps the parent [`DeliveryStatus::OutcomeUnknown`], because resuming it
///   would re-send a part the provider may already hold;
/// - anything else still has work to do, and comes back as [`DeliveryStatus::Retryable`].
///
/// A delivery with no parts is not delivered. An empty part list means the render produced nothing,
/// which is a defect in the renderer rather than a successful send of nothing.
pub fn aggregate_parent_status(parts: &[DeliveryPartStatus]) -> DeliveryStatus {
    if parts.is_empty() {
        return DeliveryStatus::DeadLetter;
    }
    if parts.contains(&DeliveryPartStatus::Dead) {
        return DeliveryStatus::DeadLetter;
    }
    if parts.contains(&DeliveryPartStatus::OutcomeUnknown) {
        return DeliveryStatus::OutcomeUnknown;
    }
    if parts
        .iter()
        .all(|part| *part == DeliveryPartStatus::Delivered)
    {
        return DeliveryStatus::Delivered;
    }
    DeliveryStatus::Retryable
}

stored_enum! {
    /// Why a send did not succeed, in the terms a queue transition needs.
    ///
    /// Recorded on the row so an operator sees *why* without reading a free-text detail, and so a
    /// metric can distinguish a revoked credential from a rate limit.
    FailureClass as "delivery failure class" {
        /// The credential was refused. Retrying with the same credential cannot help.
        Authentication => "authentication",
        /// The provider asked us to slow down.
        RateLimited => "rate_limited",
        /// The provider rejected what we sent. Re-sending the same bytes cannot help.
        InvalidPayload => "invalid_payload",
        /// The destination is gone.
        DestinationUnavailable => "destination_unavailable",
        /// The request did not complete: connection, DNS, TLS.
        Network => "network",
        /// The request timed out. Whether the provider acted on it is unknown.
        Timeout => "timeout",
        /// The provider reported a fault of its own.
        ProviderFault => "provider_fault",
        /// Something on our side of the call broke.
        Internal => "internal",
        /// This delivery was waiting on another that will never be delivered. Typed rather than
        /// folded into `internal`, so a dead-lettered descendant says so.
        DependencyFailed => "dependency_failed",
        /// The work this delivery belonged to was closed before it went out.
        Superseded => "superseded",
        /// The lease lapsed without the run reporting anything.
        LeaseExpired => "lease_expired",
    }
}

stored_enum! {
    /// Why somebody asked for a delivery not to go out.
    ///
    /// Recorded once, beside `cancellation_requested_at`, and never cleared: a delivery that is in
    /// flight when it is cancelled keeps the intent through whatever its provider says, so no retry,
    /// release or lease sweep can hand it back to the queue.
    DeliveryCancellationReason as "delivery cancellation reason" {
        /// The owning agent was removed from the channel its task belongs to.
        ChannelAgentRemoved => "channel_agent_removed",
        /// The task that produced the delivery was stopped.
        TaskStopped => "task_stopped",
    }
}

/// A cancellation somebody recorded on a delivery: why, and when it was first asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryCancellation {
    pub reason: DeliveryCancellationReason,
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

/// The parent's status once a cancelled delivery stops sending: what its parts prove, and no more.
///
/// It differs from [`aggregate_parent_status`] in which fact outranks which. The cancellation has
/// just marked every unsent part dead, so a dead part no longer means the provider refused
/// something -- and letting it outrank an ambiguous part would relabel a request the provider may
/// hold as definitely unsent. So:
///
/// - any part whose request may have reached the provider keeps the parent
///   [`DeliveryStatus::OutcomeUnknown`] -- including one still marked `sending`, which is a request
///   that went out and never reported;
/// - a delivery every part of which was accepted is [`DeliveryStatus::Delivered`]; the
///   cancellation came too late to matter;
/// - anything else is [`DeliveryStatus::DeadLetter`], with its delivered parts kept as the record
///   of a partial send.
pub fn cancelled_parent_status(parts: &[DeliveryPartStatus]) -> DeliveryStatus {
    if parts.iter().any(|part| {
        matches!(
            part,
            DeliveryPartStatus::OutcomeUnknown | DeliveryPartStatus::Sending
        )
    }) {
        return DeliveryStatus::OutcomeUnknown;
    }
    if !parts.is_empty()
        && parts
            .iter()
            .all(|part| *part == DeliveryPartStatus::Delivered)
    {
        return DeliveryStatus::Delivered;
    }
    DeliveryStatus::DeadLetter
}

impl FailureClass {
    /// Whether re-sending the same bytes could ever succeed.
    ///
    /// Read by the worker to decide between spending an attempt and going terminal immediately:
    /// a payload the provider will always reject costs five backoffs and reaches the same verdict.
    pub const fn is_worth_retrying(self) -> bool {
        match self {
            Self::RateLimited | Self::Network | Self::Timeout | Self::ProviderFault => true,
            Self::Authentication
            | Self::InvalidPayload
            | Self::DestinationUnavailable
            | Self::Internal
            | Self::DependencyFailed
            | Self::Superseded
            | Self::LeaseExpired => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_parent_never_relabels_uncertainty_as_unsent() {
        use DeliveryPartStatus::{Dead, Delivered, OutcomeUnknown, Sending};

        assert_eq!(
            cancelled_parent_status(&[Delivered, OutcomeUnknown, Dead]),
            DeliveryStatus::OutcomeUnknown
        );
        assert_eq!(
            cancelled_parent_status(&[Sending, Dead]),
            DeliveryStatus::OutcomeUnknown
        );
        assert_eq!(
            cancelled_parent_status(&[Delivered, Delivered]),
            DeliveryStatus::Delivered
        );
        assert_eq!(
            cancelled_parent_status(&[Delivered, Dead]),
            DeliveryStatus::DeadLetter
        );
        assert_eq!(cancelled_parent_status(&[]), DeliveryStatus::DeadLetter);
        for reason in DeliveryCancellationReason::ALL {
            assert_eq!(
                Ok(*reason),
                DeliveryCancellationReason::from_str(reason.as_str())
            );
        }
    }

    #[test]
    fn every_vocabulary_round_trips_through_its_stored_string() {
        for purpose in DeliveryPurpose::ALL {
            assert_eq!(Ok(*purpose), DeliveryPurpose::from_str(purpose.as_str()));
        }
        for status in DeliveryStatus::ALL {
            assert_eq!(Ok(*status), DeliveryStatus::from_str(status.as_str()));
        }
        for status in DeliveryPartStatus::ALL {
            assert_eq!(Ok(*status), DeliveryPartStatus::from_str(status.as_str()));
        }
        for class in FailureClass::ALL {
            assert_eq!(Ok(*class), FailureClass::from_str(class.as_str()));
        }
        assert!(DeliveryStatus::from_str("sent").is_err());
    }

    /// Exactly one status may carry lease columns, and the claim set is exactly the two that carry
    /// none but still owe a send. Stated as a test because the SQL `CHECK` and the claim predicate
    /// are both derived from these two functions.
    #[test]
    fn only_sending_holds_a_lease_and_only_unfinished_rows_are_claimable() {
        let leased: Vec<_> = DeliveryStatus::ALL
            .iter()
            .filter(|status| status.holds_lease())
            .collect();
        assert_eq!(vec![&DeliveryStatus::Sending], leased);

        let claimable: Vec<_> = DeliveryStatus::ALL
            .iter()
            .filter(|status| status.is_claimable())
            .collect();
        assert_eq!(
            vec![&DeliveryStatus::Pending, &DeliveryStatus::Retryable],
            claimable
        );
        // An ambiguous outcome is the one case a retry would turn into a duplicate.
        assert!(!DeliveryStatus::OutcomeUnknown.is_claimable());
    }

    #[test]
    fn a_parent_is_delivered_only_when_every_part_is() {
        use DeliveryPartStatus::*;

        assert_eq!(
            DeliveryStatus::Delivered,
            aggregate_parent_status(&[Delivered, Delivered])
        );
        assert_eq!(
            DeliveryStatus::Retryable,
            aggregate_parent_status(&[Delivered, Prepared])
        );
        assert_eq!(
            DeliveryStatus::Retryable,
            aggregate_parent_status(&[Delivered, Retryable])
        );
        // Ambiguity outranks the delivered parts beside it: resuming would re-send that part.
        assert_eq!(
            DeliveryStatus::OutcomeUnknown,
            aggregate_parent_status(&[Delivered, OutcomeUnknown, Prepared])
        );
        // Poison outranks ambiguity: there is nothing left to reconcile.
        assert_eq!(
            DeliveryStatus::DeadLetter,
            aggregate_parent_status(&[OutcomeUnknown, Dead])
        );
        // A render that froze nothing is a defect, not a delivery of nothing.
        assert_eq!(DeliveryStatus::DeadLetter, aggregate_parent_status(&[]));
    }

    #[test]
    fn a_reply_only_interface_carries_every_purpose_but_outreach() {
        for purpose in DeliveryPurpose::ALL {
            assert!(purpose.permitted_by(BindingDeliveryPolicy::ReplyAndInitiate));
            assert_eq!(
                *purpose != DeliveryPurpose::Outreach,
                purpose.permitted_by(BindingDeliveryPolicy::ReplyOnly)
            );
        }
    }
}
