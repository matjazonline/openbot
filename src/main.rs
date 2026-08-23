use dotenvy::dotenv;
use std::future::{Future, IntoFuture};
use std::sync::Arc;
use tokio::time::{Duration, sleep};
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
    adapters::smtp::SmtpServer,
    infra::{app::create_app, events::run_mailbox_event_listener, setup::init_app_state},
    services::task_worker::TaskWorker,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let app_state = init_app_state().await?;

    // Create broadcast channel for background worker graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    // Initialize Task Worker poller loop
    let task_worker = Arc::new(
        TaskWorker::new(
            app_state.thread_use_cases.get_task_persistence().await,
            app_state.thread_use_cases.clone(),
            app_state.config.clone(),
        )
        .with_schedules(app_state.schedule_use_cases.clone())
        .with_monitoring(app_state.monitoring.clone()),
    );

    tokio::spawn(task_worker.start_worker_loop(shutdown_rx.resubscribe()));

    // Initialize Incoming SMTP Server loop
    let smtp_server = Arc::new(
        SmtpServer::new(app_state.thread_use_cases.clone(), app_state.config.clone())
            .with_monitoring(app_state.monitoring.clone()),
    );

    tokio::spawn(smtp_server.start_server_loop(shutdown_rx.resubscribe()));

    // Republish committed messages to open mailboxes. It listens on the database rather than
    // in-process because the writer is usually the task worker or the SMTP loop above.
    tokio::spawn(run_mailbox_event_listener(
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
        .with_graceful_shutdown(shutdown_signal(shutdown_tx))
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

    Ok(())
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
}
