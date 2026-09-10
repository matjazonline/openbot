# Step 6: Bounded execution, approvals, and suspension

Dependencies: steps 2–5. Main files: proposed `src/adapters/harness/rig/mod.rs`, `approval.rs`, and
execution helpers; existing `src/application/services/harness/ports.rs` is the boundary.

## Run implementation

Implement `AgentHarness` for `RigHarness`, returning `HarnessKind::Rig`. Build fresh per-run state
from the compiled spec and registry model; reuse only immutable factories/transport resources.
Keep pure compilation synchronous and box the external runtime future at the existing seam.

Use the pinned Rig runner with lifecycle hooks if the dependency proof demonstrates that hooks can
terminate before subsequent calls. Current `AgentRunner` documents bounded model calls, sequential
tools by default, and a shared hook-driven execution loop. Explicitly configure these settings.
If its hooks cannot implement immediate suspension, drive its lower-level run/completion surface
inside this adapter instead; do not ship a loop that continues after parking the task. Source:
[AgentRunner API](https://docs.rs/rig/latest/rig/struct.AgentRunner.html).

## Approval and outcome rules

Before executing a tool requiring approval, call the existing `HarnessApprovals::decide` with
`ApprovalAsk`/`ApprovalTrigger`. Reuse existing payload shape, canonical arguments, deterministic
step-key hashing, prior decisions, and internal-delegation policy. Missing approval context never
implies approval. Do not duplicate policy in Rig prompts or provider hooks.

| Event | Required action |
| --- | --- |
| Approved | Invoke the guarded tool once under current ownership |
| Pending | Stop the real runner; return `Suspended` with no reply content |
| Rejected | Do not invoke; provide a sanitized denial if continued reasoning is allowed |
| Approval persistence/lookup error | Abort and return the application error |
| Native tool returns `Suspended` | Stop later calls; preserve its durable task transition |
| Successful final response | Return `Completed` with sanitized text and usage |
| Provider/runtime failure | Return an error, never an emailed failure string |

Use a typed run-local stop reason when Rig's hook/tool error channel cannot carry the application's
outcome. Check it before mapping the runner's final error so intentional suspension cannot become
an ordinary retry. Never detect suspension by matching output text. Keep per-run state isolated.

## Limits and lifecycle

The outer task supervisor continues to own the run deadline, lease, and shutdown. Audit every Rig
entry path, including simulation, for equivalent timeout coverage. Dropping/cancelling the harness
future must stop actual provider/tool work; do not detach correctness-critical Tokio tasks.

Add one run-scoped budget for model calls (step 3), tool invocations, input tokens, generated tokens,
tool-result bytes, and skill steps. Choose numeric ceilings in the implementation policy after
checking existing limits; expose no config knob without enforcement. Count hidden retries and
nested skill calls. Recheck prompt size after tool outputs are appended and reserve output room.
Use a provider/model tokenizer where available and a documented conservative fallback otherwise;
message count or observed usage alone is not a prompt budget.

Set connect/request/tool timeouts within the remaining run deadline. Keep tool concurrency at one.
Disable automatic retry of side-effecting tools; provider retries are bounded and must not replay
tools. Respect existing durable task retry classification/backoff. If `AppError` cannot preserve a
required retryable-versus-terminal distinction, introduce an explicit application error category
and update all matches/tests, rather than relying on provider error wording.

Require current ownership immediately before durable side effects through the existing host ports.
Cancellation cannot undo a request already accepted remotely; retain current idempotency and
recovery guarantees rather than promising exactly-once external delivery.

## Verification and acceptance

- Pending approval on the first of two returned tool calls results in zero invocations; suspension
  from the first tool prevents the second. No additional model request follows either suspension.
- Approved, rejected, pending, lookup-error, timeout, max-budget, and normal completion differ.
- Lease loss and shutdown cancel a blocked provider request and prevent later host commits.
- Tool loops and nested skill calls cannot evade the shared ceilings.
- Stack measurements and the stock-stack regression gate pass without raising existing limits.
