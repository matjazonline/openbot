# MicroVM Sandbox Isolation (Option 2) Architecture Plan for `mail-agents-server`

This document details the architectural design and implementation plan for executing OpenCode prompts inside isolated, hardware-virtualized MicroVM sandboxes (e.g., **E2B**, **Daytona**, or a custom Firecracker microservice) for `mail-agents-server`.

Execution mode (`in_process` vs `microvm_sandbox`) is specified per **Workflow** in `workflow_config` (with system-level fallback credentials defined in `AppConfig`).

---

## 1. High-Level Architecture

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
         ┌─────────────────────────────────┴─────────────────────────────────┐
         │                                                                   │
If Workflow Flag = "in_process"                      If Workflow Flag = "microvm_sandbox"
         │                                                                   │
         ▼                                                                   ▼
+─────────────────────────+                               +──────────────────────────+
│ InProcessAgentDriver    │                               │ AgentSandboxRunner       │
│ (ai-agents in Tokio)    │                               │ (E2B / Daytona Client)   │
+─────────────────────────+                               +────────────┬─────────────+
                                                                       │
                                                             HTTP REST / gRPC Call
                                                                       │
                                                                       ▼
                                                          +──────────────────────────+
                                                          | MicroVM Sandbox Instance |
                                                          |  +--------------------+  |
                                                          |  | opencode run "..." |  |
                                                          |  +--------------------+  |
                                                          +──────────────────────────+
```

---

## 2. MicroVM Sandbox Lifecycle

For workflows set to `microvm_sandbox`:

1. **Workflow Resolution:** When `AgentRunner` resolves execution parameters, it reads `execution_environment` (`"in_process"` or `"microvm_sandbox"`) from the target `Workflow`'s `workflow_config`.
2. **Sandbox Provisioning:** If set to `microvm_sandbox`, `AgentSandboxRunner` invokes the configured sandbox provider (`POST /sandboxes`) to boot a clean microVM instance (< 500ms).
3. **Environment & Context Ingestion:**
   - Provider API keys (`OPENAI_API_KEY`, etc.) are securely set in the sandbox environment.
   - Conversation history, system prompts, and configuration YAML are staged in an isolated `/workspace` directory inside the VM.
4. **`opencode` Execution:**
   - Command executed inside sandbox: `opencode run --config /workspace/config.yaml --prompt /workspace/prompt.txt`
   - Strict execution timeout enforcement (default: 60s).
5. **Output Extraction & Sanitization:**
   - Parse `stdout` for response content and token usage (`prompt_tokens`, `completion_tokens`).
   - Run existing `sanitize_text()` to prevent credential leakage into output logs.
6. **Mandatory Teardown:**
   - An internal RAII `SandboxGuard` guarantees `DELETE /sandboxes/{id}` is executed upon completion, failure, or panic.

---

## 3. Component Details & Design Patterns

### A. Configuration (`src/infra/config.rs` & `.env.example`)
Global system settings for sandbox execution:

```rust
pub struct AppConfig {
    // ... existing fields ...
    pub sandbox_provider: String,        // "e2b", "daytona", or "custom_http"
    pub sandbox_api_key: Option<String>, // API key for sandbox service
    pub sandbox_api_url: String,         // Base URL (e.g. "https://api.e2b.dev")
    pub sandbox_template_id: String,     // Sandbox image template with opencode pre-installed
    pub sandbox_timeout_secs: u64,       // Execution timeout limit in seconds (default: 60)
}
```

Environment variables:
```env
SANDBOX_PROVIDER=e2b
SANDBOX_API_KEY=e2b_...
SANDBOX_API_URL=https://api.e2b.dev
SANDBOX_TEMPLATE_ID=opencode-runner-v1
SANDBOX_TIMEOUT_SECS=60
```

---

### B. Workflow Entity & Resolution (`src/domain/entities/workflow.rs` & `src/application/services/agent_runner.rs`)

Workflows specify `execution_environment` (`"in_process"` or `"microvm_sandbox"`) inside `workflow_config`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEnvironment {
    InProcess,      // Default: direct ai-agents execution in host Tokio thread
    MicroVmSandbox, // Isolated execution in ephemeral MicroVM via opencode
}

impl ResolvedAgentParams {
    pub fn execution_environment(&self) -> ExecutionEnvironment {
        self.config
            .get("execution_environment")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "microvm_sandbox" => ExecutionEnvironment::MicroVmSandbox,
                _ => ExecutionEnvironment::InProcess,
            })
            .unwrap_or(ExecutionEnvironment::InProcess)
    }
}
```

