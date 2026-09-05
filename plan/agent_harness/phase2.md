# Phase 2 — The `AgentHarness` port

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. Depends on phase 1.

**Goal.** The seam. One trait that says "run this capability spec and give me back an execution
output", a registry that resolves an implementation by `HarnessKind`, and the two sub-ports that let
a harness reach approvals and our native tools without the application layer knowing what
`ai_agents::hitl::ApprovalHandler` is.

Still no behaviour change: nothing implements `AgentHarness` until phase 3.

---

## Where it lives, and why

`src/application/services/harness/mod.rs`.

Ports are defined where they are consumed (`src/application/AGENTS.md`), and the registry goes
beside them for the reason `TransportRegistry` already documents in
`src/application/transport/ports.rs`:

> Lives in the application/service layer rather than under `adapters` because the delivery worker is
> what consults it; the adapters are what register themselves into it. The previous shape — an
> `EgressRegistry` defined inside `src/adapters/protocols` — was an abstraction living inside the
> layer it abstracts.

That is the exact mistake `plan/opencode_microvm_sandbox_plan.md` makes with its per-call
`Box::new(InProcessAgentDriver::new())`, and the exact rule `src/AGENTS.md` states as *"an
abstraction must not live inside the outer adapter it is intended to abstract"*. Follow the
transport shape, not the sandbox plan's.

## 1. The port

```rust
#[async_trait]
pub trait AgentHarness: Send + Sync {
    fn kind(&self) -> HarnessKind;

    /// Run one agent turn to completion, suspension, or failure.
    ///
    /// The implementation owns everything between a composed prompt and a response: building its
    /// runtime, granting tools, executing skills, and counting tokens. It does not own prompt
    /// composition, the spam guardrail, the wall-clock deadline, or the lease — those stay with the
    /// caller, which is why they are absent from `AgentRun`.
    async fn run(&self, run: AgentRun<'_>) -> AppResult<AgentExecutionOutput>;
}
```

No default bodies. `src/application/AGENTS.md` is explicit that a silently-successful default is how
a broken protocol passes its tests, and this trait's single method *is* the correctness operation.
Note the contrast with `AgentPersistence` (`use_cases/agent.rs:168`), whose
`create_library`/`list_library` defaults return `Err`/`Ok(vec![])` — that is the anti-pattern, and
phase 4 does not copy it for `SkillManagementPersistence` or `AgentCapabilityReader`.

`AgentExecutionOutput`, `AgentExecutionDisposition` and `TokenUsage` already exist
(`agent_runner.rs:63`, `:72`, `entities/task.rs`) and are already harness-neutral. Move
`AgentExecutionOutput` and its disposition into this module; leave `TokenUsage` where it is.

## 2. `AgentRun<'_>` — name the tuple

```rust
/// Everything one harness run needs, and nothing more.
///
/// A struct rather than an eight-argument call: `src/AGENTS.md` — any tuple with three-plus
/// elements, or two same-typed elements, becomes a struct with named fields. Two `&str` prompt
/// fields and two `Option<Arc<dyn …>>` ports sit here side by side.
pub struct AgentRun<'a> {
    /// Boxed: it carries every skill body, so it dominates any future or enum it lands in.
    pub spec: Box<AgentCapabilitySpec>,
    /// Resolved from the company's encrypted model connection by the caller. Never logged, and
    /// passed to `sanitize_text` as the literal to redact.
    pub api_key: &'a str,
    /// Already composed, fenced and guardrailed by the caller.
    pub full_prompt: &'a str,
    pub history_message_count: usize,
    pub recipient_role: Option<RecipientRole>,
    pub approvals: Option<&'a dyn HarnessApprovals>,
    pub tool_host: Option<&'a dyn HarnessToolHost>,
    pub trace: Option<&'a dyn HarnessTrace>,
    /// Set by the approval handler or the outreach tool when the run parks awaiting a human or
    /// another agent. The caller reads it to decide `Completed` vs `Suspended`.
    pub suspended: Arc<AtomicBool>,
}
```

`api_key` stays a `&str` and not a newtype deliberately — it is the one value `sanitize_text`
matches literally, and wrapping it invites a `Display` impl that leaks it into a log line.

## 3. The registry

```rust
#[derive(Default)]
pub struct HarnessRegistry(HashMap<HarnessKind, Arc<dyn AgentHarness>>);

impl HarnessRegistry {
    /// Consuming and fallible, like `TransportRegistry::register`: a duplicate or mismatched
    /// registration is a boot-time error, not a last-write-wins surprise.
    pub fn register(self, harness: Arc<dyn AgentHarness>) -> Result<Self, HarnessRegistrationError>;
    pub fn get(&self, kind: HarnessKind) -> Option<&Arc<dyn AgentHarness>>;
    pub fn require(&self, kind: HarnessKind) -> Result<&Arc<dyn AgentHarness>, UnsupportedHarness>;
    pub fn registered(&self) -> Vec<HarnessKind>;
}

pub enum HarnessRegistrationError {
    /// Two registrations for one kind.
    Duplicate(HarnessKind),
    /// The registered implementation's `kind()` disagrees with the slot it was registered into.
    Mismatched { declared: HarnessKind, registered: HarnessKind },
}
```

Copy `TransportRegistry`, not `MemoryProviderRegistry` — the latter's `register` is infallible and
last-write-wins, which is fine for two providers wired conditionally from config but wrong for a
dispatch path where a silently-replaced harness is a silently-changed agent.

`require` returning a typed error rather than `Option` matters at the call site in phase 5: an agent
whose `harness_kind` has no registered implementation must fail the run loudly, not fall back to the
default harness. A fallback here would mean a deployment that drops a harness silently downgrades
every agent using it.

