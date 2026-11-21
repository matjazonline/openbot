use crate::application::identity::{CompanyStatusHealth, Failure, IdentityService, Result};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinSet, time};
use tokio_util::sync::CancellationToken;

pub async fn run(service: Arc<IdentityService>, shutdown: CancellationToken) {
    let mut interval = time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut health = time::interval(Duration::from_secs(30));
    health.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            _ = health.tick() => {
                tokio::select! { biased; () = shutdown.cancelled() => break, _ = probe(&service) => () }
            }
            _ = interval.tick() => iteration(service.clone(), shutdown.clone()).await,
        }
    }
}
/// Durable operator signal; the browser/API cannot read global company health.
pub async fn probe(service: &IdentityService) -> Result<CompanyStatusHealth> {
    let health = match time::timeout(Duration::from_secs(2), service.tenants.health()).await {
        Ok(Ok(health)) => health,
        _ => {
            tracing::warn!(
                component = "attribution_company_status_health",
                available = false,
                "company status health unavailable"
            );
            return Err(Failure::Unavailable);
        }
    };
    if health.expired || health.reconciliation_failed {
        tracing::warn!(
            component = "attribution_company_status_health",
            available = true,
            expired = health.expired,
            reconciliation_failed = health.reconciliation_failed,
            "company status requires attention"
        );
    } else {
        tracing::info!(
            component = "attribution_company_status_health",
            available = true,
            expired = false,
            reconciliation_failed = false,
            "company status healthy"
        );
    }
    Ok(health)
}
pub async fn iteration(service: Arc<IdentityService>, shutdown: CancellationToken) {
    let claims = match service.tenants.claim_reconciliation().await {
        Ok(claims) => claims,
        Err(_) => {
            tracing::warn!(component = "company_status", "claim unavailable");
            return;
        }
    };
    let mut tasks = JoinSet::new();
    for claim in claims {
        let service = service.clone();
        let shutdown = shutdown.clone();
        tasks.spawn(async move {
            let work = async {
                let result = service.refresh(claim.company_id, Some(&claim)).await;
                // Inactive is a successful stored observation; an authority rejection is an error.
                let success = result.is_ok();
                if service.tenants.finish_reconciliation(&claim, success).await.is_err() {
                    tracing::warn!(company_id = %claim.company_id.0, "reconciliation completion lost ownership or storage");
                }
            };
            tokio::select! { biased; () = shutdown.cancelled() => (), _ = time::timeout(Duration::from_secs(10), work) => () }
        });
    }
    // Dropping this iteration must not detach correctness-critical work.
    while tasks.join_next().await.is_some() {}
    if !shutdown.is_cancelled() && service.tenants.cleanup_sessions().await.is_err() {
        tracing::warn!("session cleanup unavailable");
    }
}