---

### C. Execution Driver Strategy

#### Driver Trait (`src/application/services/agent_runner.rs`)
```rust
#[async_trait::async_trait]
pub trait AgentExecutionDriver: Send + Sync {
    async fn execute(
        &self,
        full_prompt: &str,
        params: &ResolvedAgentParams,
        approval_context: Option<&ApprovalContext>,
    ) -> anyhow::Result<AgentExecutionOutput>;
}
```

#### Driver Implementations
1. **`InProcessAgentDriver`** (`src/application/services/drivers/in_process.rs`): Executes via `ai_agents` library directly in the host process.
2. **`AgentSandboxRunner`** (`src/application/services/drivers/sandbox.rs`): Handles sandbox provisioning, staging, `opencode` execution, and microVM teardown.

---

### D. `AgentRunner` Orchestrator Logic

`AgentRunner` maintains standard setup, sanitization, and metrics logic while delegating execution:

```rust
pub async fn execute(self) -> anyhow::Result<AgentExecutionOutput> {
    // 1. Format delivery context, history, and full prompt
    let raw_full_prompt = format!("{}{}{}{}", delivery_ctx, pipeline_ctx_str, history_str, self.prompt);
    
    // 2. Evaluate LLM Spam Guardrails
    if !self.skip_spam_guardrail { ... }

    // 3. Select Execution Driver based on Workflow Config
    let driver: Box<dyn AgentExecutionDriver> = match self.params.execution_environment() {
        ExecutionEnvironment::InProcess => Box::new(InProcessAgentDriver::new()),
        ExecutionEnvironment::MicroVmSandbox => Box::new(AgentSandboxRunner::new(
            self.app_config.as_ref().expect("AppConfig required for sandbox execution")
        )?),
    };

    // 4. Delegate Execution to Driver
    let start_time = std::time::Instant::now();
    let result = driver.execute(&raw_full_prompt, self.params, self.approval_context.as_ref()).await;
    let duration_ms = start_time.elapsed().as_millis() as u64;

    // 5. Sanitize Output and Record Metrics
    self.record_metrics(&result, duration_ms);
    result
}
```

---

## 4. Implementation Milestones

- **Milestone 1: Configuration & Workflow Flag**
  - Update `AppConfig` and `.env.example` with global sandbox settings.
  - Implement `ResolvedAgentParams::execution_environment()` to read `execution_environment` from `Workflow.workflow_config`.

- **Milestone 2: Execution Driver Refactoring & `AgentSandboxRunner`**
  - Define `AgentExecutionDriver` trait.
  - Refactor existing in-process logic into `InProcessAgentDriver`.
  - Create `AgentSandboxRunner` in `src/application/services/drivers/sandbox.rs` with `reqwest` REST client and `SandboxGuard` RAII cleanup.

- **Milestone 3: `AgentRunner` Wiring**
  - Update `AgentRunner::execute()` to select the driver based on `params.execution_environment()`.
  - Ensure approval context, sanitization, and `MonitoringService` metrics remain consistent across both execution drivers.

- **Milestone 4: Verification & Tests**
  - Unit tests for workflow parameter resolution (`InProcess` vs `MicroVmSandbox`).
  - Mock integration tests for sandbox lifecycle (create $\rightarrow$ execute `opencode` $\rightarrow$ teardown) and timeout/error handling.
