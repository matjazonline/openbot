use std::process::ExitCode;

use attribution_service::{
    adapters::postgres,
    infra::{
        config::{Config, RUNTIME_THREAD_STACK_BYTES},
        startup,
    },
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("attribution_service=info")),
        )
        .init();
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments == ["version"] {
        println!("attribution-service {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let command = match arguments.as_slice() {
        [] => "serve",
        [command] if matches!(command.as_str(), "serve" | "migrate") => command,
        _ => {
            tracing::error!("usage: attribution-service [serve|migrate|version]");
            return ExitCode::from(2);
        }
    };
    tracing::warn!(
        "No backward compatibility: incompatible schemas require an operator-managed database reset"
    );
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "configuration rejected");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(RUNTIME_THREAD_STACK_BYTES)
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            tracing::error!("runtime construction failed");
            return ExitCode::FAILURE;
        }
    };
    let result: anyhow::Result<()> = runtime.block_on(async move {
        if command == "migrate" {
            let pool = postgres::connect(&config).await?;
            postgres::migrate(&pool).await?;
            pool.close().await;
            return Ok(());
        }
        let shutdown = CancellationToken::new();
        let serving = startup::serve(config, shutdown.clone());
        tokio::pin!(serving);
        tokio::select! {
            result = &mut serving => result,
            signal = startup::shutdown_signal() => {
                signal?;
                shutdown.cancel();
                serving.await
            }
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "attribution service stopped with an error");
            ExitCode::FAILURE
        }
    }
}
