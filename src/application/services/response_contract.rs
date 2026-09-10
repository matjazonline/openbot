//! Final-answer validation and contract identity, independent of schema libraries and runtimes.
use crate::{
    app_error::{AppError, AppResult},
    entities::response_contract::{ContractFingerprint, ResponseContract},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_RESPONSE_BYTES: usize = 65_536;
pub const MAX_REPAIR_CALLS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidResponse {
    MalformedJson,
    SchemaMismatch,
    OutputLimit,
    ToolCall,
}
impl InvalidResponse {
    pub fn code(self) -> &'static str {
        match self {
            Self::MalformedJson => "malformed_json",
            Self::SchemaMismatch => "schema_mismatch",
            Self::OutputLimit => "output_limit",
            Self::ToolCall => "tool_call_not_allowed",
        }
    }
}

pub trait ResponseContractValidator: Send + Sync {
    fn validate_contract(&self, contract: &ResponseContract) -> AppResult<()>;
    fn validate_value(&self, contract: &ResponseContract, value: &Value) -> AppResult<bool>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuredResponse {
    contract: ResponseContract,
    fingerprint: ContractFingerprint,
    body: String,
}
impl StructuredResponse {
    pub fn validate(
        contract: &ResponseContract,
        candidate: &str,
        secret: Option<&str>,
        validator: &dyn ResponseContractValidator,
    ) -> AppResult<Result<Self, InvalidResponse>> {
        if candidate.len() > MAX_RESPONSE_BYTES {
            return Ok(Err(InvalidResponse::OutputLimit));
        }
        let sanitized = super::harness::sanitize_text(candidate, secret);
        let value: Value = match serde_json::from_str(&sanitized) {
            Ok(value) => value,
            Err(_) => return Ok(Err(InvalidResponse::MalformedJson)),
        };
        let body = super::harness::sanitize_text(&value.to_string(), secret);
        if body.len() > MAX_RESPONSE_BYTES {
            return Ok(Err(InvalidResponse::OutputLimit));
        }
        let persisted: Value = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(_) => return Ok(Err(InvalidResponse::MalformedJson)),
        };
        if !validator.validate_value(contract, &persisted)? {
            return Ok(Err(InvalidResponse::SchemaMismatch));
        }
        Ok(Ok(Self {
            contract: contract.clone(),
            fingerprint: contract.fingerprint(),
            body,
        }))
    }
    pub fn body(&self) -> &str {
        &self.body
    }
    pub fn contract(&self) -> &ResponseContract {
        &self.contract
    }
    pub fn verify(&self, body: &str, validator: &dyn ResponseContractValidator) -> AppResult<()> {
        let valid =
            Self::validate(&self.contract, body, None, validator)?.map_err(|_| invalid_output())?;
        if &valid != self || body != self.body {
            return Err(invalid_output());
        }
        Ok(())
    }
}
pub fn invalid_output() -> AppError {
    AppError::Execution(crate::app_error::ExecutionFailure::InvalidOutput)
}
