//! Startup, readiness and graceful shutdown. Tracing is initialized by `main` before any of this.
use super::config::Config;
use crate::{
    adapters::{
        anthropic::{AnthropicProvider, AnthropicSettings},
        embedding, http,
        postgres::{MIGRATOR, PgStore, connect_options, pool_options},
    },
    application::{
        embedding::EmbeddingPool,
        generation_worker::{GenerationSettings, GenerationWorker, Maintenance},
        indexer::{Indexer, IndexerSettings, supervise},
        ports::{GenerationProvider, SearchStore, Store},
        provider_breaker::ProviderBreaker,
        service::App,
    },
};
use anyhow::{Context, bail};
use std::{
    future::IntoFuture,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Connect within the configured deadline. The URL never appears in errors or logs.
pub async fn connect(config: &Config) -> anyhow::Result<sqlx::PgPool> {
    let options =
        connect_options(&config.database_url, &config.pool).map_err(anyhow::Error::msg)?;
    match tokio::time::timeout(
        config.connect_timeout,
        pool_options(&config.pool).connect_with(options),
    )
    .await
    {
        Ok(Ok(pool)) => Ok(pool),
        Ok(Err(error)) => bail!("cannot connect to the memory database: {}", redact(&error)),
        Err(_) => bail!("cannot connect to the memory database within the connect timeout"),
    }
}

fn redact(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(db) => format!("database error {}", db.code().unwrap_or_default()),
        sqlx::Error::Io(io) => format!("I/O error: {}", io.kind()),
        sqlx::Error::PoolTimedOut => "pool timed out".into(),
        sqlx::Error::Tls(_) => "TLS error".into(),
        other => other.to_string().chars().take(200).collect(),
    }
}

pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    MIGRATOR
        .run(pool)
        .await
        .context("the memory database migration failed; a mismatched init migration requires a database reset")
}

/// Load and self-check the pinned embedding model (both pool sessions) off the async threads.
/// A missing or modified artifact fails startup: there is no keyword-only mode (D7).
pub async fn load_embeddings(config: &Config) -> anyhow::Result<Arc<EmbeddingPool>> {
    let directory = config.model_dir.clone();
    let loaded = tokio::task::spawn_blocking(move || embedding::load(&directory))
        .await
        .context("the embedding model loader stopped")?
        .context("the embedding model is unavailable")?;
    tracing::info!(load_ms = loaded.load_ms as u64, "embedding model loaded");
    Ok(Arc::new(loaded.pool()?))
}

/// Refuse a database without the vector capability or indexed with another model.
pub async fn check_retrieval(store: &PgStore, embeddings: &EmbeddingPool) -> anyhow::Result<()> {
    store
        .vector_capability()
        .await
        .map_err(|error| anyhow::anyhow!("{}", error.message))?;
    store
        .adopt_embedding_model(embeddings.model_id())
        .await
        .map_err(|error| anyhow::anyhow!("{}", error.message))?;
    Ok(())
}

/// Serve until `shutdown` is cancelled, then drain for at most the configured grace period.
pub async fn serve(config: Config, shutdown: CancellationToken) -> anyhow::Result<()> {
    let pool = connect(&config).await?;
    migrate(&pool).await?;
    let embeddings = load_embeddings(&config).await?;
    check_retrieval(&PgStore::new(pool.clone()), &embeddings).await?;
    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("cannot bind {}", config.bind))?;
    run(config, pool, embeddings, listener, shutdown).await
}

pub async fn run(
    config: Config,
    pool: sqlx::PgPool,
    embeddings: Arc<EmbeddingPool>,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    // The provider is built from configuration only; startup never contacts it.
    let provider: Option<Arc<dyn GenerationProvider>> = match &config.generation {
        Some(generation) => {
            let mut settings = AnthropicSettings::new(generation.api_key.clone());
            settings.base_url = generation.base_url.clone();
            settings.effort = generation.effort.clone();
            Some(Arc::new(
                AnthropicProvider::new(settings).map_err(anyhow::Error::msg)?,
            ))
        }
        None => None,
    };
    let settings = GenerationSettings {
        workers: config.generation.as_ref().map_or(0, |g| g.workers),
        shutdown_grace: config.shutdown_grace,
        ..GenerationSettings::default()
    };
    run_services(
        config, pool, embeddings, provider, settings, listener, shutdown,
    )
    .await
}

