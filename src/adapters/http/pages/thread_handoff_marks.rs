//! The held-reply badge on a mailbox thread row, and the slot it lives in.
//!
//! A second, independent mark beside [`super::thread_activity`]'s rather than a
//! `ThreadActivity` variant. `ThreadActivity` is derived from a `background_tasks` row, and a held
//! reply has no task -- that is the whole point of the hold. A variant there would have to be
//! synthesised from outside the task table, and every consumer of it (the schedules badge, the
//! collaboration strip, the task board) would start rendering a state with no task behind it.

use super::*;

/// What a thread row says about its handoff. Absent when the thread has no open handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadHandoffMark {
    pub state: ThreadHandoffState,
    /// Whether anybody has claimed it. The row says "unclaimed" louder than it says "claimed".
    pub claimed: bool,
}

impl ThreadHandoffMark {
    /// The mark one open handoff produces, as the row needs it.
    pub const fn of(handoff: &ThreadHandoff) -> Self {
        Self {
            state: handoff.state,
            claimed: handoff.responsible_principal_id.is_some(),
        }
    }

    /// The long form, which is what a tooltip and the banner's state line both say.
    pub const fn label(self) -> &'static str {
        match self.state {
            ThreadHandoffState::NeedsInstruction => "Needs instruction",
            ThreadHandoffState::Drafting => "Drafting",
            ThreadHandoffState::DraftReady => "Draft ready",
            // A closed handoff is not returned by the read that feeds this, so it has no mark.
            ThreadHandoffState::Resolved | ThreadHandoffState::Dismissed => "",
        }
    }

    /// The short form, for a narrow thread column. Only the one that does not fit is shortened.
    const fn short_label(self) -> &'static str {
        match self.state {
            ThreadHandoffState::NeedsInstruction => "Needs input",
            _ => self.label(),
        }
    }

    /// The badge colour, which the banner's state line matches so the two read as one thing.
    pub const fn badge_class(self) -> &'static str {
        match self.state {
            ThreadHandoffState::NeedsInstruction => "badge-warning",
            ThreadHandoffState::DraftReady => "badge-info",
            // Out of the attention queue and waiting on nobody, so the quietest of the three: a
            // row that shouted would contradict the queue it is not in.
            ThreadHandoffState::Drafting
            | ThreadHandoffState::Resolved
            | ThreadHandoffState::Dismissed => "badge-ghost",
        }
    }
}

/// The compact mark for a thread row. Empty string for `None`, which is what clears the slot.
///
/// A **word**, not a glyph, unlike every other mark on this row. This is the only one that asks
/// the reader to do something, and an unlabelled dot has never once communicated that. The long
/// form goes in `title`, so a column too narrow for "Needs instruction" still has it on hover.
///
/// No `animate-pulse` in any state. The activity mark pulses because a running task is transient
/// and clears itself; a handoff is durable and waits for a person, and a badge that pulses forever
/// is noise.
pub fn thread_handoff_mark(mark: Option<ThreadHandoffMark>) -> String {
    // Terminal states never reach a row -- `thread_handoffs_for_threads` returns open handoffs
    // only -- and an empty label would render an empty badge, so they clear the slot instead.
    match mark.filter(|mark| !mark.label().is_empty()) {
        None => String::new(),
        Some(mark) => format!(
            r##"<span class="badge badge-xs shrink-0 {class}" title="{title}">{label}</span>"##,
            class = mark.badge_class(),
            title = escape_html_text(mark.label()),
            label = escape_html_text(mark.short_label()),
        ),
    }
}

/// An independently replaceable handoff mark within a live thread row.
pub fn thread_handoff_slot(thread_id: Uuid, mark: Option<ThreadHandoffMark>) -> String {
    format!(
        r##"<span class="thread-handoff-mark" sse-swap="{event}" hx-target="this" hx-swap="innerHTML">{mark}</span>"##,
        event = thread_handoff_event(thread_id),
        mark = thread_handoff_mark(mark),
    )
}

/// The SSE event name carrying one thread's handoff mark.
pub fn thread_handoff_event(thread_id: Uuid) -> String {
    format!("handoff-{thread_id}")
}
