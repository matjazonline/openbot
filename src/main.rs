use dotenvy::dotenv;
use std::sync::Arc;
use tracing::info;

use mail_agents::{
    adapters::smtp::SmtpServer,
    infra::{app::create_app, setup::init_app_state},
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
        .with_monitoring(app_state.monitoring.clone()),
    );

    tokio::spawn(task_worker.start_worker_loop(shutdown_rx.resubscribe()));

    // Initialize Incoming SMTP Server loop
    let smtp_server = Arc::new(
        SmtpServer::new(app_state.thread_use_cases.clone(), app_state.config.clone())
            .with_monitoring(app_state.monitoring.clone()),
    );

    tokio::spawn(smtp_server.start_server_loop(shutdown_rx.resubscribe()));

    let app = create_app(app_state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();

    info!("Backend listening at {}", &listener.local_addr().unwrap());

    // Serve axum app with graceful shutdown listener
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl+c event");
            info!("Received termination signal. Triggering graceful shutdown...");
            let _ = shutdown_tx.send(());
        })
        .await
        .unwrap();

    Ok(())
}
