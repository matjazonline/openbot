//! A per-database one-shot pause at the actual payload/final-projection boundary.
use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub(crate) struct ReadPause {
    pub fetched: tokio::sync::Notify,
    pub resume: tokio::sync::Notify,
}
static PAUSES: OnceLock<Mutex<HashMap<Uuid, Arc<ReadPause>>>> = OnceLock::new();
pub(crate) fn pause(company: CompanyId) -> Arc<ReadPause> {
    let pause = Arc::new(ReadPause::default());
    assert!(
        PAUSES
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(company.as_uuid(), pause.clone())
            .is_none()
    );
    pause
}
pub(super) async fn after_payload(company: CompanyId) {
    let pause = PAUSES
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .remove(&company.as_uuid());
    if let Some(pause) = pause {
        pause.fetched.notify_one();
        pause.resume.notified().await;
    }
}
