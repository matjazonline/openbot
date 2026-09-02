# Step 18 — Finish Lifecycle, Observability, Failure Testing, and Rollout

## Outcome

Ship the peer transport as an operable system: all workers are supervised through shutdown,
operators can see/recover stuck work without reading secrets or bodies, CI exercises the failure
model, and rollout has measurable stop/go gates.

## Runtime ownership

- Construct Slack client/registry/integration use cases in `src/infra/setup.rs` only when the
  all-or-none Slack config is present. Absence disables Slack install/event routes cleanly while
  email remains healthy.
- Store only narrow use-case/port handles in `AppState`; never store plaintext bot tokens.
- In `src/main.rs`, supervise inbound-event, delivery, unknown-reconciliation, binding-drift, and
  retention loops with retained handles and the existing shutdown broadcast.
- Stop admission first, cancel active provider/agent work, release or leave short recoverable
  fenced leases, and join every loop within the drain budget. Log exactly which component exceeded
  shutdown; do not detach nested work.
- Ensure database pool sizing accounts for all worker concurrency and dedicated listeners. Add
  explicit acquire/statement/provider timeouts rather than increasing the pool blindly.

## Operational surfaces

- Extend readiness to report required database connectivity and, separately, Slack configuration
  state without calling Slack on every probe. Liveness stays unconditional/cheap.
- Add authorized integration/binding views: status, workspace/conversation safe identifiers,
  scopes, last verification, queue counts/ages, last classified error, reauthorization state, and
  dead letters. Never show tokens, ciphertext, signatures, raw payloads, or full message bodies.
- Generalize outbox UI to transport/purpose/status and show multipart progress plus
  `outcome_unknown` risk. Manual retry/reconcile/dead-letter actions require manager scope, CSRF,
  exact company ownership, reason, and audit event.
- Metrics cover inbox/delivery state transitions, attempts, lease loss, oldest-ready age, provider
  latency, 429 wait, unknown reconciliation, binding drift, and per-transport throughput. Avoid
  unbounded workspace/conversation labels in metric cardinality.
- Add bounded retention sweepers for completed inbox payloads, OAuth states, audit data according to
  policy, and provider diagnostic metadata. Deletion never removes canonical messages/maps needed
  for loop suppression.

## CI and test matrix

Update `.github/workflows/ci.yml` so it continues to run formatting, locked offline compilation,
migrations, the full Postgres-backed suite, Clippy, deployment-script tests, and stack budget. Add:

- `scripts/transport-boundary-check.sh` from step 11;
- fresh-schema migration, reset/bootstrap, and final-schema assertion tests;
- local Slack stub integration tests requiring no external credential/network;
- concurrent inbox and delivery claimants;
- lease loss during active Slack request;
- crash before/after every transaction/API boundary;
- ambiguous provider outcomes and poison full batches;
- reply-before-root and delayed/out-of-order events;
- cross-tenant installation/binding/map attempts;
- public/shared/unauthorized conversation link attempts;
- partial multipart delivery and dependent-root failure;
- 429/`Retry-After`, conversation/workspace limits, and tenant fairness; and
- shutdown during active ingress/send/reconciliation with no orphan work.

If async refactoring grows stack frames, use `scripts/stack-frames.sh` to locate the growth and
shrink/box the correct seam. Do not raise `RUST_MIN_STACK`, `RUNTIME_THREAD_STACK_BYTES`, or
`STACK_BUDGET_KIB` without the repository-required early-failure calibration and documented reason.

## Rollout

1. Confirm and record that each target environment may be destructively reset, stop all writers and
   workers, reset the database, run the complete migration set, and bootstrap only final-model data.
   There is no rolling upgrade, data import, queue drain, or rollback to the old schema.
2. Deploy Slack code with install/event routes disabled by absent config; verify email regression,
   generic queues, dashboards, and resource use.
3. Configure a separate Slack test app/workspace and one non-sensitive private conversation. Enable
   one company/binding; exercise roots, replies, reordering, mirrors, agent replies, restarts, 429,
   token revoke, unlink, and unknown-outcome recovery.
4. Observe queue age/error/duplicate indicators through a defined soak window. Define numeric abort
   thresholds before the soak, including oldest-event age, dead-letter count, unknown-outcome age,
   and message duplication.
5. Expand by company behind a persisted feature flag. Pausing a binding is the immediate kill
   switch; it must not disable email or delete history.
6. Publish operator/user documentation for installation, private-channel grant, supported message
   types, data retention, unlink/revoke, reauthorization, rate limits, and non-exact-once edge cases.

## Final acceptance criteria

- One business channel can safely use email and one or more Slack interfaces over the same
  canonical thread/message history.
- Adding another chat provider requires provider config/client, identity parser, renderer/sender,
  OAuth/install adapter as applicable, and registration—no canonical schema redesign.
- Every review-required failure scenario is automated and green in CI.
- Production documentation names only enforced limits/configuration and makes no unsupported
  validation or exactly-once promise.
