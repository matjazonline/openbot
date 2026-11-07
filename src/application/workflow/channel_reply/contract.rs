use super::super::compiler::{Schema, SourceSpan};
use crate::app_error::{AppError, AppResult};
use crate::application::transport::CanonicalContent;
use crate::domain::workflow::{
    Binding, Context, ContextLimits, ContextReference, RunId, RunMetadata, resolve,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const MAX_CONTRACT_BYTES: usize = 16 * 1024;
const CONTRACT_SPAN: SourceSpan = SourceSpan {
    start: 0,
    end: 0,
    line: 1,
    column: 1,
};

/// Uses the existing binding reference vocabulary. Within this contract `/input`
/// is the validated final business result, never the incoming message or run metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReplyReference(String);

impl TryFrom<String> for ReplyReference {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let parsed = ContextReference::parse(&value).map_err(|_| "Invalid result reference")?;
        if parsed.root_name() != "input" {
            return Err("A response reference must select the final result through /input");
        }
        Ok(Self(value))
    }
}
impl From<ReplyReference> for String {
    fn from(value: ReplyReference) -> Self {
        value.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ReplyText {
    Literal { literal: String },
    Reference { r#ref: ReplyReference },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelReplyContract {
    pub format_version: u8,
    pub subject: ReplyText,
    pub body: ReplyText,
}

impl ChannelReplyContract {
    pub fn validate(&self, output_schema: &Value) -> AppResult<()> {
        if self.format_version != 1
            || serde_json::to_vec(self).map_err(|_| invalid())?.len() > MAX_CONTRACT_BYTES
        {
            return Err(invalid());
        }
        Schema::compile(output_schema.clone(), "/output_schema", CONTRACT_SPAN)
            .map_err(|_| invalid())?;
        self.subject.validate(output_schema)?;
        self.body.validate(output_schema)?;
        if let ReplyText::Literal { literal } = &self.subject {
            CanonicalContent::parse(literal, "validation").map_err(|_| invalid())?;
        }
        if let ReplyText::Literal { literal } = &self.body {
            validate_content("", literal)?;
        }
        Ok(())
    }

    pub fn project(&self, output_schema: &Value, result: &Value) -> AppResult<CanonicalContent> {
        self.validate(output_schema)?;
        Schema::compile(output_schema.clone(), "/output_schema", CONTRACT_SPAN)
            .map_err(|_| invalid())?
            .validate(result, "/result", CONTRACT_SPAN)
            .map_err(|_| invalid())?;
        let subject = self.subject.evaluate(result)?;
        let body = self.body.evaluate(result)?;
        validate_content(&subject, &body)
    }
}

impl ReplyText {
    fn validate(&self, schema: &Value) -> AppResult<()> {
        let Self::Reference { r#ref } = self else {
            return Ok(());
        };
        let reference = ContextReference::parse(&r#ref.0).map_err(|_| invalid())?;
        let mut selected = schema;
        for segment in reference.tail() {
            if selected.get("type").and_then(Value::as_str) != Some("object")
                || !selected
                    .get("required")
                    .and_then(Value::as_array)
                    .is_some_and(|required| {
                        required.iter().any(|value| value.as_str() == Some(segment))
                    })
                || unsupported_schema(selected)
            {
                return Err(invalid());
            }
            selected = selected
                .get("properties")
                .and_then(|fields| fields.get(segment))
                .ok_or_else(invalid)?;
        }
        if selected.get("type").and_then(Value::as_str) != Some("string")
            || unsupported_schema(selected)
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn evaluate(&self, result: &Value) -> AppResult<String> {
        let binding = match self {
            Self::Literal { literal } => Binding::Literal(Value::String(literal.clone())),
            Self::Reference { r#ref } => {
                Binding::Reference(ContextReference::parse(&r#ref.0).map_err(|_| invalid())?)
            }
        };
        let empty = Value::Null;
        let outputs = BTreeMap::new();
        let context = Context {
            input: result,
            params: &empty,
            step_outputs: &outputs,
            run: RunMetadata {
                run_id: RunId::new(uuid::Uuid::nil()),
                parent_run_id: None,
                deadline: None,
            },
        };
        resolve(&binding, &context, ContextLimits::default())
            .map_err(|_| invalid())?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(invalid)
    }
}

fn unsupported_schema(schema: &Value) -> bool {
    [
        "$ref",
        "$dynamicRef",
        "allOf",
        "anyOf",
        "oneOf",
        "not",
        "if",
        "then",
        "else",
    ]
    .iter()
    .any(|key| schema.get(key).is_some())
}

fn validate_content(subject: &str, body: &str) -> AppResult<CanonicalContent> {
    let content = CanonicalContent::parse(subject, body).map_err(|_| invalid())?;
    if content.body_text().trim().is_empty() {
        return Err(invalid());
    }
    Ok(content)
}

fn invalid() -> AppError {
    AppError::BadRequest("Invalid channel response contract or result; select required string output fields or bounded text literals".into())
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
