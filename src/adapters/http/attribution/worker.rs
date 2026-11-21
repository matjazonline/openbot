use super::Bridge;
use crate::application::attribution::{
    DeliveryResult, IdentityError, IdentityResult, LifecycleHealth,
};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinSet, time};
use tokio_util::sync::CancellationToken;

pub async fn run(bridge: Arc<Bridge>, shutdown: CancellationToken) {
    let mut interval = time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut health = time::interval(Duration::from_secs(30));
    health.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            _ = health.tick() => {
                tokio::select! { biased; () = shutdown.cancelled() => break, _ = probe(&bridge) => () }
            }
            _ = interval.tick() => iteration(bridge.clone(), shutdown.clone()).await,
        }
    }
}
/// Operator log signal only: no tenant route exposes global lifecycle state.
pub async fn probe(bridge: &Bridge) -> IdentityResult<LifecycleHealth> {
    let health = match time::timeout(Duration::from_secs(2), bridge.outbox.health()).await {
        Ok(Ok(health)) => health,
        _ => {
            tracing::warn!(
                component = "attribution_lifecycle_health",
                available = false,
                "lifecycle health unavailable"
            );
            return Err(IdentityError::Unavailable);
        }
    };
    if health.undelivered || health.failed {
        tracing::warn!(
            component = "attribution_lifecycle_health",
            available = true,
            undelivered = health.undelivered,
            failed = health.failed,
            "lifecycle delivery requires attention"
        );
    } else {
        tracing::info!(
            component = "attribution_lifecycle_health",
            available = true,
            undelivered = false,
            failed = false,
            "lifecycle delivery healthy"
        );
    }
    Ok(health)
}
pub async fn iteration(bridge: Arc<Bridge>, shutdown: CancellationToken) {
    let claims = match bridge.outbox.claim().await {
        Ok(claims) => claims,
        Err(_) => {
            tracing::warn!(component = "attribution_lifecycle", "claim unavailable");
            return;
        }
    };
    let mut tasks = JoinSet::new();
    for claim in claims {
        let bridge = bridge.clone();
        let shutdown = shutdown.clone();
        tasks.spawn(async move {
            let work=async {
                let result=bridge.delivery.post(&bridge.config.delivery).bearer_auth(&bridge.config.lifecycle_secret).json(&claim.payload).send().await;
                let delivery=match result {
                    Ok(response) if response.status()==reqwest::StatusCode::OK => DeliveryResult::Delivered,
                    Ok(response) if response.status().is_client_error() => DeliveryResult::Rejected,
                    _=>DeliveryResult::Unavailable,
                };
                if bridge.outbox.complete(&claim,delivery).await.is_err() {tracing::warn!(notification_id=%claim.id,"attribution delivery completion lost ownership or storage");}
            };
            tokio::select! {biased; ()=shutdown.cancelled()=>(), _=time::timeout(Duration::from_secs(10),work)=>()}
        });
    }
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            tracing::error!("attribution lifecycle task failed");
        }
    }
    if !shutdown.is_cancelled() && bridge.outbox.cleanup().await.is_err() {
        tracing::warn!("attribution identity cleanup unavailable");
    }
}
