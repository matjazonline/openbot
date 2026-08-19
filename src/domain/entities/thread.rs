use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::cursor::ThreadCursor;
use crate::entities::value_objects::EmailAddress;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Thread {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub subject: String,
    pub participant_emails: Vec<EmailAddress>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Thread {
    /// This thread's position in its channel's newest-first column — where paging continues from,
    /// and where a live column resumes from.
    pub fn cursor(&self) -> ThreadCursor {
        ThreadCursor::new(self.updated_at, self.id)
    }

    /// The subject a further message in this thread carries: `Re:` once, never `Re: Re:`.
    pub fn reply_subject(&self) -> String {
        let subject = self.subject.trim();
        if subject.to_lowercase().starts_with("re:") {
            subject.to_string()
        } else {
            format!("Re: {subject}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread_with_subject(subject: &str) -> Thread {
        Thread {
            id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            subject: subject.to_string(),
            participant_emails: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn reply_subject_prefixes_once() {
        assert_eq!(
            thread_with_subject("Invoice question").reply_subject(),
            "Re: Invoice question"
        );
        assert_eq!(
            thread_with_subject("Re: Invoice question").reply_subject(),
            "Re: Invoice question"
        );
        assert_eq!(
            thread_with_subject("  RE: Invoice question ").reply_subject(),
            "RE: Invoice question"
        );
    }
}
