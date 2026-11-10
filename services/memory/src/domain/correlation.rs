//! A client-supplied correlation id (p06 gate A2-2): opaque, non-secret, 1–128 bytes of
//! `[A-Za-z0-9._:-]`. It is never authority: not part of idempotency identity, never compared for
//! replay and never a request id (the service mints its own `x-request-id`).
use super::error::valid_external_id;

/// Fixed-size so the principals that carry it stay `Copy`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CorrelationId {
    bytes: [u8; 128],
    len: u8,
}

impl CorrelationId {
    /// `None` for anything but 1–128 bytes of `[A-Za-z0-9._:-]`.
    pub fn parse(value: &str) -> Option<Self> {
        if !valid_external_id(value) || value.len() > 128 {
            return None;
        }
        let mut bytes = [0u8; 128];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Some(Self {
            bytes,
            len: value.len() as u8,
        })
    }

    pub fn as_str(&self) -> &str {
        // Only validated ASCII is ever stored.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
}

impl std::fmt::Debug for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_operations_correlation_ids_accept_only_the_id_alphabet() {
        assert_eq!(
            CorrelationId::parse("ob.g.abc:1-2_3").unwrap().as_str(),
            "ob.g.abc:1-2_3"
        );
        for bad in ["", "has space", "ünï", "a/b", &"x".repeat(129)] {
            assert!(CorrelationId::parse(bad).is_none(), "{bad}");
        }
        assert!(CorrelationId::parse(&"x".repeat(128)).is_some());
    }
}
