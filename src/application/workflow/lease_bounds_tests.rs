//! A raised bound must not let a renewal or the supervision guard outlive a lease window.
use super::*;

fn every_lease() -> impl Iterator<Item = LeasePolicy> {
    (3_000..=60_000)
        .step_by(250)
        .chain([3_001, 5_250, 5_999, 6_001])
        .map(|ms| LeasePolicy::new(Duration::from_millis(ms)).unwrap())
}

#[test]
fn workflow_persistence_bounds_fit_every_lease() {
    for policy in every_lease() {
        let d = policy.duration();
        let (a, attempt, resolution) = (
            policy.acquire_bound(),
            policy.attempt_bound(),
            policy.resolution_bound(),
        );
        let (deadline, guard) = (policy.persistence_deadline(), policy.supervision_guard());
        assert!(
            a < attempt,
            "{d:?}: acquire {a:?} inside attempt {attempt:?}"
        );
        assert_eq!(deadline, attempt + resolution, "{d:?}");
        // A renewal may start immediately after a full-deadline operation and still land.
        assert!(
            deadline <= d / 2,
            "{d:?}: deadline {deadline:?} exceeds half the lease"
        );
        assert!(
            guard > deadline,
            "{d:?}: the adapter must settle before the guard"
        );
        assert!(
            policy.heartbeat() + guard < d,
            "{d:?}: a hung renewal must stop as Persistence before the window expires"
        );
        assert!(attempt <= Duration::from_secs(2) && resolution <= Duration::from_secs(1));
    }
    let short = LeasePolicy::new(Duration::from_secs(3)).unwrap();
    assert_eq!(
        (
            short.attempt_bound(),
            short.persistence_deadline(),
            short.supervision_guard()
        ),
        (
            Duration::from_secs(1),
            Duration::from_millis(1_500),
            Duration::from_millis(1_750)
        )
    );
}
