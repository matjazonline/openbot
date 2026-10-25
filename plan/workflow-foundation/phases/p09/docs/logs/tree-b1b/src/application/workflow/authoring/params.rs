//! Schema-driven binding parameter fields (p09 B1b): which form field each top-level property
//! of a version's `parameter_schema` gets, the starting value of each from stored params, and
//! the params object a submitted form builds. The binding command still validates that object
//! against the schema; this only shapes the form.
use serde_json::{Map, Value};

/// Past this many properties the form takes the whole params object as JSON.
pub const MAX_PARAMETER_FIELDS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum ParameterInput {
    Text {
        max_length: Option<u64>,
    },
    Integer,
    Number,
    /// A select of `true`/`false`, unset allowed.
    Boolean,
    /// A string `enum`: a select of its values.
    Choice(Vec<String>),
    /// Anything else (object, array, untyped, a non-string enum): one JSON value.
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParameterField {
    pub name: String,
    pub required: bool,
    pub input: ParameterInput,
    pub description: Option<String>,
}

/// One field per property of an object schema, by name; `None` when the schema is not an
/// object with properties (or has more than [`MAX_PARAMETER_FIELDS`]), so the form takes the
/// whole object as JSON.
pub fn parameter_fields(schema: Option<&Value>) -> Option<Vec<ParameterField>> {
    let properties = schema?.get("properties")?.as_object()?;
    if properties.is_empty() || properties.len() > MAX_PARAMETER_FIELDS {
        return None;
    }
    let required: Vec<&str> = schema?
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(
        properties
            .iter()
            .map(|(name, property)| ParameterField {
                name: name.clone(),
                required: required.contains(&name.as_str()),
                input: input_of(property),
                description: property
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
            .collect(),
    )
}

fn input_of(property: &Value) -> ParameterInput {
    if let Some(values) = property.get("enum").and_then(Value::as_array) {
        let choices: Option<Vec<String>> = values
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect();
        return choices.map_or(ParameterInput::Json, ParameterInput::Choice);
    }
    match property.get("type").and_then(Value::as_str) {
        Some("string") => ParameterInput::Text {
            max_length: property.get("maxLength").and_then(Value::as_u64),
        },
        Some("integer") => ParameterInput::Integer,
        Some("number") => ParameterInput::Number,
        Some("boolean") => ParameterInput::Boolean,
        _ => ParameterInput::Json,
    }
}

/// A field's starting value from stored params: the string itself for text and choices, the
/// number or boolean as written, JSON for the rest; empty when the parameter is not set.
pub fn field_value(field: &ParameterField, params: &Value) -> String {
    match (params.get(&field.name), &field.input) {
        (None, _) => String::new(),
        (Some(Value::String(text)), ParameterInput::Text { .. } | ParameterInput::Choice(_)) => {
            text.clone()
        }
        (Some(value), ParameterInput::Json) => {
            serde_json::to_string_pretty(value).unwrap_or_default()
        }
        (Some(value), _) => value.to_string(),
    }
}

/// The params object a submitted form builds from `value(name)` per field; an empty value
/// leaves its parameter out. `Err` says which field does not parse as its type.
pub fn parameters_from_fields(
    fields: &[ParameterField],
    value: impl Fn(&str) -> Option<String>,
) -> Result<Value, String> {
    let mut params = Map::new();
    for field in fields {
        let Some(raw) = value(&field.name).filter(|raw| !raw.trim().is_empty()) else {
            continue;
        };
        let parsed = match &field.input {
            ParameterInput::Text { .. } | ParameterInput::Choice(_) => Value::String(raw),
            ParameterInput::Integer => raw
                .trim()
                .parse::<i64>()
                .map(Value::from)
                .map_err(|_| format!("Parameter {} must be a whole number.", field.name))?,
            ParameterInput::Number => raw
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number)
                .ok_or_else(|| format!("Parameter {} must be a number.", field.name))?,
            ParameterInput::Boolean => match raw.trim() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(format!("Parameter {} must be true or false.", field.name)),
            },
            ParameterInput::Json => serde_json::from_str(&raw)
                .map_err(|_| format!("Parameter {} must be valid JSON.", field.name))?,
        };
        params.insert(field.name.clone(), parsed);
    }
    Ok(Value::Object(params))
}

/// The whole params object typed as JSON (a schema the form has no fields for); empty is `{}`.
pub fn parameters_from_json(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    match serde_json::from_str(raw) {
        Ok(Value::Object(params)) => Ok(Value::Object(params)),
        Ok(_) => Err("Parameters must be a JSON object.".into()),
        Err(_) => Err("Parameters must be valid JSON.".into()),
    }
}
