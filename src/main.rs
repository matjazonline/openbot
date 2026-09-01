use dotenvy::dotenv;
use std::future::{Future, IntoFuture};
use std::sync::Arc;
use tokio::time::{Duration, sleep, timeout};
use tracing::{info, warn};

/// How long to let in-flight requests finish after a stop signal before exiting regardless.
///
/// Comfortably longer than the 15s a mailbox send can hold its request, and comfortably shorter
/// than the platform's own kill timeout, so the process gets to exit on its own terms.
const DRAIN_GRACE: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
enum DrainOutcome {
    Forced,
    DeadlineReached,
}

use mail_agents::{
    adapters::{persistence::PostgresPersistence, smtp::SmtpServer},
    infra::{
        app::create_app,
        config::{
            agent_run_timeout_from_env, runtime_thread_stack_bytes_from_env,
            task_worker_concurrency_from_env,
        },
        events::run_mailbox_event_listener,
        postgres_persistence,
        runtime_metrics::LinuxRuntimeMetricSource,
        setup::{init_app_state, init_tracing},
    },
    services::{
        runtime_metrics::{ActiveTaskExecutions, RuntimeMetricSampler},
        task_worker::TaskWorker,
    },
};

#[derive(Debug, PartialEq, Eq)]
enum ProcessCommand {
    Serve,
    CredentialStatus { required_version: Option<u32> },
    CredentialRotate,
    Help,
}

/// Build the runtime by hand rather than through `#[tokio::main]`, which offers no way to widen
/// the worker stacks -- see `DEFAULT_RUNTIME_THREAD_STACK_BYTES` for why 2 MiB is not enough here.
fn main() -> anyhow::Result<()> {
    dotenv().ok();
    init_tracing();
    match parse_process_command(std::env::args().skip(1))? {
        ProcessCommand::Serve => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(runtime_thread_stack_bytes_from_env())
            .build()?
            .block_on(serve()),
        ProcessCommand::CredentialStatus { required_version } => {
            operational_runtime()?.block_on(run_credential_status(required_version))
        }
        ProcessCommand::CredentialRotate => {
            operational_runtime()?.block_on(run_credential_rotation())
        }
        ProcessCommand::Help => {
            println!("{}", command_usage());
            Ok(())
        }
    }
}

fn operational_runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

fn parse_process_command(args: impl IntoIterator<Item = String>) -> anyhow::Result<ProcessCommand> {
    let args = args.into_iter().collect::<Vec<_>>();
    match args.as_slice() {
        [] => Ok(ProcessCommand::Serve),
        [help] if help == "--help" || help == "-h" => Ok(ProcessCommand::Help),
        [credentials, status] if credentials == "credentials" && status == "status" => {
            Ok(ProcessCommand::CredentialStatus {
                required_version: None,
            })
        }
        [credentials, status, require, version]
            if credentials == "credentials"
                && status == "status"
                && require == "--require-version" =>
        {
            let required_version = parse_required_version(version)?;
            Ok(ProcessCommand::CredentialStatus {
                required_version: Some(required_version),
            })
        }
        [credentials, rotate] if credentials == "credentials" && rotate == "rotate" => {
            Ok(ProcessCommand::CredentialRotate)
        }
        _ => anyhow::bail!("Invalid command.\n\n{}", command_usage()),
    }
}

fn parse_required_version(value: &str) -> anyhow::Result<u32> {
    let version = value
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("--require-version must be a positive integer"))?;
    if version == 0 {
        anyhow::bail!("--require-version must be a positive integer");
    }
    Ok(version)
}

fn command_usage() -> &'static str {
    "Usage:\n  mail_agents\n  mail_agents credentials status [--require-version <N>]\n  mail_agents credentials rotate"
}

async fn run_credential_status(required_version: Option<u32>) -> anyhow::Result<()> {
    let persistence = postgres_persistence().await?;
    let status = persistence.credential_status().await?;
    println!("{}", serde_json::to_string(&status)?);
    ensure_credential_status(&status, required_version)
}

