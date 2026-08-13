# Pattern B: Persistent Reverse WebSocket Tunnel Architecture Plan for `mail-agents-server`

This document outlines the architectural design and implementation plan for executing OpenCode agent prompts on a user's **local machine** using a persistent reverse WebSocket tunnel (Pattern B).

With this pattern, the user runs a local daemon (`opencode-tunnel` or `opencode serve`) on their machine. The daemon establishes a secure, outbound-only WebSocket connection back to `mail-agents-server`. No open incoming ports, static IPs, or public tunneling services (like Ngrok or Cloudflare Tunnels) are required.

---

## 1. High-Level System Architecture

```
+-----------------------------------------------------------------------------------+
|                                mail-agents-server                                 |
|                                                                                   |
|  +-------------------+       +-----------------------+     +-------------------+  |
|  │ Inbound Webhook / │       │ ThreadUseCases        │     │ Workflow Entity   │  |
|  │ SMTP Gateway      ├──────►│ (Ingest & Pipeline)   ├───► │ workflow_config   │  |
|  +-------------------+       +-----------┬-----------+     +---------┬---------+  |
+------------------------------------------│───────────────────────────│────────────+
                                           │                           │
                                           ▼                           │
                               +───────────────────────+               │ Read Workflow
                               │ AgentRunner           │◄──────────────┘ Execution Flag
                               +-----------┬-----------+
                                           │
                        +──────────────────┴──────────────────+
                        │ Select Execution Driver             │
                        +──────────────────┬──────────────────+
                                           │
                                           │ If Workflow Flag = "local_tunnel"
                                           │
                                           ▼
                               +───────────────────────+
                               │ LocalTunnelDriver     │
                               +-----------┬-----------+
                                           │
                                           │ Lookup Active Connection
                                           ▼
                               +───────────────────────+
                               │ TunnelRegistry        │
                               │ (In-Memory Sessions)  │
                               +-----------┬-----------+
                                           │
                                 Persistent Outbound
                              WebSocket Connection (WSS)
                                           │
                                           ▼
+-----------------------------------------------------------------------------------+
|                              User's Local Machine                                 |
|                                                                                   |
|  +-----------------------------------------------------------------------------+  |
|  │ opencode-tunnel Daemon                                                      │  |
|  │                                                                             │  |
|  │  1. Receives execute_request via WebSocket                                  │  |
|  │  2. Executes `opencode run` locally (with access to local files & tools)    │  |
|  │  3. Returns execute_response with content and token usage                   │  |
|  +-----------------------------------------------------------------------------+  |
+-----------------------------------------------------------------------------------+
```

---

## 2. Key Advantages

1. **Zero Firewall Configuration:** The connection is initiated outbound from the user's machine to `mail-agents-server` over standard HTTPS/WSS (`port 443`).
2. **Access to Local Resources:** The local `opencode` instance can read local files, execute local code, access local databases, or call intranet APIs.
3. **Instant Real-Time Execution:** Persistent connection eliminates handshake latency for prompt requests.

---

## 3. Workflow Configuration (`Workflow.workflow_config`)

To enable local tunnel execution for a specific workflow, set `"execution_environment": "local_tunnel"` and generate a unique `tunnel_key`:

```json
{
  "name": "Local Dev Agent",
  "execution_environment": "local_tunnel",
  "tunnel_key": "mat_live_9f8e7d6c5b4a3210",
  "system_prompt": "You are a helpful local assistant with file access.",
  "llm": {
    "provider": "openai",
    "model": "gpt-4o"
  }
}
```

---

## 4. Reverse WebSocket Protocol Specification

### Endpoint
`GET /v1/tunnels/connect?key={tunnel_key}` (Upgraded to WebSocket via Axum `ws` feature).

### Message Types (JSON over WebSocket)

#### 1. Server $\rightarrow$ Local Daemon: `execute_request`
```json
{
  "type": "execute_request",
  "request_id": "req_550e8400-e29b-41d4-a716-446655440000",
  "prompt": "Inspect the local database schema in ./migrations",
  "history": [
    { "role": "Human", "sender": "user@example.com", "clean_text_body": "Can you check the migrations?" }
  ],
  "system_prompt": "You are a helpful assistant.",
  "recipient_role": "to",
  "timeout_secs": 60
}
```

