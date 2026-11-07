//! Terminal response policy belongs to a committed execution, not its business output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseDisposition {
    Automatic,
    Suppressed,
}

impl ResponseDisposition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Suppressed => "suppressed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Invalid workflow response disposition")]
pub struct InvalidResponseDisposition;

impl std::str::FromStr for ResponseDisposition {
    type Err = InvalidResponseDisposition;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "automatic" => Ok(Self::Automatic),
            "suppressed" => Ok(Self::Suppressed),
            _ => Err(InvalidResponseDisposition),
        }
    }
}
