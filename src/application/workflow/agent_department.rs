//! The department context of an `agent.run` on a department-owned desk (01DEP-P03-D11): the
//! department's name and charter and, on a desk that is not public, the desks around it. Names,
//! charters and addresses are company-managed, so the preamble carries them as instructions.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The most child departments a roster lists, by slug.
pub const MAX_ROSTER_CHILDREN: usize = 20;
/// The most agents a roster lists, by slug.
pub const MAX_ROSTER_AGENTS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentContext {
    pub name: String,
    pub slug: String,
    pub charter: Option<String>,
    /// The run's agent holds this department's `lead` role.
    pub lead: bool,
    /// Absent on a public desk, whose conversations are with outsiders (01DEP-P03-G3-F3).
    pub roster: Option<DepartmentRoster>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartmentRoster {
    pub desk: Option<RosterDesk>,
    pub parent: Option<RosterDepartment>,
    pub children: Vec<RosterDepartment>,
    pub more_children: bool,
    pub agents: Vec<RosterAgent>,
    pub more_agents: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterDesk {
    pub id: Uuid,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterDepartment {
    pub name: String,
    pub slug: String,
    pub desk: Option<RosterDesk>,
}

/// An agent member or role holder, with its own default desk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterAgent {
    pub name: String,
    pub slug: String,
    pub lead: bool,
    pub desk: Option<RosterDesk>,
}

impl DepartmentContext {
    /// Appended to the agent's own instructions, before its skills. It lists addresses only:
    /// a message sent to a desk does not start that desk's work (C004).
    pub(super) fn render(&self, text: &mut String) {
        text.push_str(&format!(
            "\n\nDepartment: {} ({}). This conversation is on a desk the department owns.",
            self.name, self.slug
        ));
        if let Some(charter) = self
            .charter
            .as_deref()
            .filter(|charter| !charter.is_empty())
        {
            text.push_str("\nCharter:\n");
            text.push_str(charter);
        }
        if self.lead {
            text.push_str("\nYou answer as this department's lead.");
        }
        let Some(roster) = &self.roster else {
            return;
        };
        text.push_str("\nDesks around this department:");
        line(text, "This department's desk", roster.desk.as_ref());
        if let Some(parent) = &roster.parent {
            line(
                text,
                &format!("Parent department {} ({})", parent.name, parent.slug),
                parent.desk.as_ref(),
            );
        }
        for child in &roster.children {
            line(
                text,
                &format!("Child department {} ({})", child.name, child.slug),
                child.desk.as_ref(),
            );
        }
        if roster.more_children {
            text.push_str("\n- More child departments exist than are listed.");
        }
        for agent in &roster.agents {
            let role = if agent.lead { ", lead" } else { "" };
            line(
                text,
                &format!("Agent {} ({}{role})", agent.name, agent.slug),
                agent.desk.as_ref(),
            );
        }
        if roster.more_agents {
            text.push_str("\n- More agents belong to this department than are listed.");
        }
    }
}

fn line(text: &mut String, label: &str, desk: Option<&RosterDesk>) {
    text.push_str("\n- ");
    text.push_str(label);
    match desk {
        Some(desk) => {
            text.push_str(": ");
            text.push_str(&desk.address);
        }
        None => text.push_str(": no desk"),
    }
}