/// Serve with the supervised workers: the indexer, the maintenance task (reaper and
/// retention sweeper) and, with a provider, `settings.workers` generation workers. Each is
/// restarted after a panic; all stop with the server within the one shutdown grace and are
/// aborted after it (an interrupted lease simply expires and its fence guards later commits).
#[allow(clippy::too_many_arguments)]
pub async fn run_services(
    config: Config,
    pool: sqlx::PgPool,
    embeddings: Arc<EmbeddingPool>,
    provider: Option<Arc<dyn GenerationProvider>>,
    settings: GenerationSettings,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let store: Arc<dyn Store> = Arc::new(PgStore::new(pool.clone()));
    let mut app = App::new(store.clone(), embeddings.clone());
    let mut workers = Vec::new();
    let indexer = Indexer::new(store.clone(), embeddings, IndexerSettings::default());
    workers.push(tokio::spawn(supervise(
        "indexer",
        shutdown.clone(),
        Duration::from_secs(1),
        {
            let shutdown = shutdown.clone();
            move || {
                let indexer = indexer.clone();
                let shutdown = shutdown.clone();
                async move { indexer.run(shutdown).await }
            }
        },
    )));
    let maintenance = Maintenance::new(store.clone(), settings.clone());
    workers.push(tokio::spawn(supervise(
        "generation-maintenance",
        shutdown.clone(),
        Duration::from_secs(1),
        {
            let shutdown = shutdown.clone();
            move || {
                let maintenance = maintenance.clone();
                let shutdown = shutdown.clone();
                async move { maintenance.run(shutdown).await }
            }
        },
    )));
    if let Some(provider) = provider {
        // One breaker per process, shared by its workers and read by the metrics export (A4-1).
        let breaker = Arc::new(ProviderBreaker::new(
            settings.breaker_base,
            settings.breaker_max,
        ));
        app = app
            .with_generation(provider.clone())
            .with_breaker(breaker.clone());
        for _ in 0..settings.workers {
            let worker = GenerationWorker::new(store.clone(), provider.clone(), settings.clone())
                .with_breaker(breaker.clone());
            workers.push(tokio::spawn(supervise(
                "generation",
                shutdown.clone(),
                Duration::from_secs(1),
                {
                    let shutdown = shutdown.clone();
                    move || {
                        let worker = worker.clone();
                        let shutdown = shutdown.clone();
                        async move { worker.run(shutdown).await }
                    }
                },
            )));
        }
        tracing::info!(workers = settings.workers, "observation generation enabled");
    } else {
        tracing::info!("observation generation disabled: no MEMORY_ANTHROPIC_API_KEY");
    }
    // One grace for the whole shutdown: the worker joins get what the server drain left.
    let requested = tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown.cancelled().await;
            tokio::time::Instant::now()
        }
    });
    let served = serve_app(app, &config, listener, shutdown.clone()).await;
    shutdown.cancel();
    let since = requested.await.map_or(Duration::ZERO, |at| at.elapsed());
    let deadline = tokio::time::Instant::now() + config.shutdown_grace.saturating_sub(since);
    for mut worker in workers {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if tokio::time::timeout(remaining, &mut worker).await.is_err() {
            tracing::warn!("a worker did not stop within the shutdown grace; aborting it");
            worker.abort();
        }
    }
    pool.close().await;
    served
}

pub async fn serve_app(
    app: App,
    config: &Config,
    listener: TcpListener,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let ready = Arc::new(AtomicBool::new(false));
    let router = http::router(app, ready.clone(), &config.http, shutdown.clone());
    let address = listener.local_addr()?;
    let stopping = {
        let shutdown = shutdown.clone();
        let ready = ready.clone();
        async move {
            shutdown.cancelled().await;
            ready.store(false, Ordering::Release);
        }
    };
    let server = axum::serve(listener, router).with_graceful_shutdown(stopping);
    let mut server = Box::pin(server.into_future());
    ready.store(true, Ordering::Release);
    tracing::info!(%address, "memory service ready");
    tokio::select! {
        result = &mut server => result.context("the HTTP server failed")?,
        () = shutdown.cancelled() => {
            match tokio::time::timeout(config.shutdown_grace, &mut server).await {
                Ok(result) => result.context("the HTTP server failed during shutdown")?,
                Err(_) => tracing::warn!("shutdown grace elapsed with requests still in flight"),
            }
        }
    }
    tracing::info!("memory service stopped");
    Ok(())
}

/// Cancel `token` on SIGINT or SIGTERM.
pub fn cancel_on_signals(token: CancellationToken) {
    tokio::spawn(async move {
        let interrupt = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install the SIGTERM handler");
        #[cfg(unix)]
        tokio::select! {
            _ = interrupt => {},
            _ = terminate.recv() => {},
        }
        #[cfg(not(unix))]
        let _ = interrupt.await;
        tracing::info!("shutdown requested");
        token.cancel();
    });
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
