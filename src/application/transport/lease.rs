//! Ownership of one durable row, for the workers that claim them.
//!
//! Every queue in this system -- the inbound inbox, the delivery queue -- has the same shape: a
//! claimant takes a row, mints a fresh execution id, and every later write it makes is fenced on
//! that id so a superseded run cannot report a result. The types live here, with the workers that
//! consume them, rather than in the SQL adapter that happens to implement the claim.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::entities::transport::uuid_id;

uuid_id!(ExecutionId);
uuid_id!(WorkerId);

/// A live claim on one row.
///
/// Generic in the row's id type on purpose. `TaskLeaseRef` -- three bare `Uuid` fields called
/// `task_id`, `worker_id` and `execution_generation` -- is the shape `src/AGENTS.md` warns about:
/// nothing stops a caller passing the worker id where the row id belongs, and the compiler cannot
/// tell a lease on a delivery from a lease on an inbound event. Here both mistakes are type
/// errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionLease<Row> {
    /// The row this claim is over.
    pub row: Row,
    /// The execution this claim minted. Every fenced write names it in its `WHERE` clause.
    pub execution: ExecutionId,
    /// Which worker process holds it, for diagnosis and for reaping.
    pub owner: WorkerId,
    /// When the claim lapses if it is not renewed. A reaper may only touch an expired row, and
    /// even then must guard against a replacement execution having taken over.
    pub expires_at: DateTime<Utc>,
}

impl<Row: Copy> ExecutionLease<Row> {
    pub fn new(row: Row, owner: WorkerId, expires_at: DateTime<Utc>) -> Self {
        Self {
            row,
            execution: ExecutionId::new(Uuid::new_v4()),
            owner,
            expires_at,
        }
    }

    /// Whether this claim is still live at `now`.
    ///
    /// Read rather than assumed: a worker that has been descheduled, or that spent a long time in
    /// a provider call, may hold a lease that lapsed while it was working.
    pub fn is_live_at(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now
    }

    /// The same claim with a later deadline. Renewal never changes the execution id -- a new id
    /// means a new claimant, which is the whole fence.
    pub fn renewed_until(self, expires_at: DateTime<Utc>) -> Self {
        Self { expires_at, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::transport::{DeliveryId, InboundEventId};

    #[test]
    fn two_claims_on_the_same_row_are_distinguished_by_their_execution() {
        let row = DeliveryId::random();
        let owner = WorkerId::random();
        let deadline = Utc::now() + chrono::Duration::seconds(60);

        let first = ExecutionLease::new(row, owner, deadline);
        let second = ExecutionLease::new(row, owner, deadline);
        assert_ne!(first.execution, second.execution);
        // Renewal is the same claim, deliberately: only a new claimant mints a new execution.
        assert_eq!(
            first
                .renewed_until(deadline + chrono::Duration::seconds(60))
                .execution,
            first.execution
        );
    }

    #[test]
    fn a_lapsed_claim_reports_itself_as_lapsed() {
        let now = Utc::now();
        let lease = ExecutionLease::new(
            InboundEventId::random(),
            WorkerId::random(),
            now - chrono::Duration::seconds(1),
        );
        assert!(!lease.is_live_at(now));
    }
}