fn ensure_credential_status(
    status: &mail_agents::adapters::persistence::credential_rotation::CredentialStatus,
    required_version: Option<u32>,
) -> anyhow::Result<()> {
    if !status.is_valid() {
        anyhow::bail!("Credential status found malformed or unavailable rows");
    }
    if let Some(required_version) = required_version
        && !status.satisfies_required_version(required_version)
    {
        anyhow::bail!("Credential rows have not converged on required version {required_version}");
    }
    Ok(())
}

async fn run_credential_rotation() -> anyhow::Result<()> {
    let persistence: PostgresPersistence = postgres_persistence().await?;
    let report = persistence.rotate_credentials().await?;
    println!("{}", serde_json::to_string(&report)?);
    if !report.complete {
        anyhow::bail!("Credential rotation did not converge");
    }
    Ok(())
}

async fn serve() -> anyhow::Result<()> {
    let task_worker_concurrency = task_worker_concurrency_from_env();
    let app_state = init_app_state().await?;
    let memory_worker = app_state.memory_worker.clone();

    // Create broadcast channel for background worker graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    let active_task_executions = ActiveTaskExecutions::default();
    let runtime_source = LinuxRuntimeMetricSource::new(
        app_state.runtime_identity.clone(),
        active_task_executions.clone(),
        task_worker_concurrency,
        app_state.memory_provider_activity.clone(),
    );
    let runtime_sampler =
        RuntimeMetricSampler::new(app_state.runtime_metrics.clone(), runtime_source);
    let mut runtime_sampler_handle = tokio::spawn(runtime_sampler.run(shutdown_rx.resubscribe()));

    // Initialize Task Worker poller loop
    let task_worker = Arc::new(
        TaskWorker::new(
            app_state.thread_use_cases.get_task_persistence().await,
            app_state.thread_use_cases.clone(),
            app_state.config.clone(),
        )
        .with_task_concurrency(task_worker_concurrency)
        .with_agent_run_timeout(agent_run_timeout_from_env())
        .with_active_task_executions(active_task_executions)
        .with_schedules(app_state.schedule_use_cases.clone())
        .with_monitoring(app_state.monitoring.clone()),
    );

    let task_worker_handle = tokio::spawn(task_worker.start_worker_loop(shutdown_rx.resubscribe()));
    let mut memory_worker_handle = tokio::spawn(memory_worker.run(shutdown_rx.resubscribe()));

    // Initialize Incoming SMTP Server loop
    let smtp_server = Arc::new(
        SmtpServer::new(app_state.thread_use_cases.clone(), app_state.config.clone())
            .with_monitoring(app_state.monitoring.clone()),
    );

    let smtp_handle = tokio::spawn(smtp_server.start_server_loop(shutdown_rx.resubscribe()));

    // Republish committed messages to open mailboxes. It listens on the database rather than
    // in-process because the writer is usually the task worker or the SMTP loop above.
    let mailbox_listener_handle = tokio::spawn(run_mailbox_event_listener(
        app_state.db.clone(),
        app_state.events.clone(),
        shutdown_rx.resubscribe(),
    ));

    let app = create_app(app_state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();

    info!("Backend listening at {}", &listener.local_addr().unwrap());

    // Serve axum app with graceful shutdown listener
    let mut draining = shutdown_tx.subscribe();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_tx.clone()))
        .into_future();
    tokio::pin!(server);

    tokio::select! {
        result = &mut server => result.unwrap(),
        // The mailbox holds SSE streams open for as long as a tab is, and a graceful shutdown waits
        // for every connection to close — so without a deadline one open tab pins the process until
        // the platform loses patience and kills it mid-request. Cutting a stream is harmless: the
        // client reconnects and resumes from its last event id.
        outcome = wait_for_drain_end(&mut draining, second_interrupt(), DRAIN_GRACE) => {
            match outcome {
                DrainOutcome::Forced => {
                    warn!("Second interrupt received; exiting without remaining connections.");
                }
                DrainOutcome::DeadlineReached => {
                    warn!(
                        "Connections still open {}s into the drain; exiting without them.",
                        DRAIN_GRACE.as_secs()
                    );
                }
            }
        }
    }

    // Also covers an HTTP server that ended without a signal: every background owner receives the
    // same stop notification, and the sampler is explicitly joined rather than detached.
    let _ = shutdown_tx.send(());
    let mut task_worker_handle = task_worker_handle;
    let mut smtp_handle = smtp_handle;
    let mut mailbox_listener_handle = mailbox_listener_handle;
    tokio::join!(
        join_background("runtime metric sampler", &mut runtime_sampler_handle),
        join_background("memory worker", &mut memory_worker_handle),
        join_background("task worker", &mut task_worker_handle),
        join_background("SMTP listener", &mut smtp_handle),
        join_background("mailbox listener", &mut mailbox_listener_handle),
    );

    Ok(())
}

