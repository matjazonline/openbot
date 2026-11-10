use super::*;
use axum::extract::ws::Message;

pub(super) async fn send(
    socket: &mut WebSocket,
    message: &ServerMessage,
    stop: &CancellationToken,
) -> AppResult<()> {
    let frames = framing::encode(message)?;
    let write = async {
        for frame in frames {
            socket
                .send(Message::Text(frame.into()))
                .await
                .map_err(|_| AppError::Internal("local transport write failed".into()))?;
        }
        Ok(())
    };
    tokio::select! {biased;
        _=stop.cancelled()=>Err(AppError::Internal("local transport stopped".into())),
        result=timeout(Duration::from_secs(IO_SECONDS),write)=>result.map_err(|_|AppError::Internal("local transport write timed out".into()))?,
    }
}

pub(super) async fn initialize(
    socket: &mut WebSocket,
    store: &dyn LocalProtocolStore,
    stop: &CancellationToken,
    fence: ConnectionFence,
) -> AppResult<bool> {
    let mut assembler = framing::Assembler::handshake();
    let hello = loop {
        let message = socket
            .recv()
            .await
            .ok_or(AppError::InvalidCredentials)?
            .map_err(|_| AppError::InvalidCredentials)?;
        match message {
            Message::Text(text) => {
                if let Some(bytes) = assembler.push(&text)? {
                    let ClientMessage::Hello { hello } =
                        serde_json::from_slice(&bytes).map_err(|_| AppError::InvalidCredentials)?
                    else {
                        return Err(AppError::InvalidCredentials);
                    };
                    break hello;
                }
            }
            _ => return Err(AppError::InvalidCredentials),
        }
    };
    if hello.version != PROTOCOL_VERSION {
        super::record_issue(store, fence, ConnectionIssue::IncompatibleProtocol).await;
    }
    let welcome = store.connect_journal(fence, &hello).await?;
    let ready = !welcome.recovery_required;
    send(socket, &ServerMessage::Welcome { welcome }, stop).await?;
    Ok(ready)
}

pub(super) async fn run(
    socket: &mut WebSocket,
    store: &dyn LocalProtocolStore,
    stop: &CancellationToken,
    fence: ConnectionFence,
    #[cfg(test)] command_write_gate: Option<&super::tests::CommandWriteGate>,
) -> AppResult<()> {
    let identity = ConnectionIdentity::from_fence(fence)?;
    let mut assembler = framing::Assembler::default();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECONDS));
    let mut poll = tokio::time::interval(Duration::from_millis(POLL_MILLIS));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_received = Instant::now();
    let mut window = Instant::now();
    let mut frames = 0usize;
    loop {
        let assembly_deadline = assembler
            .deadline()
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(REASSEMBLY_SECONDS));
        tokio::select! {biased;
            _=stop.cancelled()=>return Ok(()),
            _=tokio::time::sleep_until(assembly_deadline),if assembler.deadline().is_some()=>return Err(AppError::BadRequest("local transport assembly expired".into())),
            incoming=socket.recv()=> {
                if window.elapsed()>=Duration::from_secs(60) {window=Instant::now();frames=0;}
                frames+=1;
                if frames>1024 {return Err(AppError::BadRequest("local transport frame budget exhausted".into()));}
                let Some(incoming)=incoming else {return Ok(());};
                let incoming=incoming.map_err(|_|AppError::BadRequest("invalid local transport".into()))?;
                last_received=Instant::now();
                match incoming {
                    Message::Text(text)=>if let Some(bytes)=assembler.push(&text)? {
                        let message=serde_json::from_slice(&bytes).map_err(|_|AppError::BadRequest("invalid local message".into()))?;
                        process(socket,store,stop,fence,identity,message).await?;
                    },
                    Message::Ping(bytes)=>timeout(Duration::from_secs(IO_SECONDS),socket.send(Message::Pong(bytes))).await.map_err(|_|AppError::InvalidCredentials)?.map_err(|_|AppError::InvalidCredentials)?,
                    Message::Pong(_)=>{},Message::Close(_)=>return Ok(()),
                    Message::Binary(_)=>return Err(AppError::BadRequest("local transport requires text frames".into())),
                }
            },
            _=heartbeat.tick()=> {
                if last_received.elapsed()>=Duration::from_secs(CONNECTION_STALE_SECONDS) || !store.renew_connection(fence).await? {return Err(AppError::InvalidCredentials);}
                tokio::select! {biased; _=stop.cancelled()=>return Ok(()),
                    result=timeout(Duration::from_secs(IO_SECONDS),socket.send(Message::Ping(Vec::new().into())))=>result.map_err(|_|AppError::InvalidCredentials)?.map_err(|_|AppError::InvalidCredentials)?,}
            },
            _=poll.tick()=> {
                let deliveries=tokio::select! {biased; _=stop.cancelled()=>return Ok(()), result=store.pending_commands(fence)=>result?};
                for delivery in deliveries {
                    if !store.renew_connection(fence).await? {return Err(AppError::InvalidCredentials);}
                    #[cfg(test)]
                    if let Some(gate) = command_write_gate.filter(|gate| gate.operation == delivery.operation.id()) {
                        // First-delivery COMMIT has returned, but no command bytes
                        // have reached the socket. Shutdown must cancel this cut.
                        gate.reached.notify_one();
                        stop.cancelled().await;
                        return Ok(());
                    }
                    send(socket,&ServerMessage::Command{identity,delivery:Box::new(delivery)},stop).await?;
                }
            },
        }
    }
}

async fn process(
    socket: &mut WebSocket,
    store: &dyn LocalProtocolStore,
    stop: &CancellationToken,
    fence: ConnectionFence,
    identity: ConnectionIdentity,
    message: ClientMessage,
) -> AppResult<()> {
    match message {
        ClientMessage::RetireMappings {
            identity: received,
            request,
        } => {
            if received != identity {
                return Err(AppError::InvalidCredentials);
            }
            let sessions = store.retired_mappings(fence, &request).await?;
            send(
                socket,
                &ServerMessage::MappingsRetired {
                    identity,
                    nonce: request.nonce,
                    sessions,
                },
                stop,
            )
            .await?;
        }
        ClientMessage::Hello { .. } => {
            return Err(AppError::BadRequest("hello already completed".into()));
        }
        ClientMessage::Ready { identity: received } => {
            if received != identity {
                return Err(AppError::InvalidCredentials);
            }
            let ready = store.finish_reconciliation(fence).await?;
            send(socket, &ServerMessage::Ready { identity, ready }, stop).await?;
        }
        ClientMessage::Observation {
            identity: received,
            observation,
        } => {
            if received != identity {
                return Err(AppError::InvalidCredentials);
            }
            let through = store.accept_observation(fence, &observation).await?;
            send(
                socket,
                &ServerMessage::Acknowledged { identity, through },
                stop,
            )
            .await?;
        }
        ClientMessage::Heartbeat { identity: received } => {
            if received != identity || !store.renew_connection(fence).await? {
                return Err(AppError::InvalidCredentials);
            }
        }
    }
    Ok(())
}