#### 2. Local Daemon $\rightarrow$ Server: `execute_response`
```json
{
  "type": "execute_response",
  "request_id": "req_550e8400-e29b-41d4-a716-446655440000",
  "status": "success",
  "content": "Found 5 migration files in ./migrations directory...",
  "token_usage": {
    "prompt_tokens": 142,
    "completion_tokens": 68
  },
  "error": null
}
```

#### 3. Heartbeat / Ping-Pong (`ping` / `pong`)
To keep stateful connections alive across NATs and load balancers:
- Server sends `{"type": "ping"}` every 15 seconds.
- Local Daemon responds `{"type": "pong"}` within 5 seconds.

---

## 5. Server Component Details

### A. Tunnel Registry (`src/adapters/http/tunnel_registry.rs`)
An in-memory, thread-safe registry stored in `AppState`:

```rust
pub struct TunnelSession {
    pub workflow_id: Uuid,
    pub tx: mpsc::Sender<TunnelMessage>,
    pub pending_requests: Arc<DashMap<String, oneshot::Sender<TunnelResponse>>>,
}

#[derive(Clone)]
pub struct TunnelRegistry {
    sessions: Arc<DashMap<Uuid, TunnelSession>>,
}
```

### B. WebSocket Handler (`src/adapters/http/routes/tunnel.rs`)
1. Extract `tunnel_key` from query params or headers.
2. Resolve matching `Workflow` from database using `tunnel_key`.
3. Upgrade HTTP request to WebSocket connection.
4. Register `TunnelSession` in `TunnelRegistry`.
5. Run read/write loops with ping-pong heartbeat.
6. Automatically unregister session on disconnection or drop.

### C. `LocalTunnelAgentDriver` (`src/application/services/drivers/local_tunnel.rs`)
Implements `AgentExecutionDriver`:

```rust
#[async_trait::async_trait]
impl AgentExecutionDriver for LocalTunnelAgentDriver {
    async fn execute(
        &self,
        full_prompt: &str,
        params: &ResolvedAgentParams,
        _approval_context: Option<&ApprovalContext>,
    ) -> anyhow::Result<AgentExecutionOutput> {
        let workflow_id = params.workflow_id
            .ok_or_else(|| anyhow::anyhow!("Workflow ID required for local tunnel execution"))?;

        // 1. Get active session from TunnelRegistry
        let session = self.registry.get_session(&workflow_id)
            .ok_or_else(|| anyhow::anyhow!("Local agent runner for this workflow is offline. Please connect your local opencode-tunnel."))?;

        // 2. Register oneshot channel for request_id
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        session.pending_requests.insert(request_id.clone(), tx);

        // 3. Send execute_request over WebSocket
        let req_msg = TunnelMessage::ExecuteRequest { ... };
        session.tx.send(req_msg).await?;

        // 4. Await response with timeout
        let timeout_duration = Duration::from_secs(60);
        match tokio::time::timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => {
                if response.status == "success" {
                    Ok(AgentExecutionOutput {
                        content: response.content,
                        token_usage: response.token_usage,
                    })
                } else {
                    Err(anyhow::anyhow!("Local agent execution failed: {}", response.error.unwrap_or_default()))
                }
            }
            Ok(Err(_)) => Err(anyhow::anyhow!("Tunnel connection dropped during execution")),
            Err(_) => Err(anyhow::anyhow!("Local agent execution timed out after 60s")),
        }
    }
}
```

---

## 6. Implementation Milestones

- **Milestone 1: Tunnel Registry & WebSocket Endpoint**
  - Add `axum` `ws` feature to `Cargo.toml`.
  - Create `TunnelRegistry` in `src/adapters/http/tunnel_registry.rs`.
  - Add WebSocket route `GET /v1/tunnels/connect` in `src/adapters/http/routes/tunnel.rs`.

- **Milestone 2: `LocalTunnelAgentDriver` Implementation**
  - Implement `LocalTunnelAgentDriver` in `src/application/services/drivers/local_tunnel.rs`.
  - Integrate driver choice in `AgentRunner::execute()`.

- **Milestone 3: Local CLI Client (`opencode-tunnel`)**
  - Provide a lightweight daemon command or script (e.g. `opencode tunnel --key mat_live_...`) that connects to the server and executes `opencode run` locally upon receiving `execute_request`.

- **Milestone 4: Verification & Integration Tests**
  - Test WebSocket connection registration, heartbeat, and graceful disconnect.
  - Test end-to-end flow: Inbound email $\rightarrow$ WebSocket dispatch $\rightarrow$ local execution response $\rightarrow$ outbound email.
