//! Global and per-company concurrency for memory-service requests (decision E1): interactive work
//! (recall, tools, `memory.load`) never waits behind background work, and no company holds more
//! than its fixed share of any pool. Background work has separate permits for promised deliveries
//! and for lifecycle reconciliation, so a backlog of either never starves the other.
use crate::application::memory::MemoryServiceError;
use crate::infra::memory_config::{PER_COMPANY_BACKGROUND, PER_COMPANY_INTERACTIVE};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryPool {
    Interactive,
    /// Lifecycle reconciliation (admin calls).
    Background,
    /// Promised saves and generation requests (the delivery worker).
    Delivery,
}

pub struct MemoryPools {
    interactive: Arc<Semaphore>,
    background: Arc<Semaphore>,
    delivery: Arc<Semaphore>,
    companies: Mutex<HashMap<(MemoryPool, Uuid), Arc<Semaphore>>>,
}

/// Holds a global and a per-company slot; both are released on drop.
pub struct MemoryPermit {
    _company: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

impl MemoryPools {
    pub fn new(interactive: usize, background: usize) -> Self {
        Self {
            interactive: Arc::new(Semaphore::new(interactive)),
            background: Arc::new(Semaphore::new(background)),
            delivery: Arc::new(Semaphore::new(background)),
            companies: Mutex::new(HashMap::new()),
        }
    }

    /// Waits at most until `deadline`; exhaustion is `Unavailable` (nothing was sent).
    pub async fn acquire(
        &self,
        pool: MemoryPool,
        company: Uuid,
        deadline: Instant,
    ) -> Result<MemoryPermit, MemoryServiceError> {
        let company_slots = self.company_slots(pool, company)?;
        let global = match pool {
            MemoryPool::Interactive => self.interactive.clone(),
            MemoryPool::Background => self.background.clone(),
            MemoryPool::Delivery => self.delivery.clone(),
        };
        // Company first: a company at its share never takes a global slot others could use.
        let company_permit = tokio::time::timeout_at(deadline, company_slots.acquire_owned())
            .await
            .map_err(|_| MemoryServiceError::Unavailable)?
            .map_err(|_| MemoryServiceError::Unavailable)?;
        let global_permit = tokio::time::timeout_at(deadline, global.acquire_owned())
            .await
            .map_err(|_| MemoryServiceError::Unavailable)?
            .map_err(|_| MemoryServiceError::Unavailable)?;
        Ok(MemoryPermit {
            _company: company_permit,
            _global: global_permit,
        })
    }

    /// Company slots of `pool` held right now (test observation of E1's accounting).
    #[cfg(test)]
    pub(crate) fn in_use(&self, pool: MemoryPool, company: Uuid) -> usize {
        let share = match pool {
            MemoryPool::Interactive => PER_COMPANY_INTERACTIVE,
            MemoryPool::Background | MemoryPool::Delivery => PER_COMPANY_BACKGROUND,
        };
        self.companies
            .lock()
            .unwrap()
            .get(&(pool, company))
            .map_or(0, |slots| share - slots.available_permits())
    }

    fn company_slots(
        &self,
        pool: MemoryPool,
        company: Uuid,
    ) -> Result<Arc<Semaphore>, MemoryServiceError> {
        let mut companies = self
            .companies
            .lock()
            .map_err(|_| MemoryServiceError::Unavailable)?;
        // Bounded map: entries nobody holds are dropped before a new one is added.
        companies.retain(|_, slots| Arc::strong_count(slots) > 1);
        let share = match pool {
            MemoryPool::Interactive => PER_COMPANY_INTERACTIVE,
            MemoryPool::Background | MemoryPool::Delivery => PER_COMPANY_BACKGROUND,
        };
        Ok(companies
            .entry((pool, company))
            .or_insert_with(|| Arc::new(Semaphore::new(share)))
            .clone())
    }
}
