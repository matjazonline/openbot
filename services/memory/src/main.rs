//! `memory-service` CLI: `serve` (default), `migrate`, `admin bootstrap [--label L]`,
//! `admin reindex-failed`, `admin after-restore`, `model-check`, `openapi`, `version`.
use memory_service::{
    adapters::{embedding, http::openapi, postgres::PgStore},
    application::{
        ports::{GenerationStore, SearchStore, Store},
        service,
    },
    domain::credential::{self, TokenKind},
    infra::{config::Config, startup, telemetry},
};
use std::process::ExitCode;
use tokio_util::sync::CancellationToken;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("openapi") => {
            print!("{}", openapi::render());
            return ExitCode::SUCCESS;
        }
        Some("version") => {
            println!("memory-service {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("model-check") => {
            // No database and no other configuration: manifest validation, both sessions, the
            // self-check and behavioural probes, reported with the resource budget.
            let directory = std::env::var_os("MEMORY_MODEL_DIR")
                .map_or_else(|| "assets/model".into(), std::path::PathBuf::from);
            return match embedding::model_check(&directory) {
                Ok(report) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).unwrap_or_default()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("model check failed: {error:#}");
                    ExitCode::FAILURE
                }
            };
        }
        _ => {}
    }
    // Structured logging exists before the first fallible step.
    for problem in telemetry::init() {
        tracing::warn!("{problem}");
    }
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "refusing to start");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(config.runtime_thread_stack_bytes)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(%error, "cannot build the runtime");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async move {
        match args.first().map(String::as_str) {
            None | Some("serve") => {
                let shutdown = CancellationToken::new();
                startup::cancel_on_signals(shutdown.clone());
                startup::serve(config, shutdown).await
            }
            Some("migrate") => {
                let pool = startup::connect(&config).await?;
                startup::migrate(&pool).await?;
                tracing::info!("migrations applied");
                Ok(())
            }
            Some("admin") if args.get(1).map(String::as_str) == Some("bootstrap") => {
                let usage = "usage: memory-service admin bootstrap [--label LABEL]";
                let label = match (args.get(2).map(String::as_str), args.get(3), args.len()) {
                    (None, _, _) => "bootstrap operator".to_owned(),
                    (Some("--label"), Some(label), 4) => label.clone(),
                    _ => anyhow::bail!("{usage}"),
                };
                // Refuse a bad label before touching the database, with the reason.
                service::label(&label).map_err(|error| anyhow::anyhow!("{}", error.message))?;
                let pool = startup::connect(&config).await?;
                startup::migrate(&pool).await?;
                let issued = credential::issue(TokenKind::Admin)
                    .map_err(|_| anyhow::anyhow!("no randomness available"))?;
                PgStore::new(pool)
                    .bootstrap_operator(issued.id, &label, issued.verifier)
                    .await
                    .map_err(|error| anyhow::anyhow!("{}", error.message))?;
                // Shown once; only its verifier is stored.
                println!("{}", issued.token);
                Ok(())
            }
            Some("admin") if args.get(1).map(String::as_str) == Some("reindex-failed") => {
                // Requeue observations whose semantic indexing failed terminally (D7).
                let pool = startup::connect(&config).await?;
                startup::migrate(&pool).await?;
                let count = PgStore::new(pool)
                    .reindex_failed()
                    .await
                    .map_err(|error| anyhow::anyhow!("{}", error.message))?;
                println!("requeued {count} observations for indexing");
                Ok(())
            }
            Some("admin") if args.get(1).map(String::as_str) == Some("after-restore") => {
                // On a restored database, before `serve` and with clients kept away: finish
                // every unfinished operation as `cancelled(restored)`, so a restore never
                // replays inference (A4-2).
                let pool = startup::connect(&config).await?;
                startup::migrate(&pool).await?;
                let count = PgStore::new(pool)
                    .after_restore()
                    .await
                    .map_err(|error| anyhow::anyhow!("{}", error.message))?;
                println!("cancelled {count} unfinished generation operations (restored)");
                Ok(())
            }
            Some(other) => anyhow::bail!(
                "unknown command {other:?}; expected serve, migrate, admin bootstrap, \
                 admin reindex-failed, admin after-restore, model-check, openapi or version"
            ),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "memory service failed");
            ExitCode::FAILURE
        }
    }
}
