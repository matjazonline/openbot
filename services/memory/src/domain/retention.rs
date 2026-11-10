//! Retention windows (06A.1). Generation inputs and raw evidence use
//! [`super::generation::RETENTION_DAYS`]; the audit trigger in the schema repeats the audit window.

/// Idempotency records are kept this long, and until their request deadline has passed (D5).
pub const IDEMPOTENCY_RETENTION_DAYS: u32 = 7;
/// Audit events and daily spend rows are kept this long.
pub const AUDIT_RETENTION_DAYS: u32 = 400;
pub const SPEND_RETENTION_DAYS: u32 = 400;
/// A finished operation no observation references, and a source whose content was nulled and
/// that nothing references, are removed this long after finishing or nulling (06A A1 F5).
pub const METADATA_RETENTION_DAYS: u32 = 30;
/// The label every credential of a removed tenant carries (06A A1 F4): labels are free text.
pub const REMOVED_LABEL: &str = "removed";
