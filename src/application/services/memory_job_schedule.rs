use chrono::{DateTime, Utc};
use uuid::Uuid;

// Polling stays bounded while jitter prevents workers on multiple instances from synchronizing.
const MIN_POLL_SECONDS: i64 = 2;
const MAX_POLL_SECONDS: i64 = 5;

pub(super) fn retry_at(failure_attempts: i32) -> DateTime<Utc> {
    let base_seconds = (5_i64 * 2_i64.pow(failure_attempts.clamp(0, 6) as u32)).min(300);
    let seconds = jittered_seconds(base_seconds, 80, 120).clamp(4, 300);
    Utc::now() + chrono::Duration::seconds(seconds)
}

pub(super) fn next_poll_at() -> DateTime<Utc> {
    Utc::now()
        + chrono::Duration::seconds(jittered_seconds(
            MIN_POLL_SECONDS,
            100,
            MAX_POLL_SECONDS * 100 / MIN_POLL_SECONDS,
        ))
}

fn jittered_seconds(base: i64, minimum_percent: i64, maximum_percent: i64) -> i64 {
    let bytes = Uuid::new_v4().into_bytes();
    let sample = u64::from_le_bytes(bytes[..8].try_into().expect("UUID prefix is eight bytes"));
    let percent =
        minimum_percent + (sample % (maximum_percent - minimum_percent + 1) as u64) as i64;
    (base * percent / 100).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_and_poll_schedules_are_bounded_and_jittered() {
        let now = Utc::now();
        assert!(retry_at(99) <= now + chrono::Duration::seconds(301));
        let poll = next_poll_at();
        assert!(poll >= now + chrono::Duration::seconds(MIN_POLL_SECONDS));
        assert!(poll <= now + chrono::Duration::seconds(MAX_POLL_SECONDS + 1));
    }
}
