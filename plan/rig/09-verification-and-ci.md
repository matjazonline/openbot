# Step 9: Contract tests and CI gates

Dependencies: steps 1–8. Main files: Rig sibling test modules, shared application test support,
persistence/HTTP tests, `.github/workflows/ci.yml`, and relevant scripts.

## Test matrix

Use our existing simulated agent endpoint, `scripted_llm` in
`src/application/services/test_support.rs`, as the shared foundation for model-backed tests in
required CI. Extend it for Rig and provider-specific protocols rather than building a second
simulator or replacing the harness. Real provider credentials and model nondeterminism must not
be prerequisites for a green build. Keep all fixtures free of real credentials and private mail.
Exercise serialization at each actual provider boundary, not only a fake `AgentHarness` that
bypasses the new adapter. Small dispatch-only unit tests may retain `StubHarness`.

## Existing endpoint and tool-call support

The current helper binds `127.0.0.1:0`, returns a finite sequence of OpenAI Chat Completions
responses, and captures request bodies. `register_scripted_agent_base_url(agent_id, base_url)`
feeds the trusted `provider_base_url` through the test-only capability-resolution hook.
`LlmTurn::tool_call(name, arguments)` already emits an assistant tool call with a wire ID;
the real harness executes it and sends the result in its next model request. For example, this
existing API scripts a complete two-request conversation:

```rust
let mut llm = scripted_llm(vec![
    LlmTurn::tool_call("todo", serde_json::json!({ "operation": "list" })),
    LlmTurn::text("No tasks are pending."),
]).await;
```

See `completes_a_two_turn_tool_loop_through_ai_agents` in
`src/application/services/agent_runner/mod_tests.rs` for fixture wiring and assertions on tool
declarations, assistant history, and result correlation. Database-backed examples already live in
`src/application/use_cases/thread/{external_reply_tests,inter_channel_tests}.rs` and
`src/application/services/task_worker.rs`.

The simulator supplies the model's choice of tool, not the tool's result. Built-ins and native
tools execute through their actual grants, dispatcher, application services, and test database.
For HTTP MCP, a separate local MCP fixture supplies the external server response; the real MCP
client, discovery, and guarded bridge still run. External delivery and other network tools likewise
use local endpoints or existing recording transports at their external boundary.

## Request-checked scenarios

Extend the shared helper into an ordered scenario with an expected request and scripted response
per exchange. Keep the short `LlmTurn` API for simple cases. The following are implementation
requirements, not capabilities of today's helper:

- Select the scenario explicitly in each test when creating its endpoint. Use one endpoint/state
  per independent run or agent; interleaved tenant or delegated-agent requests must not race to
  consume a global response queue. Keep endpoint registration test-only, with scoped cleanup.
- Before answering, assert method/path, selected model, relevant prompt/history fields, and tool
  declarations (names and meaningful schema constraints). Match structured fields and intentional
  prompt fragments instead of the whole rendered prompt, ordering of JSON object keys, or generated
  timestamps. Redact fixture auth headers in diagnostics; compare their synthetic values in memory.
- Script explicit provider call IDs, ordered batches of calls, and optional text alongside calls.
  Validate the next request contains the corresponding assistant calls and exactly one result per
  completed call, with the expected content and no duplicates/orphans. Preserve provider-specific
  IDs and opaque continuation fields, including distinct call/item IDs where applicable.
- Make the final reply conditional on matching the expected tool results. A fixed final answer
  alone cannot prove a tool succeeded. Assert the resulting database state, invocation count, and
  task/message/outbox state independently; a tool-result assertion is not proof of a committed effect.
- Fail on an unexpected request, mismatch, or unused required exchange. Provide an explicit awaited
  completion assertion that propagates server failures and checks exact counts, including zero
  later requests after suspension. Do not treat an error response that the runtime handles as proof
  the test passed. Keep the recorder active until the run has settled so extra calls are observable.
- Script status/headers/raw malformed bodies, usage, disconnects, and retry sequences. Use bounded
  barriers to hold a response until a competing claimant or cancellation event arrives; avoid
  timing races based on arbitrary sleeps. Only the retries declared by the scenario may occur.
- Bound request/response bytes, captured exchanges, accept/read/write waits, and fixture lifetime.
  Own listener/connection tasks, cancel and await them on explicit shutdown, and provide abort/drop
  cleanup when a test fails. Do not leave detached listeners or endpoint registrations behind.

Prefer explicit scenarios over magic phrases that choose behavior by scanning incoming mail.
Ordinary prompts remain realistic test input; the scenario states what the model should do with
them. A prompt change then fails only the relevant assertion, rather than selecting a different
script or silently returning a fallback answer. Reusable named scenarios are useful fixture
builders, not commands interpreted by the production agent or a new production model provider.

## Provider protocols and network isolation

Separate reusable scenario expectations from provider wire encoding/decoding in shared test support.
The existing helper only implements OpenAI Chat Completions; add fixtures for the actual API chosen
by each factory in step 2. Do not route all five logical providers through the OpenAI client just
to reuse a response shape. Keep explicit wire assertions per provider so a normalized scenario
cannot hide incorrect serialization, authentication, result correlation, or usage parsing.
Unsupported simulator protocols must fail rather than fall through to a real provider.

