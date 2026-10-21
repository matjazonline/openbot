//! A skill: reusable prompt instructions an agent may be given (06-D5). Executable tool steps
//! were removed in 06C; procedures that call tools are workflows, and a skill names the tools it
//! needs as `required_tools` instead.
//!
//! The instructions are stored as one JSONB array of typed values rather than as a `steps` table.
//! A skill is a handful of ordered items edited as a single document, and a second table would buy
//! referential integrity nobody needs at the cost of a join on every agent run.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{creation::CreationProvenance, value_objects::SkillSlug};

pub const MAX_SKILL_INSTRUCTIONS: usize = 32;
pub const MAX_SKILL_INSTRUCTION_CHARS: usize = 8_000;
pub const MAX_SKILL_TRIGGER_CHARS: usize = 500;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 500;
pub const MAX_SKILL_NAME_CHARS: usize = 120;
pub const MAX_SKILL_INSTRUCTIONS_JSON_BYTES: usize = 524_288;

/// One skill as it is stored and edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub id: Uuid,
    /// `None` identifies an operator-managed definition in the global skill library.
    #[serde(default)]
    pub company_id: Option<Uuid>,
    pub slug: SkillSlug,
    pub name: String,
    /// The subtitle a picker shows.
    pub description: String,
    /// When an agent should reach for this skill.
    pub trigger: String,
    pub instructions: Vec<SkillInstruction>,
    pub created_by: CreationProvenance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One instruction of a skill: prompt text.
///
/// Externally tagged so the stored shape (`{"kind":"prompt","text":...}`) names its kind; a new
/// kind would be additive rather than a silent reinterpretation of stored rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillInstruction {
    Prompt { text: String },
}

impl SkillInstruction {
    pub fn text(&self) -> &str {
        match self {
            Self::Prompt { text } => text,
        }
    }
}

impl Skill {
    pub fn is_library(&self) -> bool {
        self.company_id.is_none()
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
    validate_bounded_text(
        &format!("The prompt in instruction {}", index + 1),
        instruction.text(),
        MAX_SKILL_INSTRUCTION_CHARS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> SkillInstruction {
        SkillInstruction::Prompt {
            text: text.to_string(),
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
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_skill_of_prompts_is_accepted() {
        let skill = skill(vec![
            prompt("Read the invoice."),
            prompt("Summarise the invoice."),
        ]);
        assert_eq!(skill.validate(), Ok(()));
    }

    #[test]
    fn an_empty_instruction_list_is_refused() {
        let error = skill(vec![]).validate().expect_err("no instructions");
        assert!(error.contains("at least one"), "{error}");
    }

    #[test]
    fn more_than_thirty_two_instructions_are_refused() {
        let many = (0..=MAX_SKILL_INSTRUCTIONS)
            .map(|index| prompt(&format!("Step {index}")))
            .collect();
        let error = skill(many).validate().expect_err("too many instructions");
        assert!(error.contains("at most"), "{error}");
    }

    #[test]
    fn a_blank_or_oversized_prompt_is_refused() {
        assert!(skill(vec![prompt("   ")]).validate().is_err());
        let long = "x".repeat(MAX_SKILL_INSTRUCTION_CHARS + 1);
        assert!(skill(vec![prompt(&long)]).validate().is_err());
    }

    #[test]
    fn a_blank_description_or_trigger_is_refused() {
        let mut blank = skill(vec![prompt("Answer.")]);
        blank.description = " ".into();
        assert!(blank.validate().is_err());
        let mut blank = skill(vec![prompt("Answer.")]);
        blank.trigger = String::new();
        assert!(blank.validate().is_err());
    }

    /// 06C: a stored executable tool step is no longer a skill instruction.
    #[test]
    fn a_prompt_round_trips_and_a_tool_step_does_not_decode() {
        let stored = serde_json::json!({"kind":"prompt","text":"Answer briefly."});
        let decoded: SkillInstruction = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), stored);
        assert!(
            serde_json::from_value::<SkillInstruction>(
                serde_json::json!({"kind":"tool","tool":"datetime"})
            )
            .is_err()
        );
    }
}
