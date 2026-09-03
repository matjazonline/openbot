//! Durable inbound-event state and its bounded classifications.
//!
//! Provider payloads remain in the short-lived inbox table. These values are the small, stable
//! vocabulary shared by the application worker, persistence adapter, and operator metrics.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::{InvalidTransportValue, stored_enum};

stored_enum! {
    /// Where one authenticated provider event stands.
    InboundEventStatus as "inbound event status" {
        Pending => "pending",
        Processing => "processing",
        Retryable => "retryable",
        Completed => "completed",
        Ignored => "ignored",
        DeadLetter => "dead_letter",
    }
}

impl InboundEventStatus {
    pub const fn is_claimable(self) -> bool {
        matches!(self, Self::Pending | Self::Retryable)
    }

    pub const fn holds_lease(self) -> bool {
        matches!(self, Self::Processing)
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Ignored | Self::DeadLetter)
    }
}

stored_enum! {
    /// A low-cardinality explanation for an event attempt that did not complete.
    InboundEventErrorClass as "inbound event error class" {
        Decode => "decode",
        InvalidPayload => "invalid_payload",
        Routing => "routing",
        Dependency => "dependency",
        RateLimited => "rate_limited",
        ProviderFault => "provider_fault",
        Deadline => "deadline",
        Internal => "internal",
        UnsupportedTransport => "unsupported_transport",
        LeaseExpired => "lease_expired",
    }
}

impl InboundEventErrorClass {
    pub const fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Dependency
                | Self::RateLimited
                | Self::ProviderFault
                | Self::Deadline
                | Self::Internal
                | Self::LeaseExpired
        )
    }
}

stored_enum! {
    /// Why an authenticated event intentionally produced no canonical message.
    InboundEventIgnoreReason as "inbound event ignore reason" {
        NotMessage => "not_message",
        UnsupportedEvent => "unsupported_event",
        UnsupportedSubtype => "unsupported_subtype",
        AutomatedSender => "automated_sender",
        EmptyContent => "empty_content",
        InactiveBinding => "inactive_binding",
        DeliveryConfirmation => "delivery_confirmation",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_processing_holds_a_lease() {
        let leased: Vec<_> = InboundEventStatus::ALL
            .iter()
            .copied()
            .filter(|status| status.holds_lease())
            .collect();
        assert_eq!(leased, vec![InboundEventStatus::Processing]);
    }

    #[test]
    fn terminal_and_retryable_classifications_are_explicit() {
        assert!(InboundEventErrorClass::ProviderFault.is_retryable());
        assert!(!InboundEventErrorClass::InvalidPayload.is_retryable());
        assert!(InboundEventStatus::DeadLetter.is_terminal());
        assert!(!InboundEventStatus::Retryable.is_terminal());
    }
}
