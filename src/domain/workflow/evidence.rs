//! Bounded evidence identifiers. These values carry identity, never provider authority.
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid workflow evidence identifier")]
pub struct EvidenceIdentityError;

macro_rules! evidence_text {
    ($name:ident, $valid:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl AsRef<str>) -> Result<Self, EvidenceIdentityError> {
                let value = value.as_ref();
                if !($valid)(value) {
                    return Err(EvidenceIdentityError);
                }
                Ok(Self(value.to_owned()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
                Self::parse(String::deserialize(decoder)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

fn reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

evidence_text!(EvidenceVerifierId, reference);
evidence_text!(EvidenceVerifierVersion, reference);
evidence_text!(EvidenceProviderId, reference);
evidence_text!(EvidenceRecordReference, reference);
evidence_text!(ActionCoverageDigest, digest);
evidence_text!(ReconciliationRequestDigest, digest);
