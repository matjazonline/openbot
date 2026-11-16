use std::{
    future::IntoFuture,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::{config::Config, connection::BoundedListener};
use crate::adapters::{http, postgres};

pub async fn serve(config: Config, shutdown: CancellationToken) -> Result<()> {
    let identity_config = Arc::new(super::identity::IdentityConfig::from_env(
        &config.public_origin,
    )?);
    let pool = postgres::connect(&config).await?;
    postgres::migrate(&pool).await?;
    let listener = TcpListener::bind(config.bind)
        .await
        .context("listener bind failed")?;
    let identity = super::identity::compose(pool.clone(), identity_config)?;
    run(&config, pool, listener, shutdown, identity).await
}

pub async fn run(
    config: &Config,
    pool: PgPool,
    listener: TcpListener,
    shutdown: CancellationToken,
    identity: crate::adapters::identity::http::IdentityHttp,
) -> Result<()> {
    let address = listener.local_addr()?;
    let setup =
        crate::adapters::setup::http::SetupHttp::production(pool.clone(), identity.clone())?;
    let accepting = Arc::new(AtomicBool::new(true));
    let forced = CancellationToken::new();
    let worker_shutdown = CancellationToken::new();
    let commerce = crate::adapters::commerce::http::CommerceHttp {
        setup: setup.clone(),
        policy: config.commerce,
    };
    let mut workers = tokio::task::JoinSet::new();
    workers.spawn(crate::adapters::identity::worker::run(
        identity.service.clone(),
        worker_shutdown.clone(),
    ));
    workers.spawn(crate::adapters::commerce::worker::run(
        commerce.clone(),
        worker_shutdown.clone(),
    ));
    workers.spawn(crate::adapters::retention::run(
        pool.clone(),
        config.retention,
        identity.service.clone(),
        worker_shutdown.clone(),
    ));
    let state = http::HttpState::new(
        pool.clone(),
        accepting.clone(),
        config.public_origin.scheme() == "https",
    )
    .with_identity(identity)
    .with_setup(setup)
    .with_commerce(commerce);
    let stopping = {
        let shutdown = shutdown.clone();
        let accepting = accepting.clone();
        async move {
            shutdown.cancelled().await;
            accepting.store(false, Ordering::Release);
        }
    };
    let server = axum::serve(
        BoundedListener::new(listener, forced.clone()),
        http::router(state),
    )
    .with_graceful_shutdown(stopping)
    .into_future();
    tokio::pin!(server);
    tracing::info!(%address, "attribution listener ready");
    let result = async { tokio::select! {
        _ = workers.join_next() => Err(anyhow::anyhow!("background worker stopped unexpectedly")),
        result = &mut server => result.context("HTTP server failed"),
        () = shutdown.cancelled() => {
            accepting.store(false, Ordering::Release);
            match tokio::time::timeout(Duration::from_secs(5), &mut server).await {
                Ok(result) => result.context("HTTP shutdown failed"),
                Err(_) => {
                    tracing::warn!(component = "http", "drain budget expired; closing remaining connections");
                    forced.cancel();
                    tokio::time::timeout(Duration::from_secs(1), &mut server).await
                        .context("HTTP connections failed to stop after forced shutdown")?
                        .context("HTTP forced shutdown failed")
                }
            }
        }
    } }.await;
    forced.cancel();
    accepting.store(false, Ordering::Release);
    worker_shutdown.cancel();
    if tokio::time::timeout(Duration::from_secs(5), async {
        while workers.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tracing::warn!(component = "reconciliation", "shutdown deadline exceeded");
        workers.abort_all();
        while workers.join_next().await.is_some() {}
    }
    tokio::time::timeout(Duration::from_secs(2), pool.close())
        .await
        .context("database pool failed to close")?;
    result
}

pub async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}
