//! TypeSafe typed text/image contract. This module is pure: no credentials, transport or agent selection.
use super::*;
use crate::domain::workflow::{ContextLimits, validate_context_value};
use serde::{Deserialize, Serialize};

pub const MAX_QUESTIONS: usize = 32;
pub const MAX_OPTIONS: usize = 64;
pub const MAX_LEVELS: usize = 10;
pub const MAX_REQUEST_BYTES: usize = 65_536;
pub const MAX_RESPONSE_BYTES: usize = 65_536;
/// Absolute tolerance for provider rounding in a normalized probability distribution.
pub const PROBABILITY_SUM_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionId(String);
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChoiceOption(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    yes: String,
    #[serde(rename = "false")]
    no: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Question {
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: String,
        criteria: BTreeMap<ChoiceOption, String>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
    Box {
        instructions: String,
    },
}

/// Validated question definitions, also usable by the HTTP adapter without changing semantics.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Questions(BTreeMap<QuestionId, Question>);
impl Questions {
    pub fn parse(value: &Value) -> Result<Self, &'static str> {
        bounded(value, MAX_REQUEST_BYTES)?;
        let map = value.as_object().ok_or("Questions must be an object")?;
        if map.is_empty() || map.len() > MAX_QUESTIONS {
            return Err("Use between 1 and 32 questions");
        }
        let mut questions = BTreeMap::new();
        for (id, value) in map {
            validate_id(id)?;
            if value.get("criteria") == Some(&Value::Null) {
                return Err("Criteria must be omitted or have the question's declared shape");
            }
            let question =
                Question::deserialize(value).map_err(|_| "Invalid Jev question shape")?;
            question.validate()?;
            questions.insert(QuestionId(id.clone()), question);
        }
        Ok(Self(questions))
    }

    pub fn validate_assets(
        &self,
        model: &str,
        assets: &crate::application::workflow::jev::JevAssetRefs,
    ) -> Result<(), &'static str> {
        if !(assets.is_empty() || model.starts_with("jev-1.13.") || model.starts_with("jev-1.14."))
        {
            return Err("Images require Jev 1.13 or 1.14");
        }
        for question in self.0.values() {
            if let Question::Box { instructions } = question {
                if !model.starts_with("jev-1.14.") {
                    return Err("Box requires Jev 1.14");
                }
                if !assets
                    .keys()
                    .any(|name| instructions.contains(&format!("`assets.{}`", name.as_str())))
                {
                    return Err("Box instructions must identify a declared `assets.NAME` image");
                }
            }
        }
        Ok(())
    }

    pub fn output_schema(&self, model: &str) -> Value {
        let answers: serde_json::Map<_, _> = self
            .0
            .iter()
            .map(|(id, question)| (id.0.clone(), question.output_schema()))
            .collect();
        object(
            json!({
                "model":{"type":"string","const":model},
                "answers":exact_object(answers),
                "usage":object(json!({
                    "input_tokens":{"type":"integer","minimum":0},
                    "output_tokens":{"type":"integer","minimum":0}
                }), &["input_tokens", "output_tokens"])
            }),
            &["model", "answers", "usage"],
        )
    }
}

