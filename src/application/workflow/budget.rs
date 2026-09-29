//! Durable accounting before scripted model/repetition work. A receipt is never
//! permission to dispatch: callers must revalidate ownership and apply their
//! effect/result recovery policy, including after a saved receipt is replayed.
use super::lease::{LeasePolicy, WorkflowFence};
use crate::app_error::{AppError, AppResult};
use crate::domain::workflow::{BudgetCharge, BudgetResource};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetReservationKey(String);

impl BudgetReservationKey {
    pub fn parse(value: impl Into<String>) -> AppResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.as_bytes()[0].is_ascii_alphanumeric()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        {
            return Err(AppError::BadRequest(
                "Invalid workflow budget reservation key".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct BudgetReservation {
    pub fence: WorkflowFence,
    pub key: BudgetReservationKey,
    charge: BudgetCharge,
}

impl BudgetReservation {
    pub fn new(
        fence: WorkflowFence,
        key: BudgetReservationKey,
        charge: BudgetCharge,
    ) -> AppResult<Self> {
        if charge.resource() == BudgetResource::Activation {
            return Err(AppError::BadRequest(
                "Activation accounting belongs to the runtime".into(),
            ));
        }
        Ok(Self { fence, key, charge })
    }

    pub fn charge(&self) -> BudgetCharge {
        self.charge
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDisposition {
    Granted,
    Exhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetReservationResult {
    /// Accounting committed; this does not grant effect permission.
    Recorded(BudgetDisposition),
    /// Exact scoped accounting fact only. May be returned after cancellation,
    /// lease loss or completion; the supplied attempt is not reauthorized.
    Replayed(BudgetDisposition),
    NotOwned,
}

#[async_trait]
pub trait WorkflowBudgets: Send + Sync {
    /// Charge once per scoped logical key, independently of attempt identity.
    /// Changed quantity conflicts. Fresh work requires a live fence; exhaustion
    /// atomically retires that execution and cannot be operator-retried.
    async fn reserve_budget(
        &self,
        request: BudgetReservation,
        policy: LeasePolicy,
    ) -> AppResult<BudgetReservationResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_budget_keys_are_bounded_ascii_identities() {
        for valid in ["op.1:a-b_c".into(), "a".repeat(128)] {
            assert!(BudgetReservationKey::parse(valid).is_ok());
        }
        for invalid in [
            "".into(),
            "a".repeat(129),
            "é".into(),
            "_a".into(),
            "a b".into(),
        ] {
            assert!(BudgetReservationKey::parse(invalid).is_err());
        }
    }
}
