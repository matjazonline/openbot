use super::{IntegrationError, IntegrationResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! text_id {
    ($name:ident, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl Into<String>) -> IntegrationResult<Self> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(IntegrationError::Invalid);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = IntegrationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
text_id!(ProviderId, 128);
text_id!(AppId, 128);
text_id!(AppPolicyDigest, 64);
text_id!(AuthConfigId, 128);
text_id!(CredentialSchema, 128);
text_id!(ClientSecretReference, 128);
text_id!(AccountKey, 128);
text_id!(OwnerNamespace, 256);
text_id!(OwnerSubject, 256);
text_id!(ExternalSubject, 256);
text_id!(ExternalIdentityKind, 128);
text_id!(AuthorizationKey, 256);
text_id!(Capability, 128);
text_id!(Scope, 256);
text_id!(HostBinding, 256);
text_id!(HostAuthorityReference, 256);
text_id!(OperationId, 128);
text_id!(CorrelationId, 128);
text_id!(CancellationId, 128);
text_id!(ConnectSessionReference, 128);
text_id!(BrowserBinding, 64);
text_id!(ConnectNonce, 64);

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: Uuid) -> Self {
                Self(value)
            }
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}
uuid_id!(AccountId);
uuid_id!(UseGrantId);
uuid_id!(ConnectAttemptId);
uuid_id!(DispatchId);

macro_rules! version {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(try_from = "i64", into = "i64")]
        pub struct $name(i64);
        impl $name {
            pub const INITIAL: Self = Self(1);
            pub fn new(value: i64) -> IntegrationResult<Self> {
                if value <= 0 {
                    return Err(IntegrationError::Invalid);
                }
                Ok(Self(value))
            }
            pub fn get(self) -> i64 {
                self.0
            }
            pub fn next(self) -> IntegrationResult<Self> {
                self.0
                    .checked_add(1)
                    .ok_or(IntegrationError::Limit)
                    .and_then(Self::new)
            }
        }
        impl TryFrom<i64> for $name {
            type Error = IntegrationError;
            fn try_from(value: i64) -> IntegrationResult<Self> {
                Self::new(value)
            }
        }
        impl From<$name> for i64 {
            fn from(value: $name) -> i64 {
                value.0
            }
        }
    };
}
version!(AccountRevision);
version!(AuthorizationRevision);
version!(GrantRevision);
version!(CredentialGeneration);
version!(ConnectionRevision);
version!(CatalogRevision);
