//! Removing the conversation someone's client appended to their reply.
//!
//! Pure text work over lines, with no protocol types in sight: mail clients quote with `>` and an
//! "On … wrote:" attribution, chat clients quote by repeating the text, and both are the same
//! problem -- the part of the body the sender actually wrote is the part that is not a copy of what
//! came before it.
//!
//! It lives in the domain because the *decision* to strip needs the thread's history, which only
//! the application has, while the heuristics themselves belong to nobody in particular. Keeping
//! them here is what lets the ingest pipeline strip a body without importing a mail parser.

/// The part of `body` the sender wrote: quote markers first, then anything the thread already said.
///
/// Two passes because they catch different clients. The marker pass handles the ones that announce
/// the quote; the history pass handles the ones that simply paste, which no marker can find.
pub fn strip(body: &str, history: &[&str]) -> String {
    let marked = strip_quote_markers(body);
    strip_repeated_history(&marked, history)
}

/// Everything before the first line that announces quoted text.
pub fn strip_quote_markers(text: &str) -> String {
    let mut kept = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        let announces_quote = trimmed.starts_with('>')
            || lower.contains("-----original message-----")
            || lower.contains("-----forwarded message-----")
            || lower.starts_with("___________")
            // "On <date>, <someone> wrote:", in the three languages seen in the wild.
            || ((lower.starts_with("on ") || lower.starts_with("am ") || lower.starts_with("le "))
                && lower.ends_with("wrote:"))
            // The header block a forward or an Outlook reply pastes above the quoted body.
            || (lower.starts_with("from:")
                && (lower.contains("sent:") || lower.contains("date:") || lower.contains("to:")));

        if announces_quote {
            break;
        }
        kept.push(line);
    }

    kept.join("\n").trim().to_string()
}

/// Everything before the first line the conversation has already seen verbatim.
///
/// The fallback for clients that quote without a marker: the first line that appears in an earlier
/// message is where this sender stopped writing and started repeating.
pub fn strip_repeated_history(text: &str, history: &[&str]) -> String {
    if history.is_empty() {
        return text.to_string();
    }

    let mut kept = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            kept.push(line);
            continue;
        }
        let seen_before = history.iter().any(|earlier| {
            earlier
                .lines()
                .any(|earlier_line| earlier_line.trim() == trimmed)
        });
        if seen_before {
            break;
        }
        kept.push(line);
    }
    kept.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_ends_the_part_the_sender_wrote() {
        let body = "Thanks, that works.\n\nOn Tue, 1 Jan 2030, Someone wrote:\n> the old text";
        assert_eq!(strip_quote_markers(body), "Thanks, that works.");
    }

    #[test]
    fn a_client_that_pastes_without_a_marker_is_caught_by_the_history() {
        let history = ["Could you confirm the date?"];
        let body = "Confirmed for Friday.\n\nCould you confirm the date?";
        assert_eq!(strip(body, &history), "Confirmed for Friday.");
    }

    /// Nothing is stripped from a message with nothing to strip: the whole body survives both
    /// passes, which is the case every first message in a conversation takes.
    #[test]
    fn an_original_message_survives_intact() {
        let body = "Line one\n\nLine two";
        assert_eq!(strip(body, &[]), body);
        assert_eq!(strip(body, &["something else entirely"]), body);
    }
}
