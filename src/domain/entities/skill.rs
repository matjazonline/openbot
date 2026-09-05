//! A skill: an ordered recipe an agent may be given, in a form no harness owns.
//!
//! The instructions are stored as one JSONB array of typed values rather than as a `steps` table.
//! A skill is a handful of ordered items edited as a single document, and a second table would buy
//! referential integrity nobody needs at the cost of a join on every agent run.
//!
//! Each harness compiles the same instructions into its own dialect: `ai-agents` gets a YAML
//! `skills:` entry with `steps:`, and a markdown-driven harness would flatten them into one
//! document. Nothing in this module knows which.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{
    creation::CreationProvenance,
    tool_catalogue::CatalogueTool,
    value_objects::{SkillSlug, ToolId},
};

pub const MAX_SKILL_INSTRUCTIONS: usize = 32;
pub const MAX_SKILL_INSTRUCTION_CHARS: usize = 8_000;
pub const MAX_SKILL_TRIGGER_CHARS: usize = 500;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 500;
pub const MAX_SKILL_NAME_CHARS: usize = 120;
/// A template variable name, not free text -- the bound is generous for one identifier.
pub const MAX_SKILL_OUTPUT_AS_CHARS: usize = 64;

/// One skill as it is stored and edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: Uuid,
    /// `None` identifies an operator-managed definition in the global skill library.
    #[serde(default)]
    pub company_id: Option<Uuid>,
    pub slug: SkillSlug,
    pub name: String,
    /// Required by `ai-agents`, and the subtitle a picker shows.
    pub description: String,
    /// Required by `ai-agents`: when the router should reach for this skill.
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    pub created_by: CreationProvenance,
    pub created_at: DateTime<Utc>,
}

/// One step of a skill.
///
/// Externally tagged so a third variant is additive: an untagged enum would silently reinterpret
/// an old stored row the day a new variant's shape overlapped an existing one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillInstruction {
    Prompt {
        text: String,
    },
    Tool {
        tool: ToolId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_as: Option<String>,
    },
}

impl SkillInstruction {
    /// The tool this step invokes, or `None` for a prompt step.
    pub fn tool_id(&self) -> Option<&ToolId> {
        match self {
            Self::Prompt { .. } => None,
            Self::Tool { tool, .. } => Some(tool),
        }
    }

    pub fn is_prompt(&self) -> bool {
        matches!(self, Self::Prompt { .. })
    }
}

impl Skill {
    pub fn is_library(&self) -> bool {
        self.company_id.is_none()
    }

    /// Tool ids this skill's steps invoke, deduplicated and in first-use order.
    ///
    /// The harness compiler unions these into the agent's `tools:` grant. Without that the runtime
    /// denies the step: its declared tool ids are built only from `tools:`, and a skill's tool step
    /// is checked against that scope exactly like a model-initiated call.
    pub fn referenced_tool_ids(&self) -> Vec<ToolId> {
        let mut ids: Vec<ToolId> = Vec::new();
        for tool in self
            .instructions
            .iter()
            .filter_map(SkillInstruction::tool_id)
        {
            if !ids.contains(tool) {
                ids.push(tool.clone());
            }
        }
        ids
    }

    /// Whether this skill is one a harness can be asked to run, with the reason if it is not.
    ///
    /// Pure and synchronous: no `async`, no persistence, no mocks in its tests. Every rule here is
    /// a run-time failure moved to write time, where it can be shown on the form that caused it
    /// instead of ending a mail thread in silence.
    pub fn validate(&self) -> Result<(), String> {
        validate_bounded_text("A skill name", &self.name, MAX_SKILL_NAME_CHARS)?;
        validate_bounded_text(
            "A skill description",
            &self.description,
            MAX_SKILL_DESCRIPTION_CHARS,
        )?;
        validate_bounded_text("A skill trigger", &self.trigger, MAX_SKILL_TRIGGER_CHARS)?;

        if self.instructions.is_empty() {
            return Err("A skill needs at least one instruction.".to_string());
        }
        if self.instructions.len() > MAX_SKILL_INSTRUCTIONS {
            return Err(format!(
                "A skill may have at most {MAX_SKILL_INSTRUCTIONS} instructions."
            ));
        }

        for (index, instruction) in self.instructions.iter().enumerate() {
            validate_instruction(index, instruction)?;
        }

        // `ai-agents` returns a response only when the *last* step is a prompt, and otherwise ends
        // the run with "Skill has no prompt step to generate response". A skill ending on a tool
        // step compiles fine and fails mid-run, so it is refused here instead.
        if !self
            .instructions
            .last()
            .is_some_and(SkillInstruction::is_prompt)
        {
            return Err(
                "The last instruction must be a prompt: it is what produces the reply.".to_string(),
            );
        }

        Ok(())
    }
}

