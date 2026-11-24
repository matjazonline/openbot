//! How a channel response closes, fixed by the run's terminal step (gate AWC-P04-D1 D2).
use crate::app_error::{AppError, AppResult};
use crate::application::transport::{CanonicalContent, MAX_SUBJECT_BYTES};
use crate::domain::workflow::RunId;

/// The response kind a proposal carries and review approves with its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyClosure {
    /// The reply contract's projection of the business output.
    Done,
    /// `channel.deny`: the step's reason is the body.
    Denied,
    /// `channel.hand_off`: the reply of a nested call this run consumed as `done`.
    HandedOff(RunId),
}

impl ReplyClosure {
    /// The closure a terminal step of this kind gives; `None` for `channel.no_reply`, which
    /// suppresses the response instead.
    pub fn of_step(kind: &str, output: &serde_json::Value) -> AppResult<Option<Self>> {
        Ok(Some(match kind {
            "channel.no_reply" => return Ok(None),
            "channel.deny" => Self::Denied,
            "channel.hand_off" => Self::HandedOff(handed_off_run(output)?),
            _ => Self::Done,
        }))
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Denied => "denied",
            Self::HandedOff(_) => "handed_off",
        }
    }

    pub fn handed_off_run(self) -> Option<RunId> {
        match self {
            Self::HandedOff(run) => Some(run),
            _ => None,
        }
    }
}

/// The run a `channel.hand_off` output names, as its step contract typed it.
pub fn handed_off_run(output: &serde_json::Value) -> AppResult<RunId> {
    output
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|run| uuid::Uuid::parse_str(run).ok())
        .map(RunId::new)
        .ok_or_else(invalid)
}

/// A denial's content: `Re:` once on the source subject, within the subject bound, and the
/// step's reason as the body.
pub fn denial(source_subject: &str, reason: &str) -> AppResult<CanonicalContent> {
    let subject = source_subject.trim();
    let subject = if subject
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("re:"))
    {
        subject.to_owned()
    } else {
        format!("Re: {subject}")
    };
    let mut end = subject.len().min(MAX_SUBJECT_BYTES);
    while !subject.is_char_boundary(end) {
        end -= 1;
    }
    let content =
        CanonicalContent::parse(subject[..end].trim_end(), reason).map_err(|_| invalid())?;
    if content.body_text().trim().is_empty() {
        return Err(invalid());
    }
    Ok(content)
}

fn invalid() -> AppError {
    AppError::BadRequest("A denial needs a non-empty reason within its bound".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_reply_closure_denial_prefixes_once_and_bounds_the_subject() {
        assert_eq!(denial("Refund", "No").unwrap().subject(), "Re: Refund");
        assert_eq!(denial("RE: Refund", "No").unwrap().subject(), "RE: Refund");
        let long = "é".repeat(MAX_SUBJECT_BYTES);
        assert!(denial(&long, "No").unwrap().subject().len() <= MAX_SUBJECT_BYTES);
        assert!(denial("Refund", " ").is_err());
    }

    #[test]
    fn channel_reply_closure_follows_the_terminal_step() {
        let run = uuid::Uuid::new_v4();
        let output = serde_json::json!({"run_id": run.to_string()});
        assert_eq!(
            ReplyClosure::of_step("data.map", &output).unwrap(),
            Some(ReplyClosure::Done)
        );
        assert_eq!(
            ReplyClosure::of_step("channel.no_reply", &output).unwrap(),
            None
        );
        assert_eq!(
            ReplyClosure::of_step("channel.hand_off", &output).unwrap(),
            Some(ReplyClosure::HandedOff(RunId::new(run)))
        );
        assert!(ReplyClosure::of_step("channel.hand_off", &serde_json::json!({})).is_err());
    }
}
