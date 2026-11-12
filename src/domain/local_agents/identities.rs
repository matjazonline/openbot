use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// These identities validate on JSON decoding too. The existing string_newtype! macro is
// intentionally permissive; using it here would bypass this external trust boundary.
macro_rules! uuid_identity {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Uuid);

        impl $name {
            pub fn new(value: Uuid) -> Result<Self, &'static str> {
                if value.is_nil() { return Err("identity must not be nil"); }
                Ok(Self(value))
            }
            pub fn as_uuid(self) -> Uuid { self.0 }
        }
        impl TryFrom<String> for $name {
            type Error = &'static str;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                let parsed = Uuid::parse_str(&value).map_err(|_| "invalid UUID identity")?;
                if value != parsed.hyphenated().to_string() {
                    return Err("identity must be a canonical lowercase UUID");
                }
                Self::new(parsed)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0.to_string() }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }
    )+};
}

uuid_identity!(
    MachineId,
    InstallationId,
    ProfileId,
    SessionId,
    OperationId,
    PairingId,
    NativeMcpCallId
);

macro_rules! external_identity {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl TryFrom<String> for $name {
            type Error = &'static str;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                if value.is_empty() || value.len() > 128 || !value.bytes().all(|byte|
                    byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte)) {
                    return Err("invalid local execution identity");
                }
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self { value.0 }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }
    )+};
}

external_identity!(
    CloudroomProjectId,
    CloudroomThreadId,
    CloudroomTurnId,
    CloudroomClientRequestId,
    CloudroomModelCallId
);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ModelId(String);

impl ModelId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ModelId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 128
            || value
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err("invalid model identity");
        }
        Ok(Self(value))
    }
}

impl From<ModelId> for String {
    fn from(value: ModelId) -> Self {
        value.0
    }
}

macro_rules! sequence_identity {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(u64);

        impl $name { pub fn get(self) -> u64 { self.0 } }
        impl TryFrom<u64> for $name {
            type Error = &'static str;
            fn try_from(value: u64) -> Result<Self, Self::Error> {
                if value == 0 || value > i64::MAX as u64 { return Err("invalid sequence"); }
                Ok(Self(value))
            }
        }
        impl From<$name> for u64 { fn from(value: $name) -> u64 { value.0 } }
    )+};
}

sequence_identity!(CredentialGeneration, ConnectionGeneration, EventSequence);
