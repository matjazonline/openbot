//! Code-owned protocol limits. Adapters must reject before parsing or dispatching.
use chrono::{DateTime, Utc};

pub use crate::domain::local_agents::{MAX_PROMPT_BYTES, MAX_RESULT_BYTES};

pub const PROTOCOL_VERSION: u16 = 1;
pub const PAIRING_ENTROPY_BYTES: usize = 16;
pub const PAIRING_TTL_SECONDS: i64 = 600;
pub const MAX_PAIRING_CODES: usize = 5;
pub const HANDSHAKE_SECONDS: u64 = 10;
pub const HEARTBEAT_SECONDS: u64 = 15;
pub const CONNECTION_STALE_SECONDS: u64 = 45;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_CHUNK_BYTES: usize = 16 * 1024;
pub const REASSEMBLY_SECONDS: u64 = 30;
pub const MAX_UNSETTLED_EFFECTS: usize = 8;
pub const MAX_QUEUED_COMMANDS: usize = 32;
pub const RESERVED_CONTROL_COMMANDS: usize = 8;
pub const MAX_PROJECTS: usize = 128;
pub const MAX_PROVIDERS: usize = 32;
pub const MAX_MODELS_PER_PROVIDER: usize = 256;
pub const MAX_PAGE_RECORDS: usize = 100;
pub const DEFAULT_DEADLINE_SECONDS: i64 = 30 * 60;
pub const MAX_DEADLINE_SECONDS: i64 = 24 * 60 * 60;
pub const MAX_OUTBOX_BYTES: usize = 64 * 1024 * 1024;

pub fn validate_capacity(
    unsettled: usize,
    queued: usize,
    command: &super::LocalCommand,
) -> Result<(), &'static str> {
    let queue_limit = if command.is_effect() {
        MAX_QUEUED_COMMANDS - RESERVED_CONTROL_COMMANDS
    } else {
        MAX_QUEUED_COMMANDS
    };
    if queued >= queue_limit || (command.is_effect() && unsettled >= MAX_UNSETTLED_EFFECTS) {
        return Err("machine command capacity exhausted");
    }
    Ok(())
}

pub fn validate_deadline(now: DateTime<Utc>, deadline: DateTime<Utc>) -> Result<(), &'static str> {
    let remaining = deadline.signed_duration_since(now);
    if remaining <= chrono::Duration::zero()
        || remaining > chrono::Duration::seconds(MAX_DEADLINE_SECONDS)
    {
        return Err("deadline must be in the next 24 hours");
    }
    Ok(())
}