## 4. Sub-ports — keeping `ai_agents` out

Two things a harness needs that only the application layer can answer. Both are traits so the
adapter never reaches back into use cases.

### Approvals

```rust
#[async_trait]
pub trait HarnessApprovals: Send + Sync {
    /// Decide one approval trigger, parking the run if a human must answer.
    async fn decide(&self, ask: ApprovalAsk<'_>) -> AppResult<ApprovalVerdict>;
}

/// A harness-neutral restatement of `ai_agents::hitl::ApprovalTrigger`.
pub enum ApprovalAsk<'a> {
    Tool { name: &'a str, args: &'a serde_json::Value },
    Condition { name: &'a str, matched: bool },
    State { from: &'a str, to: &'a str },
}

pub enum ApprovalVerdict {
    Approved,
    Rejected { reason: String },
}
```

The body of `AgentApprovalHandler` (`agent_runner.rs:599–790`) moves here almost unchanged: the
`step_key` hash, the prior-decision lookup, the approval email, the `suspended` flag, and
`InternalDelegationPolicy`'s all-internal exemption (`:614` `approves_without_human`). What is left
behind in phase 3 is a ~30-line `impl ai_agents::hitl::ApprovalHandler` that translates
`ApprovalTrigger` → `ApprovalAsk` and `ApprovalVerdict` → `ApprovalResult`.

`InternalDelegationPolicy` classifies recipients against the channel directory — an authorization
decision — so it moves with the handler and keeps its `?` propagation. Do not let it acquire an
`.unwrap_or(false)` on the way across.

### Native tools

```rust
#[async_trait]
pub trait HarnessToolHost: Send + Sync {
    /// The native tools this run may use. The harness grants exactly these plus its own built-ins.
    fn available(&self) -> &[ToolId];
    async fn invoke(&self, id: &ToolId, args: serde_json::Value) -> AppResult<ToolInvocation>;
}

pub struct ToolInvocation {
    pub success: bool,
    pub output: serde_json::Value,
}
```

Today the three native tools are `impl ai_agents::Tool` structs registered in `build_with_tools`
(`agent_runner.rs:1424`) only when the run supplies the matching context — `agent_persistence`
zipped with `binding_persistence` for the directory tool, an `OutreachToolContext` for outreach, an
`AgentChannelProvisioning` for channel creation. That "context present ⇒ tool registered" coupling
becomes `available()`: the host reports which of the three it can actually serve, and the harness
grants that intersection with `spec.granted_tools`.

Keep the JSON schemas where the tools are. `invoke` is a dispatcher, not a schema registry — the
adapter reads each tool's schema off the native tool itself in phase 3.

### Trace

```rust
#[async_trait]
pub trait HarnessTrace: Send + Sync {
    async fn tool_started(&self, tool: &ToolId, args: &serde_json::Value);
    async fn tool_finished(&self, record: ToolTraceRecord<'_>);
    async fn run_failed(&self, error: &str);
}
```

`AgentTraceHooks` (`agent_trace_hooks.rs`) already does exactly this against
`ai_agents::AgentHooks`; its `source_label()` maps `ToolCallSource::{Model, Skill, StateAction,
Plan, Orchestration, Spawner}` and that enum is the one genuinely harness-shaped thing in it.
Restate it as a `ToolTraceSource` in this module with the same variants, and let the phase-3 shim
translate. The comment at `agent_runner.rs:1466` explains why this is a hook and not logging inside
each tool — it sees the runtime's built-ins and anything behind MCP too — and that reason survives
the move.

## 5. What does **not** move

`AgentRunner` keeps prompt composition (`compose_prompt`, `render_history`, `UntrustedFence`), the
spam guardrail call, credential resolution, and the `AiExecutionMetrics` record. Those are ours in
every harness. The `tokio::time::timeout` wall clock and the `while_leased` heartbeat stay in
`dispatch.rs`/`task_worker.rs` where they are.

One consequence to hold onto for phase 3: `src/application/AGENTS.md` says *"Losing ownership
cancels the real work — do not spawn an inner agent future and then lease only the outer
`JoinHandle`."* `AgentHarness::run` is awaited inside `while_leased`, so cancellation propagates by
drop. A future sandbox harness that POSTs to a remote VM must honour that in its `Drop` — teardown
on cancel, not only on the happy path. Write that requirement into the trait's doc comment now,
while there is one implementation and it is trivially true.

## 6. Stack budget — measure before you build

Record the baseline in `phase3.md` before touching anything:

```sh
./scripts/stack-frames.sh
```

`AgentHarness::run` is the `Box::pin` seam. It replaces — does not wrap —
`Box::pin(self.build_agent())` and `Box::pin(agent.chat(..))`. If the after-number is worse than the
before-number, the cause is a forwarding `async fn` somebody added between the caller and the trait
method; delete it rather than raising a limit.

---

## Tests

Pure, no mocks, inline:

- `registering_two_harnesses_for_one_kind_is_refused`
- `a_harness_whose_kind_disagrees_with_its_slot_is_refused`
- `require_reports_the_missing_kind_rather_than_falling_back` — the one that encodes the
  no-silent-downgrade rule
- `an_approval_ask_round_trips_its_variants`

A `StubHarness` returning a canned `AgentExecutionOutput` goes in
`src/application/services/test_support.rs` beside the existing mocks, not re-declared per file.

## Done when

`cargo test --lib services::harness::` is green, and `grep -rn "ai_agents" src/application/services/harness/`
returns nothing. `src/application/` as a whole is still full of `ai_agents` imports — that is
phase 3's job.
