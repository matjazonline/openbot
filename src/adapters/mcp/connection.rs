use super::*;
use crate::application::services::mcp_client::{
    McpPreparationFailure as Failure, McpPreparationResult,
};
use preparation::Control;

struct Resources {
    endpoint_permit: OwnedSemaphorePermit,
    permit: OwnedSemaphorePermit,
    token: Option<SecretString>,
    remaining: Arc<std::sync::atomic::AtomicUsize>,
    finished: tokio::sync::oneshot::Receiver<()>,
    cleanup: http::Cleanup,
    control: Control,
}
impl HttpMcpClient {
    pub(super) async fn connect_control(
        &self,
        endpoint: &McpEndpoint,
        token: Option<SecretString>,
        control: Control,
    ) -> McpPreparationResult<Box<dyn McpSession>> {
        let (endpoint_permit, permit) = control
            .run(async {
                self.permits(endpoint)
                    .await
                    .map_err(|_| Failure::Unavailable)
            })
            .await?;
        let client = control
            .run(async {
                self.policy
                    .client(endpoint)
                    .await
                    .map_err(|_| Failure::Unavailable)
            })
            .await?;
        let mut workers = control.run(async { Ok(self.workers.lock().await) }).await?;
        while let Some(result) = workers.try_join_next() {
            if result.is_err() {
                tracing::warn!("MCP worker failed");
            }
        }
        control.check().await?;
        let mut config =
            StreamableHttpClientTransportConfig::with_uri(endpoint.as_str().to_owned());
        config.auth_header = token.as_ref().map(|token| token.expose_secret().to_owned());
        config.allow_stateless = true;
        config.reinit_on_expired_session = false;
        config.channel_buffer_capacity = 8;
        let remaining = Arc::new(std::sync::atomic::AtomicUsize::new(MAX_MCP_DISCOVERY_BYTES));
        let (lifetime, finished) = preparation::lifetime();
        let cleanup = http::Cleanup {
            client: client.clone(),
            endpoint: endpoint.as_str().to_owned(),
            token: token.clone(),
            session: Default::default(),
        };
        let transport = StreamableHttpClientTransport::with_client(
            http::BoundedHttp {
                client,
                remaining: remaining.clone(),
                control: control.clone(),
                lifetime,
                cleanup: cleanup.clone(),
            },
            config,
        );
        let (session, receiver) = worker::ProxySession::channel();
        let (ready, initialized) = tokio::sync::oneshot::channel();
        let resources = Resources {
            endpoint_permit,
            permit,
            token,
            remaining,
            finished,
            cleanup,
            control: control.clone(),
        };
        workers.spawn(initialize(transport, resources, receiver, ready));
        drop(workers);
        initialized.await.map_err(|_| control.failure())??;
        Ok(Box::new(session))
    }
}

async fn initialize(
    transport: StreamableHttpClientTransport<http::BoundedHttp>,
    resources: Resources,
    receiver: worker::Receiver,
    mut ready: tokio::sync::oneshot::Sender<McpPreparationResult<()>>,
) {
    let Resources {
        endpoint_permit,
        permit,
        token,
        remaining,
        finished,
        cleanup,
        control,
    } = resources;
    let _stop_on_abandon = control.stop.clone().drop_guard();
    let transport_stop = transport.cancel_token();
    // Dropping this initializer drops its transport sending endpoints. Never retain the
    // WorkerTransport while joining: the SDK handshake waits ignore its cancellation token.
    let result = tokio::select! {biased;
        _=ready.closed()=>Err(control.record(Failure::Cancelled)),
        result=control.run(async {
            ().serve_with_ct(transport,control.stop.clone()).await.map_err(|error| {
                match errors::initialization(error) {
                    AppError::McpAuthentication => control.record(Failure::Authentication),
                    _ => control.failure(),
                }
            })
        })=>result,
    };
    match result {
        Ok(client) => {
            let session = Session {
                client,
                _permit: permit,
                _endpoint_permit: endpoint_permit,
                token,
                remaining,
                pending_call: Default::default(),
                stop: control.stop.clone(),
                finished: Some(finished),
                cleanup,
            };
            if let Err(error) = control.check().await {
                let _ = Box::new(session).close().await;
                let _ = ready.send(Err(error));
                return;
            }
            let _ = ready.send(Ok(()));
            worker::run(session, receiver, control.stop.clone()).await;
        }
        Err(error) => {
            control.stop.cancel();
            transport_stop.cancel();
            // The private capability is also held by returned byte/SSE streams. Its final
            // drop establishes that SDK HTTP work ended, not merely that cancellation fired.
            let complete = finished.await;
            let closed = cleanup.finish().await;
            let failure = if complete.is_ok() && closed.is_ok() {
                error
            } else {
                Failure::Unavailable
            };
            let _ = ready.send(Err(failure));
        }
    }
}
