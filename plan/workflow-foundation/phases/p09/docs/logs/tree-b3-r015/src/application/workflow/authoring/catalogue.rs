use crate::application::workflow::compiler::{ControlKind, RouteContract, SourceDecoder};
use crate::application::workflow::registry::{self, TYPES};
use crate::domain::workflow::{StepId, VersionId};
use serde_json::Value;

/// One step type as the editor's reference shows it: derived from an actual registration and
/// an example that compiles, so the reference cannot describe a contract the compiler refuses.
#[derive(Debug, Clone)]
pub struct CatalogueEntry {
    pub type_name: &'static str,
    pub notes: &'static str,
    pub input_schema: Value,
    pub output_schema: Value,
    /// `success`, or the choices: the example's for a `decision.*` type, whose author
    /// declares them ([`Self::declared_choices`]); the type's own otherwise.
    pub routes: Vec<String>,
    pub declared_choices: bool,
    /// `child workflow`, `repeat`, or empty.
    pub control: &'static str,
    /// A complete source using the type (JSON, which YAML accepts).
    pub example: String,
}

/// Every step type this deployment runs, in catalogue order; `refused` (the deployment's
/// unhandled kinds) are left out, as publication refuses them.
pub fn catalogue_reference(decoder: &dyn SourceDecoder, refused: &[&str]) -> Vec<CatalogueEntry> {
    TYPES
        .iter()
        .copied()
        .filter(|name| !refused.contains(name))
        .filter_map(|name| {
            let example = registry::example(name)?;
            let decoded = decoder.decode(&example.source).ok()?;
            let help = registry::authoring_help(
                decoded,
                VersionId::new(uuid::Uuid::nil()),
                &StepId::parse("start").expect("example step"),
                &example.facts,
                &example.dependencies,
            )
            .ok()?;
            Some(CatalogueEntry {
                type_name: name,
                notes: help.notes,
                input_schema: help.input_schema,
                output_schema: help.output_schema,
                routes: match help.routes {
                    RouteContract::Success => vec!["success".to_owned()],
                    RouteContract::Choices(choices) => choices
                        .iter()
                        .map(|choice| choice.as_str().to_owned())
                        .collect(),
                },
                declared_choices: name.starts_with("decision."),
                control: match help.control {
                    ControlKind::None => "",
                    ControlKind::ChildCall => "child workflow",
                    ControlKind::Repeat => "repeat",
                },
                example: serde_json::from_str::<Value>(&help.example)
                    .and_then(|value| serde_json::to_string_pretty(&value))
                    .unwrap_or(help.example),
            })
        })
        .collect()
}
