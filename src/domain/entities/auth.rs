use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A verifier's authentication result. Unknown spellings and legacy `null` values fail closed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AuthVerdict {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    TempError,
    PermError,
    Unavailable,
    #[default]
    Unknown,
}

impl AuthVerdict {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "pass" => Self::Pass,
            "fail" => Self::Fail,
            "softfail" | "soft_fail" => Self::SoftFail,
            "neutral" => Self::Neutral,
            "temperror" | "temp_error" => Self::TempError,
            "permerror" | "perm_error" => Self::PermError,
            "none" | "unavailable" => Self::Unavailable,
            _ => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::SoftFail => "softfail",
            Self::Neutral => "neutral",
            Self::TempError => "temperror",
            Self::PermError => "permerror",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

impl Serialize for AuthVerdict {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AuthVerdict {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<String>::deserialize(deserializer)?
            .as_deref()
            .map(Self::parse)
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_legacy_strings_null_and_unknown_values() {
        for (json, expected) in [
            (r#""pass""#, AuthVerdict::Pass),
            (r#""FAIL""#, AuthVerdict::Fail),
            (r#""softfail""#, AuthVerdict::SoftFail),
            (r#""neutral""#, AuthVerdict::Neutral),
            (r#""temperror""#, AuthVerdict::TempError),
            (r#""permerror""#, AuthVerdict::PermError),
            (r#""none""#, AuthVerdict::Unavailable),
            (r#""future-value""#, AuthVerdict::Unknown),
            ("null", AuthVerdict::Unknown),
        ] {
            assert_eq!(serde_json::from_str::<AuthVerdict>(json).unwrap(), expected);
        }
    }
}
