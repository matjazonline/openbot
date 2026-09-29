//! Database-wide I/O capacity, installed once before workers begin claiming.
use crate::application::app_error::{AppError, AppResult};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowCapacityPolicy {
    global: u16,
    company: u16,
}

impl WorkflowCapacityPolicy {
    pub fn new(global: u16, company: u16) -> AppResult<Self> {
        // Leave capacity for another company even when one tenant is saturated.
        if !(2..=1024).contains(&global) || company == 0 || company >= global {
            return Err(AppError::BadRequest(
                "Invalid workflow capacity policy".into(),
            ));
        }
        Ok(Self { global, company })
    }

    pub fn global(self) -> u16 {
        self.global
    }

    pub fn company(self) -> u16 {
        self.company
    }
}

impl Default for WorkflowCapacityPolicy {
    fn default() -> Self {
        Self {
            global: 16,
            company: 2,
        }
    }
}

#[async_trait]
pub trait WorkflowCapacity: Send + Sync {
    /// Install once, or verify the exact already-installed policy. Claims install
    /// the default if configuration was omitted. A mismatch never changes policy.
    async fn configure_capacity(&self, policy: WorkflowCapacityPolicy) -> AppResult<()>;
}