async fn join_background(name: &str, handle: &mut tokio::task::JoinHandle<()>) {
    match timeout(DRAIN_GRACE, &mut *handle).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warn!(%error, component = name, "Background task ended unexpectedly"),
        Err(_) => {
            warn!(
                component = name,
                "Background task did not stop within drain grace; aborting it"
            );
            handle.abort();
            let _ = handle.await;
        }
    }
}

/// Waits until graceful shutdown has begun, then bounds how long connections may keep draining.
async fn wait_for_drain_end<F>(
    draining: &mut tokio::sync::broadcast::Receiver<()>,
    force_shutdown: F,
    grace: Duration,
) -> DrainOutcome
where
    F: Future<Output = ()>,
{
    let _ = draining.recv().await;

    tokio::select! {
        _ = force_shutdown => DrainOutcome::Forced,
        _ = sleep(grace) => DrainOutcome::DeadlineReached,
    }
}

async fn second_interrupt() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for a second ctrl+c event");
}

/// Resolves on whichever stop signal arrives first, then tells the background loops to wind down.
///
/// SIGTERM matters as much as ^C here: it is what `docker stop`, Kubernetes, and a bare `kill`
/// send, and without a handler the process dies where it stands — cutting off whatever agent run
/// an HTTP request was holding open.
async fn shutdown_signal(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl+c event");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }

    info!("Received termination signal. Triggering graceful shutdown...");
    let _ = shutdown_tx.send(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::pending;
    use tokio::time::Instant;

    #[test]
    fn operational_commands_are_parsed_before_server_startup() {
        assert_eq!(
            parse_process_command(["credentials".into(), "status".into()]).unwrap(),
            ProcessCommand::CredentialStatus {
                required_version: None
            }
        );
        assert_eq!(
            parse_process_command([
                "credentials".into(),
                "status".into(),
                "--require-version".into(),
                "2".into(),
            ])
            .unwrap(),
            ProcessCommand::CredentialStatus {
                required_version: Some(2)
            }
        );
        assert_eq!(
            parse_process_command(["credentials".into(), "rotate".into()]).unwrap(),
            ProcessCommand::CredentialRotate
        );
    }

    #[test]
    fn operational_command_rejects_zero_and_unknown_arguments() {
        assert!(
            parse_process_command([
                "credentials".into(),
                "status".into(),
                "--require-version".into(),
                "0".into(),
            ])
            .is_err()
        );
        assert!(parse_process_command(["credentials".into(), "unknown".into()]).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn drain_deadline_expires_when_no_second_interrupt_arrives() {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
        shutdown_tx.send(()).unwrap();
        let started = Instant::now();

        let outcome = wait_for_drain_end(&mut shutdown_rx, pending(), DRAIN_GRACE).await;

        assert_eq!(outcome, DrainOutcome::DeadlineReached);
        assert_eq!(started.elapsed(), DRAIN_GRACE);
    }

    #[tokio::test(start_paused = true)]
    async fn second_interrupt_ends_the_drain_immediately() {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel(1);
        shutdown_tx.send(()).unwrap();
        let started = Instant::now();

        let outcome =
            wait_for_drain_end(&mut shutdown_rx, std::future::ready(()), DRAIN_GRACE).await;

        assert_eq!(outcome, DrainOutcome::Forced);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn a_background_owner_that_ignores_shutdown_is_aborted_inside_the_drain_grace() {
        let mut handle = tokio::spawn(std::future::pending::<()>());
        let started = Instant::now();

        join_background("test owner", &mut handle).await;

        assert!(handle.is_finished());
        assert_eq!(started.elapsed(), DRAIN_GRACE);
    }
}
