//! Outbound-only peer transport. TaskTracker owns and joins accepted upgrades and the expiry loop.
mod driver;
mod framing;

use crate::{
    app_error::{AppError, AppResult},
    application::local_agents::{ConnectionFence, MachineCredential, bounds::*, protocol::*},
    domain::local_agents::InstallationId,
};
use axum::{
    Router,
    extract::{FromRef, State, WebSocketUpgrade, ws::WebSocket},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures::SinkExt;
use std::sync::{Arc, Mutex};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore, oneshot},
    time::{Duration, Instant, timeout, timeout_at},
};
use tokio_util::{
    sync::CancellationToken,
    task::{TaskTracker, task_tracker::TaskTrackerToken},
};
use uuid::Uuid;

pub struct LocalMachineConnections {
    store: Arc<dyn LocalProtocolStore>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    slots: Arc<Semaphore>,
    closing: Mutex<bool>,
    #[cfg(test)]
    command_write_gate: Option<Arc<tests::CommandWriteGate>>,
}

impl LocalMachineConnections {
    pub fn new(store: Arc<dyn LocalProtocolStore>) -> Self {
        let service = Self {
            store,
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
            slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            closing: Mutex::new(false),
            #[cfg(test)]
            command_write_gate: None,
        };
        let stop = service.shutdown.clone();
        let store = service.store.clone();
        // The tracker retains this task through shutdown; it does not capture the service Arc.
        service.tasks.spawn(async move {
            let mut tick=tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {biased;
                    _=stop.cancelled()=>break,
                    _=tick.tick()=>{
                        let result=tokio::select! {biased; _=stop.cancelled()=>break, result=store.expire_operations(100)=>result};
                        if result.is_err() {tracing::warn!("local operation expiry check failed");}
                    }
                }
            }
        });
        service
    }

    fn reserve(&self) -> Option<(OwnedSemaphorePermit, TaskTrackerToken)> {
        // Reservation and close are serialized before any await, including an upgrade not polled yet.
        let closing = self.closing.lock().unwrap();
        if *closing {
            return None;
        }
        let permit = self.slots.clone().try_acquire_owned().ok()?;
        Some((permit, self.tasks.token()))
    }

    pub async fn shutdown(&self) -> AppResult<()> {
        {
            let mut closing = self.closing.lock().unwrap();
            *closing = true;
            self.slots.close();
            self.shutdown.cancel();
            self.tasks.close();
        }
        timeout(Duration::from_secs(10), self.tasks.wait())
            .await
            .map_err(|_| AppError::Internal("local connections exceeded shutdown drain".into()))?;
        Ok(())
    }
}

impl Drop for LocalMachineConnections {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    Arc<LocalMachineConnections>: FromRef<S>,
{
    Router::new().route("/v1/local-machines/connect", get(connect))
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> AppResult<&'a str> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(AppError::InvalidCredentials);
    }
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::InvalidCredentials)
}

async fn connect(
    State(service): State<Arc<LocalMachineConnections>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> AppResult<Response> {
    let deadline = Instant::now() + Duration::from_secs(HANDSHAKE_SECONDS);
    if one_header(&headers, "x-openbot-protocol-version")? != PROTOCOL_VERSION.to_string() {
        return Err(AppError::BadRequest(
            "unsupported local-machine protocol".into(),
        ));
    }
    let installation =
        InstallationId::try_from(one_header(&headers, "x-openbot-installation-id")?.to_owned())
            .map_err(|_| AppError::InvalidCredentials)?;
    let authorization = one_header(&headers, "authorization")?;
    let secret = authorization
        .strip_prefix("Bearer ")
        .filter(|v| v.len() == 64)
        .ok_or(AppError::InvalidCredentials)?;
    let credential =
        MachineCredential::try_from(secret.to_owned()).map_err(|_| AppError::InvalidCredentials)?;
    let Some((permit, reservation)) = service.reserve() else {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            "local connection capacity exhausted",
        )
            .into_response());
    };
    let fence = tokio::select! {biased;
        _=service.shutdown.cancelled()=>return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response()),
        result=timeout_at(deadline,service.store.acquire_connection(credential.verifier(),installation,Uuid::new_v4()))=>
            result.map_err(|_|AppError::Internal("local machine authentication timed out".into()))??.ok_or(AppError::InvalidCredentials)?,
    };
    let (sender, receiver) = oneshot::channel::<WebSocket>();
    let store = service.store.clone();
    let stop = service.shutdown.clone();
    #[cfg(test)]
    let command_write_gate = service.command_write_gate.clone();
    let task = service.tasks.spawn(async move {
        let _permit = permit;
        let _reservation = reservation;
        connection_task(
            store,
            stop,
            fence,
            deadline,
            receiver,
            #[cfg(test)]
            command_write_gate,
        )
        .await;
    });
    Ok(upgrade
        .max_frame_size(MAX_FRAME_BYTES)
        .max_message_size(MAX_FRAME_BYTES)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_FRAME_BYTES * 2)
        .on_upgrade(move |socket| async move {
            let _ = sender.send(socket);
            let _ = task.await;
        }))
}

async fn connection_task(
    store: Arc<dyn LocalProtocolStore>,
    stop: CancellationToken,
    fence: ConnectionFence,
    deadline: Instant,
    receiver: oneshot::Receiver<WebSocket>,
    #[cfg(test)] command_write_gate: Option<Arc<tests::CommandWriteGate>>,
) {
    let socket = tokio::select! {biased; _=stop.cancelled()=>None,
    result=timeout_at(deadline,receiver)=>result.ok().and_then(Result::ok),};
    if let Some(mut socket) = socket {
        let initialized = tokio::select! {biased; _=stop.cancelled()=>Ok(false),
        result=timeout_at(deadline,driver::initialize(&mut socket,store.as_ref(),&stop,fence))=>
            result.unwrap_or_else(|_|Err(AppError::BadRequest("local machine handshake timed out".into()))),};
        match initialized {
            Ok(true) => {
                let result = tokio::select! {biased; _=stop.cancelled()=>Ok(()), result=driver::run(&mut socket,store.as_ref(),&stop,fence,
                #[cfg(test)] command_write_gate.as_deref())=>result};
                if result.is_err() {
                    record_issue(store.as_ref(), fence, ConnectionIssue::ConnectionFailed).await;
                    tracing::debug!(machine_id=%fence.machine.machine_id,"local machine connection ended");
                }
            }
            Ok(false) => {}
            Err(error) => {
                record_issue(store.as_ref(), fence, ConnectionIssue::ConnectionFailed).await;
                let kind = match error {
                    AppError::InvalidCredentials => "authentication",
                    AppError::BadRequest(_) => "protocol",
                    _ => "storage",
                };
                tracing::debug!(machine_id=%fence.machine.machine_id,error_kind=kind,"local machine handshake failed");
            }
        }
        let _ = timeout(Duration::from_secs(1), socket.close()).await;
    }
    if !matches!(
        timeout(
            Duration::from_secs(IO_SECONDS),
            store.release_connection(fence),
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!(machine_id=%fence.machine.machine_id,"local connection lease cleanup failed");
    }
}

async fn record_issue(
    store: &dyn LocalProtocolStore,
    fence: ConnectionFence,
    issue: ConnectionIssue,
) {
    if !matches!(
        timeout(
            Duration::from_secs(1),
            store.record_connection_issue(fence, issue)
        )
        .await,
        Ok(Ok(_))
    ) {
        tracing::debug!(machine_id=%fence.machine.machine_id,"local connection diagnostic was not saved");
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
pub(crate) mod tests;