fn validate_id(id: &str) -> Result<(), &'static str> {
    if id.trim().is_empty() || id.len() > MAX_IDENTIFIER || id.chars().any(char::is_control) {
        return Err(
            "Question and option IDs must be nonblank, at most 128 UTF-8 bytes, without control characters",
        );
    }
    Ok(())
}
fn validate_text(text: &str) -> Result<(), &'static str> {
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        return Err("Instructions and descriptions must be nonblank and at most 16384 UTF-8 bytes");
    }
    Ok(())
}
impl Question {
    fn validate(&self) -> Result<(), &'static str> {
        let instructions = match self {
            Self::Box { instructions } => instructions,
            Self::Noul {
                instructions,
                criteria,
            } => {
                if let Some(criteria) = criteria {
                    validate_text(&criteria.yes)?;
                    validate_text(&criteria.no)?;
                }
                instructions
            }
            Self::Choice {
                instructions,
                criteria,
            } => {
                if criteria.is_empty() || criteria.len() > MAX_OPTIONS {
                    return Err("Choice requires between 1 and 64 options");
                }
                for (option, description) in criteria {
                    validate_id(&option.0)?;
                    validate_text(description)?;
                }
                instructions
            }
            Self::Score {
                instructions,
                criteria,
            } => {
                if !(2..=MAX_LEVELS).contains(&criteria.len()) {
                    return Err("Score requires between 2 and 10 ordered levels");
                }
                for description in criteria {
                    validate_text(description)?;
                }
                instructions
            }
        };
        validate_text(instructions)
    }
    fn output_schema(&self) -> Value {
        match self {
            Self::Box { .. } => object(
                json!({"type":{"type":"string","const":"box"},"box":object(json!({"left":probability(),"right":probability(),"top":probability(),"bottom":probability()}), &["left","right","top","bottom"]),"confidence":probability()}),
                &["type", "box", "confidence"],
            ),
            Self::Noul { .. } => object(
                json!({"type":{"type":"string","const":"noul"},"noul":probability()}),
                &["type", "noul"],
            ),
            Self::Choice { criteria, .. } => {
                let probabilities = criteria
                    .keys()
                    .map(|key| (key.0.clone(), probability()))
                    .collect();
                object(
                    json!({"type":{"type":"string","const":"choice"},
                    "choice":{"type":"string","enum":criteria.keys().collect::<Vec<_>>()},
                    "probabilities":exact_object(probabilities),"confidence":probability()}),
                    &["type", "choice", "probabilities", "confidence"],
                )
            }
            Self::Score { criteria, .. } => {
                let probabilities = (0..criteria.len())
                    .map(|i| (i.to_string(), probability()))
                    .collect();
                let legend = criteria
                    .iter()
                    .enumerate()
                    .map(|(i, text)| (i.to_string(), json!({"type":"string","const":text})))
                    .collect();
                object(
                    json!({"type":{"type":"string","const":"score"},
                    "score":{"type":"number","minimum":0,"maximum":criteria.len()-1},
                    "probabilities":exact_object(probabilities),"legend":exact_object(legend),"confidence":probability()}),
                    &["type", "score", "probabilities", "legend", "confidence"],
                )
            }
        }
    }
}
fn probability() -> Value {
    json!({"type":"number","minimum":0,"maximum":1})
}
fn exact_object(properties: serde_json::Map<String, Value>) -> Value {
    let required: Vec<_> = properties.keys().map(String::as_str).collect();
    object(json!(properties), &required)
}

pub fn validate_model(model: &str) -> Result<(), &'static str> {
    let parts: Vec<_> = model
        .strip_prefix("jev-")
        .unwrap_or("")
        .split('.')
        .collect();
    if model.len() > MAX_IDENTIFIER
        || parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(
            "Use an exact versioned Jev model ID, such as jev-1.14.0; aliases are unsupported",
        );
    }
    Ok(())
}

fn bounded(value: &Value, bytes: usize) -> Result<(), &'static str> {
    validate_context_value(
        value,
        ContextLimits {
            output_bytes: bytes,
            ..Default::default()
        },
    )
    .map_err(|_| "Jev payload exceeds byte, depth or work limits")
}

/// Counts borrowed request fields without creating a JSON buffer. Literal preflight may omit
/// state; resolved input preparation supplies it and enforces the complete envelope.
pub(crate) fn validate_inputs(inputs: &Value) -> Result<(), &'static str> {
    if let Some(state) = inputs.get("state") {
        validate_state(state)?;
    }
    let request = RequestFields {
        model: inputs.get("model"),
        state: inputs.get("state"),
        questions: inputs.get("questions"),
        assets: inputs.get("assets"),
    };
    request.check_size()?;
    let assets = crate::application::workflow::jev::parse_assets(inputs.get("assets"))?;
    if let (Some(model), Some(questions)) = (
        inputs.get("model").and_then(Value::as_str),
        inputs.get("questions"),
    ) {
        Questions::parse(questions)?.validate_assets(model, &assets)?;
    }
    Ok(())
}
pub fn validate_state(state: &Value) -> Result<(), &'static str> {
    if !matches!(state, Value::String(_) | Value::Object(_) | Value::Array(_)) {
        return Err("Jev state must be a string, object or array");
    }
    bounded(state, MAX_REQUEST_BYTES)
}
#[derive(Serialize)]
struct RequestFields<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    questions: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assets: Option<&'a Value>,
}
impl RequestFields<'_> {
    fn check_size(&self) -> Result<(), &'static str> {
        for value in [self.model, self.state, self.questions, self.assets]
            .into_iter()
            .flatten()
        {
            bounded(value, MAX_REQUEST_BYTES)?;
        }
        serde_json::to_writer(ByteBudget(MAX_REQUEST_BYTES), self)
            .map_err(|_| "Jev request exceeds 65536 serialized bytes")
    }
}
struct ByteBudget(usize);
impl std::io::Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("Jev request limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Run after the specialized schema: exact fields and [0,1] values are already checked.
pub(crate) fn validate_output(value: &Value) -> Result<(), &'static str> {
    bounded(value, MAX_RESPONSE_BYTES)?;
    for answer in value["answers"]
        .as_object()
        .ok_or("Missing Jev answers")?
        .values()
    {
        if answer["type"] == "box" {
            let coordinates = &answer["box"];
            let edge = |name| {
                coordinates[name]
                    .as_f64()
                    .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
                    .ok_or("Invalid Box coordinate")
            };
            if edge("left")? >= edge("right")? || edge("top")? >= edge("bottom")? {
                return Err("Box must have positive width and height");
            }
        }
        if let Some(probabilities) = answer.get("probabilities").and_then(Value::as_object) {
            let sum = probabilities.values().try_fold(0.0, |sum, value| {
                value
                    .as_f64()
                    .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
                    .map(|p| sum + p)
                    .ok_or("Invalid Jev probability")
            })?;
            if (sum - 1.0).abs() > PROBABILITY_SUM_TOLERANCE {
                return Err("Jev probabilities must sum to 1 within absolute tolerance 0.000001");
            }
        }
    }
    Ok(())
}

