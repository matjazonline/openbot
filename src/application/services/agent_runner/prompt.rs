//! Assembling what the model sees, and fencing everything in it that somebody else wrote.
//!
//! This is the runner's, not a harness's: whichever runtime executes the agent is handed a prompt
//! that has already been composed, fenced and guardrailed. Moving it behind the port would mean
//! every new harness re-deciding what an untrusted block looks like.

use crate::entities::internal_note::AgentInstructionNote;
use crate::entities::message::{MessageAudience, MessageRole, ThreadEntryKind};
use crate::entities::message_view::AgentHistoryMessage;
use crate::services::prompt_fence::{UntrustedFence, UntrustedKind};
use crate::use_cases::thread::RecipientRole;

/// Cap on a rendered subject: long enough for any real one, short enough that a hostile header
/// cannot crowd out the message it belongs to.
pub(super) const MAX_PROMPT_SUBJECT_CHARS: usize = 200;

/// The parts of one turn that reach the model, in the order they are rendered.
///
/// A struct rather than five positional arguments: `prompt`, `subject` and the upstream context
/// are three `&str`-shaped values of different meaning, which is exactly the swap `src/AGENTS.md`
/// names -- and swapping the message with the upstream context would fence the wrong one first.
pub struct PromptParts<'a> {
    /// The message this turn is answering.
    pub message: &'a str,
    /// Subject of the message `message` came from, if it has one.
    pub subject: Option<&'a str>,
    pub history: &'a [AgentHistoryMessage],
    pub internal_notes: &'a [AgentInstructionNote],
    /// Output of the prior agent in a pipeline, when this run is not the first step.
    pub upstream: Option<&'a str>,
    /// Whether the agent was addressed directly or copied.
    pub recipient_role: Option<RecipientRole>,
}

impl PromptParts<'_> {
    /// Assemble what the model sees: delivery context, upstream pipeline output, conversation
    /// history, then the message itself.
    ///
    /// Everything written by someone other than the operator -- the upstream step's output, the
    /// thread, and the message with its subject -- goes inside `fence`. The section labels stay
    /// outside it, so a body containing the line `Latest Inbound Message:` reads as something
    /// somebody typed rather than as the frame the model is reading in.
    pub fn compose(&self, fence: &UntrustedFence) -> String {
        let delivery_ctx = match self.recipient_role {
            Some(RecipientRole::To) => {
                "[Delivery Context: Email received via TO field (Primary Target)]\n"
            }
            Some(RecipientRole::Cc) => {
                "[Delivery Context: Email received via CC field (Secondary / FYI Target)]\n"
            }
            None => "",
        };

        let pipeline_ctx = self
            .upstream
            .map(str::trim)
            .filter(|upstream| !upstream.is_empty())
            .map(|upstream| {
                format!(
                    "[Upstream Pipeline Context from Prior Step Agents]:\n{}\n\n",
                    fence.wrap(UntrustedKind::UpstreamOutput, upstream)
                )
            })
            .unwrap_or_default();

        let subject_line = self
            .subject
            .and_then(prompt_subject)
            .map(|subject| format!("Subject: {subject}\n"))
            .unwrap_or_default();

        format!(
            "{}{}{}{}{}",
            delivery_ctx,
            pipeline_ctx,
            self.render_internal_notes(fence),
            self.render_history(fence),
            fence.wrap(
                UntrustedKind::Message,
                &format!("{subject_line}{}", self.message)
            )
        )
    }

    fn render_internal_notes(&self, fence: &UntrustedFence) -> String {
        if self.internal_notes.is_empty() {
            return String::new();
        }
        let rendered = self
            .internal_notes
            .iter()
            .map(|note| {
                format!(
                    "[InternalOnly | Note | Author: {} | At: {} | Note: {} | Supersedes: {}]: {}",
                    note.author_display,
                    note.created_at.to_rfc3339(),
                    note.note_id,
                    note.supersedes_note_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "none".into()),
                    note.body,
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Selected Internal Notes (private, untrusted data):\n{}\n\n",
            fence.wrap(UntrustedKind::InternalNote, &rendered)
        )
    }

    /// The thread so far, one line per message, fenced as one block: every line of it is text
    /// somebody else wrote, the addresses the lines are attributed to included.
    ///
    /// A message carries its subject when it is the first line or when the topic actually changed;
    /// repeating the same `Re:` on every line would be noise the model has to read past.
    fn render_history(&self, fence: &UntrustedFence) -> String {
        if self.history.is_empty() {
            return String::new();
        }

        let mut rendered = String::new();
        let mut shown_stem: Option<&str> = None;
        for msg in self.history {
            let role_label = match msg.role {
                MessageRole::Human => "User",
                MessageRole::Agent => "Agent",
                MessageRole::System => "System",
            };
            let boundary_label = match msg.audience {
                MessageAudience::ExternalConversation => "External conversation",
                MessageAudience::InternalOnly => "Internal-only",
                MessageAudience::LegacyUnclassified => "Legacy unclassified; do not disclose",
            };
            let entry_label = match msg.entry_kind {
                ThreadEntryKind::Conversation => "Conversation",
                ThreadEntryKind::Note => "Note",
                ThreadEntryKind::Delegation => "Delegation",
                ThreadEntryKind::SystemEvent => "System event",
            };
            let stem = subject_stem(&msg.subject);
            let changed = shown_stem.is_none_or(|shown| !shown.eq_ignore_ascii_case(stem));
            let subject_label = match prompt_subject(&msg.subject).filter(|_| changed) {
                Some(subject) => {
                    shown_stem = Some(stem);
                    format!(" | Subject: {subject}")
                }
                None => String::new(),
            };
            rendered.push_str(&format!(
                "[{boundary_label} | {entry_label} | {} ({}){}]: {}\n",
                role_label, msg.author_display, subject_label, msg.body
            ));
        }
        format!(
            "Conversation History:\n{}\n\nLatest Inbound Message:\n",
            fence.wrap(UntrustedKind::History, rendered.trim_end())
        )
    }
}

/// A subject as it can safely be rendered into the prompt: a single line of bounded length, `None`
/// when there is nothing to show. Collapsing the whitespace is what keeps a header carrying its own
/// newlines from forging the section markers the prompt is built from.
pub(super) fn prompt_subject(subject: &str) -> Option<String> {
    let mut collapsed = String::with_capacity(subject.len());
    for word in subject.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
    }
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.chars().count() > MAX_PROMPT_SUBJECT_CHARS {
        collapsed = collapsed
            .chars()
            .take(MAX_PROMPT_SUBJECT_CHARS)
            .chain(std::iter::once('\u{2026}'))
            .collect();
    }
    Some(collapsed)
}

/// A subject stripped of its reply and forward prefixes, for asking whether the *topic* changed.
/// `Re: Invoice` and `Invoice` are the same conversation, and every agent reply is stored in the
/// `Re:` form, so comparing raw subjects would report a change on every turn.
pub(super) fn subject_stem(subject: &str) -> &str {
    let mut rest = subject.trim();
    loop {
        let prefix = ["re:", "fwd:", "fw:"].into_iter().find(|prefix| {
            rest.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        });
        match prefix {
            Some(prefix) => rest = rest[prefix.len()..].trim_start(),
            None => return rest,
        }
    }
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