fn validate_bounded_text(subject: &str, value: &str, max_chars: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{subject} is required."));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{subject} may be at most {max_chars} characters."));
    }
    Ok(())
}

fn validate_instruction(index: usize, instruction: &SkillInstruction) -> Result<(), String> {
    let position = index + 1;
    match instruction {
        SkillInstruction::Prompt { text } => validate_bounded_text(
            &format!("The prompt in instruction {position}"),
            text,
            MAX_SKILL_INSTRUCTION_CHARS,
        ),
        SkillInstruction::Tool {
            tool,
            args,
            output_as,
        } => {
            // A step naming a tool nobody may be granted is denied by the runtime at the moment it
            // runs. Refusing it here is what stops the picker building a skill that cannot run.
            if CatalogueTool::get(tool).is_none() {
                return Err(format!(
                    "Instruction {position} uses \"{tool}\", which is not a tool agents may be \
                     granted."
                ));
            }

            if let Some(args) = args {
                // The runtime renders args as a template binding and defaults to `{}`; an array or
                // a scalar has nothing to bind.
                let Some(object) = args.as_object() else {
                    return Err(format!(
                        "The arguments in instruction {position} must be a JSON object."
                    ));
                };
                let rendered = serde_json::to_string(object).unwrap_or_default();
                if rendered.chars().count() > MAX_SKILL_INSTRUCTION_CHARS {
                    return Err(format!(
                        "The arguments in instruction {position} may be at most \
                         {MAX_SKILL_INSTRUCTION_CHARS} characters."
                    ));
                }
            }

            if let Some(output_as) = output_as {
                validate_output_as(position, output_as)?;
            }

            Ok(())
        }
    }
}

