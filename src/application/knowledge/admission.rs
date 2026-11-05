//! Bounded in-process admission before multipart bodies or model work.
use super::KnowledgeScope;
use crate::domain::workflow::CompanyId;
use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
const MAX_ENTRIES: usize = 4096;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum UploadAdmissionRefused {
    #[error("Too many uploads; wait a minute before trying again")]
    RateLimited,
    #[error("Knowledge uploads are busy; retry shortly")]
    Busy,
}
struct Window {
    since: Instant,
    used: u32,
}
#[derive(Default)]
struct Rates {
    actors: HashMap<uuid::Uuid, Window>,
    companies: HashMap<CompanyId, Window>,
}
pub(super) struct Admission {
    slots: Arc<Semaphore>,
    rates: Mutex<Rates>,
}
impl Admission {
    pub(super) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(4)),
            rates: Mutex::new(Rates::default()),
        }
    }
    pub(super) fn close(&self) {
        self.slots.close();
    }
    pub(super) fn body(
        &self,
        scope: KnowledgeScope,
    ) -> Result<OwnedSemaphorePermit, UploadAdmissionRefused> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| UploadAdmissionRefused::Busy)?;
        let mut rates = self
            .rates
            .lock()
            .map_err(|_| UploadAdmissionRefused::Busy)?;
        let now = Instant::now();
        take(&mut rates.actors, scope.actor_id, 4, now)?;
        take(&mut rates.companies, scope.company, 8, now)?;
        Ok(permit)
    }
}
fn take<K: Eq + std::hash::Hash>(
    rates: &mut HashMap<K, Window>,
    key: K,
    limit: u32,
    now: Instant,
) -> Result<(), UploadAdmissionRefused> {
    rates.retain(|_, window| now.duration_since(window.since) < WINDOW);
    if !rates.contains_key(&key) && rates.len() == MAX_ENTRIES {
        return Err(UploadAdmissionRefused::Busy);
    }
    let window = rates.entry(key).or_insert(Window {
        since: now,
        used: 0,
    });
    if window.used == limit {
        return Err(UploadAdmissionRefused::RateLimited);
    }
    window.used += 1;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    fn scope(company: CompanyId) -> KnowledgeScope {
        KnowledgeScope {
            company,
            actor_id: Uuid::new_v4(),
        }
    }
    #[tokio::test(start_paused = true)]
    async fn knowledge_upload_admission_has_reader_actor_company_and_cache_bounds() {
        let admission = Admission::new();
        let company = CompanyId::new(Uuid::new_v4());
        let actor = scope(company);
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(admission.body(actor).unwrap());
        }
        assert!(matches!(
            admission.body(scope(CompanyId::new(Uuid::new_v4()))),
            Err(UploadAdmissionRefused::Busy)
        ));
        held.clear();
        assert!(matches!(
            admission.body(KnowledgeScope {
                company: CompanyId::new(Uuid::new_v4()),
                ..actor
            }),
            Err(UploadAdmissionRefused::RateLimited)
        ));
        assert!(matches!(
            admission.body(actor),
            Err(UploadAdmissionRefused::RateLimited)
        ));
        for _ in 0..4 {
            drop(admission.body(scope(company)).unwrap());
        }
        assert!(matches!(
            admission.body(scope(company)),
            Err(UploadAdmissionRefused::RateLimited)
        ));
        tokio::time::advance(WINDOW).await;
        drop(admission.body(actor).unwrap());
        admission.close();
        assert!(matches!(
            admission.body(actor),
            Err(UploadAdmissionRefused::Busy)
        ));
        let now = Instant::now();
        let mut entries = HashMap::new();
        for key in 0..MAX_ENTRIES {
            take(&mut entries, key, 1, now).unwrap();
        }
        assert!(matches!(
            take(&mut entries, MAX_ENTRIES, 1, now),
            Err(UploadAdmissionRefused::Busy)
        ));
    }
}
