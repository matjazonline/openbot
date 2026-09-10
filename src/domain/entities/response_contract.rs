//! Versioned, harness-neutral final-answer contract. Schema semantics belong to a validator port.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const MAX_SCHEMA_BYTES: usize = 65_536;
pub const MAX_CONTRACT_BYTES: usize = MAX_SCHEMA_BYTES + 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Envelope", into = "Envelope")]
pub struct ResponseContract {
    schema: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u16,
    format: String,
    schema: Value,
}

impl TryFrom<Envelope> for ResponseContract {
    type Error = &'static str;
    fn try_from(value: Envelope) -> Result<Self, Self::Error> {
        if value.version != 1
            || value.format != "json_schema"
            || value.schema.to_string().len() > MAX_SCHEMA_BYTES
        {
            return Err("Unsupported or oversized response_contract");
        }
        Ok(Self {
            schema: value.schema,
        })
    }
}
impl From<ResponseContract> for Envelope {
    fn from(value: ResponseContract) -> Self {
        Self {
            version: 1,
            format: "json_schema".into(),
            schema: value.schema,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractFingerprint(String);

impl ResponseContract {
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        if input.len() > MAX_CONTRACT_BYTES {
            return Err("Oversized response_contract");
        }
        let envelope: Envelope =
            serde_json::from_str(input).map_err(|_| "Invalid response_contract")?;
        envelope.try_into()
    }
    pub fn schema(&self) -> &Value {
        &self.schema
    }
    pub fn fingerprint(&self) -> ContractFingerprint {
        // serde_json's default map representation sorts keys, including nested objects.
        ContractFingerprint(format!(
            "{:x}",
            Sha256::digest(format!("response-contract:v1:json_schema:{}", self.schema).as_bytes())
        ))
    }
    pub fn require_harness(
        &self,
        harness: super::harness::HarnessKind,
    ) -> Result<(), &'static str> {
        if harness != super::harness::HarnessKind::Rig {
            return Err("response_contract requires the rig harness");
        }
        Ok(())
    }
}

/// Omission preserves a stored contract; explicit null clears it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractUpdate(
    #[serde(deserialize_with = "present")] pub Option<Option<ResponseContract>>,
);
fn present<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<ResponseContract>>, D::Error> {
    let raw = Option::<Box<serde_json::value::RawValue>>::deserialize(d)?;
    raw.map(|raw| ResponseContract::parse(raw.get()).map_err(serde::de::Error::custom))
        .transpose()
        .map(Some)
}
