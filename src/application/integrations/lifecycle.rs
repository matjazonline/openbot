//! Bounded lifecycle recovery, owned by the host's supervised worker group.
use crate::domain::integrations::IntegrationResult;
use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
#[async_trait]
pub(crate) trait TeardownDrain: Send + Sync {
    async fn drain(&self, limit: u16, stop: &CancellationToken) -> IntegrationResult<usize>;
}
pub(crate) async fn run(drain: &dyn TeardownDrain, stop: &CancellationToken) {
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {biased; _=stop.cancelled()=>return,_=interval.tick()=>{}}
        if drain.drain(16, stop).await.is_err() {
            tracing::warn!("Integration teardown retry pending");
            tokio::select! {biased; _=stop.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
        }
    }
}
