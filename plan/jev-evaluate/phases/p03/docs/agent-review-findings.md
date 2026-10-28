# JEV p03 reviewer findings

## JEV-P03-D1-R1 — 2026-10-28

No actionable defects or optional suggestions established for JEV-P03-D1/r1. Architecture PASS; implementation and checks remain outstanding.

## JEV-P03-B1-F1 — final authorization precedes capacity queue

- Date/workflow/round: 2026-10-28, implement-astra-reviewed-2.2, JEV-P03-B1-R1. Reviewer /root/phase_03/reviewer, requested Astra/medium (actual host identity not exposed), p03, /Users/mac02/dev/openbot, main; base/HEAD 5a12ebec76c76c97e50b2a51ebacd66712db5820; digest `85ec488ab95d95a07799404883a3b3777d56c38d18b2a9c3417ebf75f621b903`. TASK.md / REVIEW.md.
- Classification: actionable defect, high severity, authorization/correctness. Open; acceptance blocking.
- Evidence: application/workflow/jev_runtime.rs:207–219 obtains the ephemeral credential before calling evaluate; adapters/workflow_jev/mod.rs:56–74 then waits for the four-slot semaphore before constructing and sending the bearer request. No authority/key refresh follows this wait. Configured workflow worker slots can be 16 (infra/workflow.rs:61), so this queue is reachable in the production composition once p05 enables it.
- Reproduction by control flow: occupy four HTTP permits, start a fifth runtime invocation and allow its final authorization to finish; revoke its selected connection/remove the model or rotate its key while it waits; release a permit. It sends with the previously accepted authority and old key. Lease renewal does not recheck connection authority. The existing capacity deadline test verifies this wait exists; rotated-key test changes the key before runtime starts and cannot cover it.
- Required correction: place the final fenced current-use authorization after capacity admission, immediately before outbound use, while retaining one overall deadline/cancellation and bounded concurrency. Add a deterministic queued-request revocation/model-removal and key-rotation regression proving no forbidden send and use of the current key. A brief unavoidable race after final authorization is not the issue; the deliberate potentially 30-second queue after authorization is.
- Requirement/instruction: C002/r1 final provider-use authorize_jev and D1/r1 current key/authority invariant. The architecture did not require this ordering; this is an implementation error. Candidate lesson: when reviewing current-use authority, trace internal transport capacity waits as well as the outer call site. No instruction edit authorized.
- Resolution: open at B1 review.

## JEV-P03-B1-F2 — new tenant-scoped ledger foreign keys lack required rejection test

- Date/workflow/round: 2026-10-28, implement-astra-reviewed-2.2, JEV-P03-B1-R1. Same reviewer/worktree/base/HEAD/digest as F1; TASK.md / REVIEW.md.
- Classification: actionable verification gap, medium severity. Open; acceptance blocking repository requirement.
- Evidence: migrations/20260817000000_init_schema.sql:11302–11303 introduces composite foreign keys from workflow_jev_invocations to executions and exact model usage provenance. The new runtime/recovery tests exercise successful admission and stale ownership but contain no attempted invalid cross-tenant ledger insert. Exhaustive rg over persistence *tests.rs for workflow_jev_invocations finds only counts, successful output read and secret-free outcome read. Fresh migration success proves the DDL is accepted, not the negative relational invariant.
- Required correction: add the rollback-only/transaction-scoped database test explicitly required by src/adapters/persistence/AGENTS.md, showing that a ledger row using another tenant's execution/usage identity is rejected by the new constraints. Also exercise mismatched same-tenant run/execution/request provenance while covering this new composite relationship.
- Instruction lesson: existing guidance is sufficient; checklist omission, not evidence for a new rule. This is not an allegation that the inspected FK columns are wrong.
- Resolution: open at B1 review.

## JEV-P03-B1-R2 resolutions — 2026-10-28

Reviewed correction artifact `33e9963bb9ed70b8f2a183367fd54d6a4f4d44031b54d2549bd7d743b730e5d8`, same base/HEAD/worktree as B1-R1.

- **JEV-P03-B1-F1 FIXED.** Application JevProvider::acquire now grants a consumed JevProviderSlot before admission and final authorize_jev. The HTTP-owned semaphore permit remains held through the actual send; HttpSlot::send contains no capacity wait. No DB transaction is held across capacity acquisition; original persistent lock ordering is preserved. One absolute deadline is passed through acquire/evaluate, preexpired calls reject, and dropping the slot/future releases capacity. Retry-After deferral explicitly drops the slot. Reqwest protocol retries are disabled with retry::never. The deterministic queued test holds all permits, waits for actual acquisition notification, then rotates the key/disables the connection/removes the model before releasing capacity; it verifies current-key dispatch or terminal refusal with no reservation. Independent 44-test Jev run passes, including prior cancellation/deadline/recovery tests.
- **JEV-P03-B1-F2 FIXED.** jev_runtime_ledger_rejects_cross_tenant_and_mismatched_provenance attempts foreign execution, foreign usage, same-tenant usage mismatch and same-tenant run mismatch in rollback-only transactions and requires FK SQLSTATE 23503. Same-tenant conflicting ledger identity is removed only transactionally so the PK does not mask the provenance check. Independent focused run includes this passing test. Binding-scoped fixture idempotency key permits the distinct same-company runs required for the test.

Both findings closed by independent inspection and execution, not by author claim. No remaining actionable findings for this artifact.