impl Registration<'_> {
    pub(super) fn jev(&mut self) -> Result<Contract, Diagnostic> {
        let connection = self.resource("typesafe")?;
        let model = self.literal("model")?;
        let model_text = model.as_str().ok_or_else(|| {
            self.error("registry.jev", "Model must be a versioned Jev ID", "model")
        })?;
        validate_model(model_text)
            .map_err(|message| self.error("registry.jev", message, "model"))?;
        let questions = self.literal("questions")?;
        let typed = Questions::parse(&questions)
            .map_err(|message| self.error("registry.jev", message, "questions"))?;
        let assets = if self.parsed.definition.steps[self.id]
            .inputs
            .contains_key("assets")
        {
            Some(self.literal("assets")?)
        } else {
            None
        };
        let refs = crate::application::workflow::jev::parse_assets(assets.as_ref())
            .map_err(|message| self.error("registry.jev", message, "assets"))?;
        typed
            .validate_assets(model_text, &refs)
            .map_err(|message| self.error("registry.jev", message, "questions"))?;
        self.check(InputCheck::Jev)?;
        let mut inputs = json!({"connection":{"const":connection},"model":{"const":model},"state":{"type":["string","object","array"]},"questions":{"const":questions}});
        let mut required = vec!["connection", "model", "state", "questions"];
        if let Some(assets) = assets {
            inputs["assets"] = json!({"const":assets});
            required.push("assets");
        }
        Ok(contract(inputs, &required, typed.output_schema(model_text)))
    }
}

/// Borrowed preflight runs before Registration::literal creates descriptor-owned copies.
pub(super) fn preflight(
    step: &crate::domain::workflow::StepDefinition,
    budget: &mut compiler::FactBudget,
    span: SourceSpan,
) -> Result<(), Diagnostic> {
    let literal = |name| match step.inputs.get(name) {
        Some(Binding::Literal(value)) => Some(value),
        _ => None,
    };
    RequestFields {
        model: literal("model"),
        state: literal("state"),
        questions: literal("questions"),
        assets: literal("assets"),
    }
    .check_size()
    .map_err(|message| Diagnostic::at("registry.jev", message, "/steps", span))?;
    if let Some(assets) = literal("assets") {
        crate::application::workflow::jev::parse_assets(Some(assets))
            .map_err(|message| Diagnostic::at("registry.jev", message, "/steps", span))?;
        for _ in 0..2 {
            budget.schema(assets)?;
        }
    }
    if let Some(questions) = literal("questions") {
        // Input const, retained definition, and answer legend/option descriptions. Charge
        // structural expansion conservatively before allocating a specialized schema.
        for _ in 0..3 {
            budget.schema(questions)?;
        }
        if let Some(map) = questions.as_object() {
            budget.punctuation(map.len().saturating_mul(1024))?;
            for question in map.values() {
                let count = match question.get("criteria") {
                    Some(Value::Object(map)) => map.len(),
                    Some(Value::Array(items)) => items.len(),
                    _ => 0,
                };
                budget.punctuation(count.saturating_mul(512))?;
            }
        }
    }
    Ok(())
}
