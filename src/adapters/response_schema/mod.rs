//! Pinned Draft 2020-12 implementation of the application validator port.
use crate::{
    app_error::{AppError, AppResult},
    entities::response_contract::ResponseContract,
    services::response_contract::ResponseContractValidator,
};
use serde_json::Value;

pub struct JsonResponseValidator;
impl JsonResponseValidator {
    fn compile(contract: &ResponseContract) -> AppResult<jsonschema::Validator> {
        let schema = contract.schema();
        crate::services::tool_schema::check_resources(schema).map_err(|_| invalid())?;
        check_dialect(schema)?;
        if !jsonschema::draft202012::meta::is_valid(schema) {
            return Err(invalid());
        }
        jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .with_retriever(crate::services::tool_schema::NoExternalReferences)
            .with_pattern_options(jsonschema::PatternOptions::regex().size_limit(65_536))
            .build(schema)
            .map_err(|_| invalid())
    }
}
fn check_dialect(value: &Value) -> AppResult<()> {
    match value {
        Value::Object(map) => {
            if map.get("$schema").is_some_and(|value| {
                value.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
            }) {
                return Err(invalid());
            }
            for child in map.values() {
                check_dialect(child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                check_dialect(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn invalid() -> AppError {
    AppError::BadRequest("response_contract: invalid, unsupported or excessive schema".into())
}
impl ResponseContractValidator for JsonResponseValidator {
    fn validate_contract(&self, contract: &ResponseContract) -> AppResult<()> {
        Self::compile(contract).map(|_| ())
    }
    fn validate_value(&self, contract: &ResponseContract, value: &Value) -> AppResult<bool> {
        Ok(Self::compile(contract)?.is_valid(value))
    }
}

/// Recheck the exact payload frozen by the transport adapter before publication eligibility.
pub(crate) fn validate_publication(
    message: &crate::use_cases::thread::MessageWrite,
    delivery: &crate::transport::NewDelivery,
) -> AppResult<()> {
    if let Some(response) = &message.structured {
        response.verify(&message.clean_text_body, &JsonResponseValidator)?;
        validate_delivery(response, delivery)?;
    }
    Ok(())
}

pub(crate) fn validate_delivery(
    response: &crate::services::response_contract::StructuredResponse,
    delivery: &crate::transport::NewDelivery,
) -> AppResult<()> {
    if delivery.parts.len() != 1 {
        return Err(crate::services::response_contract::invalid_output());
    }
    for part in delivery.parts.iter() {
        let email: crate::adapters::protocols::email::OutboundEmailV1 = part
            .payload
            .decode(
                crate::entities::transport::TransportKind::Email,
                crate::adapters::protocols::email::OUTBOUND_EMAIL_VERSION,
            )
            .map_err(|_| crate::services::response_contract::invalid_output())?;
        response.verify(&email.body_text, &JsonResponseValidator)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