Route every model call in a scenario to a registered local endpoint, including skill routing and
any enabled auxiliary model calls. The separate `TextClassifier` remains an explicit deterministic
test double unless its own provider boundary is under test. Use synthetic company credentials and
no developer/provider environment fallback. Configure required CI test execution to deny external
network egress while allowing fixture listeners and the isolated test database; fetch dependencies
before that phase. Prove the guard rejects an unregistered external destination without making a
live-provider request. A missing fixture registration must fail locally, not attempt authentication
against a public endpoint. Apply the same rule to MCP, `web_fetch`, and delivery fixtures.

## Release matrix

| Area | Release evidence |
| --- | --- |
| Registry | Five keys, exact models, unsupported/duplicate registration, trusted endpoints |
| Isolation | Concurrent tenants, key rotation, isolated mutable tools/run state |
| Providers | Text, tool call/result continuation, malformed payload, auth error, rate limit, timeout |
| Simulator | Checked requests/results, exact exchange counts, batch IDs, fixture cleanup, provider wire formats, denied external egress |
| Configuration | Unset env → Rig, ai-agents override, explicit precedence, invalid env, typed bounds |
| Tools | Native/built-in schemas, allowlist, unavailable context, forged names, bounded output |
| HTTP MCP | Company definitions/grants, multi-agent selections, shared rotation/revocation, tenant/session isolation, guarded JSON/SSE transport, schema drift, no automatic approval, checkpoint/restart, indeterminate effects; full step 4a matrix |
| Skills | Ordered calls, named output interpolation, shared budgets, suspend/resume |
| Approval | Approved/pending/rejected/error; no execution before approval |
| Suspension | Stops same-turn remaining tools and subsequent model calls; no response delivery |
| Recovery | Approved retry, quorum continuation, no duplicate committed effects |
| Lifecycle | Timeout, lease loss, cancellation, shutdown, bounded retries, no detached work |
| Accounting | Multiple calls, missing/partial usage, safe errors and diagnostics |
| Product | Save/edit/copy, configured defaults, explicit dispatch, simulation and historical readers |
| Database | Fresh/upgrade migrations, both harness values, bad value rejection |

Parameterize shared harness contract scenarios over both implementations where behavior should
match. Retain provider-specific fixtures for differing message formats, tool-call IDs, and usage.
At least one database-backed task test must enter Rig via production dispatch and finish through
the existing task/message/outbox transaction after a scripted tool-call/result continuation.
Assert both the real tool effect and the stored final response; capture delivery with the existing
recording transport. Run a second scenario through approval/suspension, discard the old runtime,
resume from persisted state, and require the endpoint to observe the saved call IDs/results without
reissuing committed calls. Keep simulator expectations alive across worker restart, independently
of the discarded harness instance. Use both harnesses for shared contracts and Rig-only scenarios
for capabilities that ai-agents does not support.

Use competing database claimants for any new or modified ownership, edit-fencing, resume, or
idempotency protocol. Prove an expired worker cannot commit a tool effect after another claimant
takes over. Sequential mocks do not establish this guarantee. Preserve poison-batch retry tests
if worker error classification changes.

## Required checks

Run against an isolated migrated test database. Prepare metadata against the intended schema;
provide `DATABASE_URL` through the test environment, not copied production credentials.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=false cargo sqlx migrate run
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
SQLX_OFFLINE=true cargo check --locked --all-targets
SQLX_OFFLINE=true cargo clippy --locked --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --locked --all-targets
scripts/transport-boundary-check.sh
scripts/stack-budget.sh
```

Regenerate/commit `.sqlx/` when queries change, rather than including unrelated metadata churn.
`SQLX_OFFLINE=true` means compilation needs no live database for SQL macros; it does not prohibit
Cargo dependency downloads. If verifying dependency-network independence, fetch the lockfile first
and then run `cargo check --locked --offline --all-targets` as a separate check.

Extend the architecture gate to reject `rig`, `rig_core`, and `rig_agent` imports in domain/
application code, following the existing script's treatment of production versus test source.
Do not exempt new adapter imports into inner layers.

Measure the task-worker → Rig chain with `scripts/stack-frames.sh` and retain the stock-stack CI
gate. Ensure its exercised scenarios actually enter Rig. Keep `clippy::large_enum_variant` enabled.
Do not raise `RUST_MIN_STACK`, `RUNTIME_THREAD_STACK_BYTES`, or `STACK_BUDGET_KIB` to hide growth;
shrink/box the chain first. Any justified raised bound needs a recorded reason and an early-failing
CI signal, with platform calibration distinguished from regression.

## Acceptance

All mandatory CI gates pass with both harnesses compiled and exercised. Optional live-provider
smoke results are recorded separately and do not substitute for deterministic contract coverage.
Run bootstrap/default-resolution integration scenarios with the property absent and explicitly set
to ai-agents. Existing ai-agents-specific fixtures must request that harness explicitly instead of
accidentally relying on the old static default.
