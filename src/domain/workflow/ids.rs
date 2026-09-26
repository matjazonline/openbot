use std::fmt;
use thiserror::Error;
use uuid::Uuid;

pub const MAX_NAME_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("name must be 1..={MAX_NAME_BYTES} ASCII bytes and start with a letter or underscore")]
    Invalid,
    #[error("$end is reserved as a transition target")]
    Reserved,
}

fn valid_part(value: &str) -> bool {
    value.len() <= MAX_NAME_BYTES
        && value
            .as_bytes()
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && value.as_bytes()[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

macro_rules! name_type {
    ($name:ident, $dotted:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl AsRef<str>) -> Result<Self, NameError> {
                let value = value.as_ref();
                if value == "$end" {
                    return Err(NameError::Reserved);
                }
                let valid = if $dotted {
                    value.len() <= MAX_NAME_BYTES && value.split('.').all(valid_part)
                } else {
                    valid_part(value)
                };
                if !valid {
                    return Err(NameError::Invalid);
                }
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

name_type!(StepId, false);
name_type!(ChoiceName, false);
name_type!(ResourceName, false);
name_type!(TypeName, true);
name_type!(FailureCode, true);

macro_rules! uuid_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new(value: Uuid) -> Self {
                Self(value)
            }
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

uuid_type!(WorkflowId);
uuid_type!(VersionId);
uuid_type!(CompanyId);
uuid_type!(RunId);
uuid_type!(ExecutionId);
uuid_type!(TriggerId);
uuid_type!(ScheduleId);
uuid_type!(ScheduleOccurrenceId);
uuid_type!(ActionInvocationId);
uuid_type!(WaitId);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_names() {
        for bad in ["", "0bad", "bad.dot", "$end", "é", "a".repeat(129).as_str()] {
            assert!(StepId::parse(bad).is_err(), "{bad}");
        }
        assert!(StepId::parse("valid_2-name").is_ok());
        assert!(TypeName::parse("agent.run").is_ok());
        assert!(TypeName::parse("agent..run").is_err());
    }
}