/// `output_as` names a template variable later steps read, so it is an identifier rather than a
/// label: anything else produces a `{{ ... }}` reference that never binds.
fn validate_output_as(position: usize, output_as: &str) -> Result<(), String> {
    let malformed = format!(
        "The output name in instruction {position} must be a variable name: letters, digits and \
         underscores, not starting with a digit."
    );
    if output_as.trim().is_empty() {
        return Err(malformed);
    }
    if output_as.chars().count() > MAX_SKILL_OUTPUT_AS_CHARS {
        return Err(format!(
            "The output name in instruction {position} may be at most \
             {MAX_SKILL_OUTPUT_AS_CHARS} characters."
        ));
    }
    let charset_ok = output_as
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if !charset_ok || output_as.starts_with(|first: char| first.is_ascii_digit()) {
        return Err(malformed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> SkillInstruction {
        SkillInstruction::Prompt {
            text: text.to_string(),
        }
    }

    fn tool_step(tool: &str) -> SkillInstruction {
        SkillInstruction::Tool {
            tool: ToolId::from(tool),
            args: None,
            output_as: None,
        }
    }

    fn skill(instructions: Vec<SkillInstruction>) -> Skill {
        Skill {
            id: Uuid::new_v4(),
            company_id: Some(Uuid::new_v4()),
            slug: SkillSlug::parse("triage-invoice").expect("a well-formed slug"),
            name: "Triage an invoice".to_string(),
            description: "Decide whether an invoice needs a human.".to_string(),
            trigger: "The message attaches or mentions an invoice.".to_string(),
            instructions,
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_skill_ending_on_a_prompt_step_is_accepted() {
        let skill = skill(vec![
            tool_step("datetime"),
            prompt("Summarise the invoice."),
        ]);

        assert_eq!(skill.validate(), Ok(()));
    }

    #[test]
    fn a_skill_must_end_on_a_prompt_step() {
        let skill = skill(vec![prompt("Read the invoice."), tool_step("datetime")]);

        let error = skill
            .validate()
            .expect_err("a trailing tool step is refused");
        assert!(error.contains("last instruction"), "{error}");
    }

    #[test]
    fn a_skill_step_naming_an_ungrantable_tool_is_refused() {
        let skill = skill(vec![tool_step("command"), prompt("Report what happened.")]);

        let error = skill.validate().expect_err("`command` is not grantable");
        assert!(error.contains("command"), "{error}");
    }

    #[test]
    fn an_empty_instruction_list_is_refused() {
        let error = skill(Vec::new())
            .validate()
            .expect_err("a skill with no instructions is refused");
        assert!(error.contains("at least one"), "{error}");
    }

    #[test]
    fn more_than_thirty_two_instructions_are_refused() {
        let mut instructions: Vec<SkillInstruction> = (0..MAX_SKILL_INSTRUCTIONS)
            .map(|index| prompt(&format!("Step {index}.")))
            .collect();
        assert_eq!(skill(instructions.clone()).validate(), Ok(()));

        instructions.push(prompt("One too many."));
        let error = skill(instructions)
            .validate()
            .expect_err("the instruction count is bounded");
        assert!(error.contains("at most 32"), "{error}");
    }

    #[test]
    fn a_blank_prompt_step_is_refused() {
        let error = skill(vec![prompt("   ")])
            .validate()
            .expect_err("a blank prompt is refused");
        assert!(error.contains("instruction 1"), "{error}");
    }

    #[test]
    fn an_oversized_prompt_step_is_refused() {
        let error = skill(vec![prompt(&"a".repeat(MAX_SKILL_INSTRUCTION_CHARS + 1))])
            .validate()
            .expect_err("the prompt length is bounded");
        assert!(error.contains("at most 8000"), "{error}");
    }

    #[test]
    fn tool_step_args_must_be_a_json_object() {
        for args in [
            serde_json::json!(["a", "b"]),
            serde_json::json!("a"),
            serde_json::json!(7),
        ] {
            let step = SkillInstruction::Tool {
                tool: ToolId::from("datetime"),
                args: Some(args.clone()),
                output_as: None,
            };
            let error = skill(vec![step, prompt("Answer.")])
                .validate()
                .expect_err("only an object binds as arguments");
            assert!(error.contains("JSON object"), "{args}: {error}");
        }

        let step = SkillInstruction::Tool {
            tool: ToolId::from("datetime"),
            args: Some(serde_json::json!({ "operation": "now" })),
            output_as: None,
        };
        assert_eq!(skill(vec![step, prompt("Answer.")]).validate(), Ok(()));
    }

    #[test]
    fn an_output_name_must_be_a_variable_name() {
        for output_as in ["", "  ", "1st", "today's date", "date-today"] {
            let step = SkillInstruction::Tool {
                tool: ToolId::from("datetime"),
                args: None,
                output_as: Some(output_as.to_string()),
            };
            let error = skill(vec![step, prompt("Answer.")])
                .validate()
                .expect_err("an output name is an identifier");
            assert!(error.contains("instruction 1"), "{output_as:?}: {error}");
        }

        let step = SkillInstruction::Tool {
            tool: ToolId::from("datetime"),
            args: None,
            output_as: Some("today_utc".to_string()),
        };
        assert_eq!(skill(vec![step, prompt("Answer.")]).validate(), Ok(()));
    }

    #[test]
    fn a_blank_description_or_trigger_is_refused() {
        let mut without_description = skill(vec![prompt("Answer.")]);
        without_description.description = "  ".to_string();
        assert!(
            without_description
                .validate()
                .expect_err("a description is required")
                .contains("description")
        );

        let mut without_trigger = skill(vec![prompt("Answer.")]);
        without_trigger.trigger = String::new();
        assert!(
            without_trigger
                .validate()
                .expect_err("a trigger is required")
                .contains("trigger")
        );
    }

    #[test]
    fn referenced_tool_ids_deduplicates_and_preserves_first_use_order() {
        let skill = skill(vec![
            tool_step("datetime"),
            tool_step("json"),
            prompt("Think."),
            tool_step("datetime"),
            tool_step("math"),
            prompt("Answer."),
        ]);

        assert_eq!(
            skill.referenced_tool_ids(),
            [
                ToolId::from("datetime"),
                ToolId::from("json"),
                ToolId::from("math"),
            ]
        );
    }

    #[test]
    fn a_skill_with_no_tool_steps_references_no_tools() {
        assert!(
            skill(vec![prompt("Answer.")])
                .referenced_tool_ids()
                .is_empty()
        );
    }

    #[test]
    fn an_instruction_round_trips_through_its_stored_shape() {
        let instructions = vec![
            SkillInstruction::Tool {
                tool: ToolId::from("datetime"),
                args: Some(serde_json::json!({ "operation": "now" })),
                output_as: Some("today_utc".to_string()),
            },
            prompt("Answer using {{ today_utc }}."),
        ];

        let stored = serde_json::to_value(&instructions).expect("instructions serialize");
        assert_eq!(
            stored[0]["kind"], "tool",
            "the tag is what keeps a new variant additive"
        );
        assert_eq!(stored[1]["kind"], "prompt");
        assert!(
            stored[1].get("args").is_none(),
            "an absent option stores nothing"
        );

        let loaded: Vec<SkillInstruction> =
            serde_json::from_value(stored).expect("instructions deserialize");
        assert_eq!(loaded, instructions);
    }
}
