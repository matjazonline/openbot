//! A skill: reusable prompt instructions an agent may be given (06-D5). Executable tool steps
//! were removed in 06C; procedures that call tools are workflows, and a skill names the tools it
//! needs as `required_tools` instead.
//!
//! The instructions are one text. Every linked skill is pasted into the agent's prompt in full, so
//! the model never sees them as separate items, and a list would only be an editing affordance.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::{creation::CreationProvenance, value_objects::SkillSlug};

/// The workflow publication bound on any one prompt text (`registry::MAX_TEXT`), in UTF-8 bytes.
/// Checked here so a skill that saves is a skill that publishes; a static assertion beside the
/// publication check keeps the two from drifting.
pub const MAX_SKILL_INSTRUCTIONS_BYTES: usize = 16_384;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 500;
pub const MAX_SKILL_NAME_CHARS: usize = 120;

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
    pub instructions: String,
    pub created_by: CreationProvenance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

        if self.instructions.trim().is_empty() {
            return Err("A skill's instructions are required.".to_string());
        }
        if self.instructions.len() > MAX_SKILL_INSTRUCTIONS_BYTES {
            return Err(format!(
                "A skill's instructions may be at most {MAX_SKILL_INSTRUCTIONS_BYTES} bytes."
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(instructions: &str) -> Skill {
        Skill {
            id: Uuid::new_v4(),
            company_id: Some(Uuid::new_v4()),
            slug: SkillSlug::parse("triage-invoice").expect("a well-formed slug"),
            name: "Triage an invoice".to_string(),
            description: "Decide whether an invoice needs a human.".to_string(),
            instructions: instructions.to_string(),
            created_by: CreationProvenance::system(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_skill_with_instructions_is_accepted() {
        assert_eq!(
            skill("Read the invoice.\n\nSummarise it.").validate(),
            Ok(())
        );
    }

    #[test]
    fn blank_instructions_are_refused() {
        let error = skill("   ").validate().expect_err("no instructions");
        assert!(error.contains("required"), "{error}");
    }

    /// The bound is the publication one, in bytes: text that fits in characters but not in bytes
    /// would save and then fail every workflow that names the skill.
    #[test]
    fn instructions_are_bounded_in_bytes_not_characters() {
        assert_eq!(
            skill(&"x".repeat(MAX_SKILL_INSTRUCTIONS_BYTES)).validate(),
            Ok(())
        );
        let multibyte = "é".repeat(MAX_SKILL_INSTRUCTIONS_BYTES / 2 + 1);
        assert!(multibyte.chars().count() < MAX_SKILL_INSTRUCTIONS_BYTES);
        let error = skill(&multibyte).validate().expect_err("too many bytes");
        assert!(error.contains("bytes"), "{error}");
    }

    #[test]
    fn a_blank_description_is_refused() {
        let mut blank = skill("Answer.");
        blank.description = " ".into();
        assert!(blank.validate().is_err());
    }
}
