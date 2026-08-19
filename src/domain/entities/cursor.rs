//! Positions in a time-ordered list, for resuming a live stream or paging a column.
//!
//! Every ordered list in the mailbox is keyed on `(timestamp, id)` rather than the timestamp
//! alone, because rows written in one transaction share a timestamp and a timestamp-only cursor
//! would skip or repeat them. The `id` tie-break is what makes "everything after this point" exact.
//!
//! The cursors are separate types even though they share a shape: a thread position and a message
//! position are not interchangeable, and passing one where the other is expected should not
//! compile. See `src/AGENTS.md`.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// How a cursor is written into a URL. Compact and sortable, with the fractional seconds that
/// separate rows written in the same second.
const CURSOR_TIMESTAMP_FORMAT: &str = "%Y%m%dT%H%M%S%.f";

/// Define a `(timestamp, id)` cursor type with a URL-safe string form.
macro_rules! timestamp_id_cursor {
    ($name:ident, $timestamp:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name {
            // Field order is the sort order: `derive(Ord)` compares the timestamp first and falls
            // back to the id, matching the `ORDER BY` and the row comparison in the SQL.
            pub $timestamp: DateTime<Utc>,
            pub id: Uuid,
        }

        impl $name {
            pub fn new($timestamp: DateTime<Utc>, id: Uuid) -> Self {
                Self { $timestamp, id }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "{}_{}",
                    self.$timestamp.format(CURSOR_TIMESTAMP_FORMAT),
                    self.id
                )
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let invalid = || format!(concat!("Invalid ", stringify!($name), ": {}"), s);
                // The UUID is what follows the *last* separator: the timestamp contains none.
                let ($timestamp, id) = s.rsplit_once('_').ok_or_else(invalid)?;
                Ok($name {
                    $timestamp: chrono::NaiveDateTime::parse_from_str(
                        $timestamp,
                        CURSOR_TIMESTAMP_FORMAT,
                    )
                    .map_err(|_| invalid())?
                    .and_utc(),
                    id: Uuid::parse_str(id).map_err(|_| invalid())?,
                })
            }
        }
    };
}

// Where one message sits in its thread, so a live reader resumes from the message it last saw.
timestamp_id_cursor!(MessageCursor, created_at);

// Where one thread sits in its channel's newest-first column. Doubles as the paging cursor for
// "load older threads" and as the resume point for the live thread column.
timestamp_id_cursor!(ThreadCursor, updated_at);

#[cfg(test)]
mod tests {
    use super::*;

    fn at(timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn id(last: u8) -> Uuid {
        Uuid::parse_str(&format!("0a8f5f5e-0000-4000-8000-0000000000{last:02}")).unwrap()
    }

    #[test]
    fn cursors_round_trip_through_their_string_form() {
        // Whole seconds and sub-second precision both have to survive: `%.f` writes nothing at all
        // when the fraction is zero, and the parse has to accept that.
        for timestamp in ["2026-08-19T10:15:00Z", "2026-08-19T10:15:00.123456Z"] {
            let message = MessageCursor::new(at(timestamp), id(1));
            assert_eq!(
                message.to_string().parse::<MessageCursor>().unwrap(),
                message
            );

            let thread = ThreadCursor::new(at(timestamp), id(1));
            assert_eq!(thread.to_string().parse::<ThreadCursor>().unwrap(), thread);
        }
    }

    #[test]
    fn cursors_order_by_timestamp_then_id() {
        assert!(
            MessageCursor::new(at("2026-08-19T10:15:00Z"), id(9))
                < MessageCursor::new(at("2026-08-19T10:15:01Z"), id(1)),
            "timestamp wins over id"
        );

        // The tie-break the resume queries depend on: same instant, ordered by id.
        assert!(
            ThreadCursor::new(at("2026-08-19T10:15:00Z"), id(1))
                < ThreadCursor::new(at("2026-08-19T10:15:00Z"), id(2))
        );
    }

    #[test]
    fn malformed_cursors_are_rejected_rather_than_ignored() {
        for input in [
            "",
            "20260819T101500", // no id
            "not-a-timestamp_0a8f5f5e-0000-4000-8000-000000000001",
            "20260819T101500_not-a-uuid",
            "20260819T101500_",
        ] {
            assert!(
                input.parse::<MessageCursor>().is_err(),
                "expected {input:?} to be rejected"
            );
            assert!(
                input.parse::<ThreadCursor>().is_err(),
                "expected {input:?} to be rejected"
            );
        }
    }
}
