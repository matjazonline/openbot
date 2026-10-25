# P08-D3 rev 1: production action-reconciliation composition and route (B3b-2)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Gate for B3b-2. It is held because it adds a production authorization composition across the
workflow, resource (HTTP, MCP, messaging) and delivery subsystems; that is costly to unwind once
the route and the p09 UI depend on it.

## Sources

- Plan `08-triggers-and-application-replacement.md`, "Public interfaces": company-scoped routes
  over the owning commands, including action reconciliation. They must keep command semantics
  identical across entry points, and authorize the referenced resource with its channel/thread.
- P08-D2 rev 2:
  - §D2.0 lists action reconciliation in B3b;
  - §D2.2a item 4: provisioning is not reconciliation-eligible.
    `ProvisioningActionAuthority` does not implement `SqlReconciliationResources`, so a request
    naming a provisioning action fails closed.
- TASK "B3b plan": the B3b split; reconciliation is composed only in test support today.
- The existing reconciliation contract (04.x), unchanged by this gate:
  - `ActionReconciliationService` (application/workflow/actions/reconciliation/service.rs);
  - `PostgresActionReconciliation<A: SqlReconciliationResources>`
    (adapters/persistence/workflow/action_reconciliation/);
  - `ReconcileActionCommand`, `EvidenceInput`, `ReconciliationOutcome`.
- Coordination r9: WF-C004 r2 (guarded pools; every transaction here uses the persistence pool).

## Facts this decision rests on

1. **The service.** `reconcile` runs in this order:
   1. `request_digest` validation;
   2. owner/admin preflight (`WorkflowOperation::Reconcile`, Company);
   3. the stored association, read by the persistence, then authorized;
   4. `snapshot` (first bounded transaction);
   5. the frozen scope and argument digest are checked against the command;
   6. `ResourceDirectory::inspect` of the action's resource under the command actor;
   7. `validate_resource`;
   8. for a replay, the recorded result; otherwise the snapshot and revision checks, then the
      evidence, then `settle` (second bounded transaction).

   The overall budget is capped at 5 s.
2. **The persistence.** `PostgresActionReconciliation` already takes `authorize_company`
   (company row FOR NO KEY UPDATE, member and principal FOR SHARE) in each transaction before the
   run head. `SqlReconciliationResources::lock_resource(tx, actor, frozen)` runs inside both
   transactions, after the run and execution locks.
3. **Implementations today.**
   - `MessageActionAuthority` (messaging, the company-owned resource) is the only production
     implementation of `SqlReconciliationResources`.
   - HTTP and MCP have test-only implementations.
   - `PostgresPersistence` implements `ResourceDirectory`.
   - `resources::inspect_on(tx, company, id)` resolves messaging (id = company), HTTP connections
     and MCP connections. The connection reads are `FOR SHARE`, and a connection of another
     company is not found.
4. **No production verifier.** There is no production `ActionEvidenceVerifier`. The prior
   phases' rule is that metadata and operator claims never permit replay; the MCP
   reconciliation test says so too.
5. **Identifiers.** The marker is `workflow_action_dispatches.id`. The subject is
   `(invocation_id, argument_digest)`. A message-delivery subject is
   `workflow_message_destinations (invocation_id, ordinal, delivery_id)`.
6. **When a run awaits reconciliation.** A run awaits reconciliation exactly when it is
   `waiting` with `waiting_reason = 'reconciliation'`. Two things put it there: recovery
   (`RecoveryDecision::Reconcile`, an effect outcome unknown), and the evidence-conflict trigger,
   which fails the pending job. Only verified `AppliedRecorded { receipt: true }` or
   `NotAppliedRecorded` reopen a job (settlement.rs). A proven-not-applied message delivery
   returns to the delivery queue (`DeliveryReopened`) and never reopens a job.

## Decision

### D3.1 Resources: `CompanyReconciliationResources`

A new unit struct in `adapters/persistence/workflow/action_reconciliation/resources.rs`
implements `SqlReconciliationResources`. `lock_resource(tx, actor, frozen)`:

1. `company = frozen.action.scope().company`.
2. It reads the frozen target:
   - `Local { resource_kind = messaging }`: delegate to `MessageActionAuthority`'s
     implementation. It checks that the target is the company itself and authorizes the company
     under the command actor.
   - `Local` of any other kind (provisioning included): **NotFound**. This fails closed, per
     D2.2a item 4.
   - `Connection { resource, resource_kind }` with kind `http` or `mcp`: first
     `authority::authorize_company(tx, company, actor)`, which re-runs the company/principal check
     on the transaction's own connection; it is idempotent with the transaction's earlier call.
     Then `resources::inspect_on(tx, company, resource)`, which returns the resource's current
     status with the connection row `FOR SHARE`; a missing resource is NotFound.
   - Any other connection kind: NotFound.
3. **The actor is the command actor.** It is not the frozen run actor: recording past truth
   requires current access, as the trait documents.
4. **Lock order:** company → run/execution (the persistence's head) → connection (`FOR SHARE`).
   This is the dispatch path's order (`SqlActionAuthority::lock_current` for HTTP/MCP). A
   connection update (enable, secret, operation registration) locks the company first and then
   the connection (`begin_http`/`authorize_company`), so no cycle is introduced. B to verify.

### D3.2 Verifier: none in production

`ActionReconciliationService::new(.., verifier: None)`.

- Only `EvidenceInput::UnknownNote` can settle. The outcome is `UnknownRecorded`, an audited
  operator annotation with the note and the claimed disposition. A `VerifiedReference` is refused
  by the service (400, `invalid()`).
- Consequence, stated for B to challenge: a production run waiting on reconciliation can be
  annotated and then cancelled through `POST …/workflow-runs/{r}/cancel`, but not resumed.
  `RetryResult::Unsafe` stays unsafe for it, which matches facts 4 and 6.
- A provider-backed verifier is a later decision. No configuration key and no stub are added,
  and the docs must not promise verification.

### D3.3 Engine composition

`WorkflowEngine::reconciliation()` returns `ActionReconciliationService<
PostgresActionReconciliation<CompanyReconciliationResources>, ProductionAuthorizer,
PostgresPersistence>` with verifier `None`. The route calls `reconcile(&command, &token, 5 s)`:

- `token` is a fresh `CancellationToken`, never cancelled. Dropping the request future drops the
  operation;
- each persistence step is its own bounded transaction with 5 s lock and statement timeouts,
  already set by `PostgresActionReconciliation`.

### D3.4 Discovery read: what a run needs reconciled

`RunDetail` gains `actions`, at most 200 with `actions_truncated`, newest dispatch first. They
come from the run's `workflow_action_dispatches` for executions not yet completed, joined to the
dispatch's intent. Each carries:

- `execution_id`, `invocation_id`, `argument_digest`, `marker_id` (the dispatch id),
  `effect_kind` (`remote`/`message`/`local`), `attempt_number` and `created_at`;
- `evidence_conflict` (`workflow_action_has_evidence_conflict`);
- for `message`, the `destinations` (`ordinal`, `delivery_id`) from
  `workflow_message_destinations`, bounded by `MAX_MESSAGE_DESTINATIONS`.

The listing grants nothing. It shows the run's state and `waiting_reason` as today, and
eligibility stays the command's own snapshot and settlement checks.

### D3.5 Route

`POST /api/companies/{c}/workflow-runs/{r}/reconciliations` takes:

```json
{"execution_id", "invocation_id", "argument_digest", "marker_id",
 "target": {"kind": "remote"} | {"kind": "message_delivery", "ordinal", "delivery_id"},
 "expected_revision", "command_key",
 "evidence": {"kind": "unknown_note", "note", "claimed": "applied"|"not_applied"|"unknown"}}
```

- The run comes from the path; company and run come only from the path.
- `ApprovalSubject` is built from `invocation_id` and `argument_digest`; the digest is parsed with
  its own validation.
- The command key is an `IdempotencyKey`.
- The answer is 200 `{outcome, revision, evidence_id, replayed}`. The outcome is serialized as
  its kind, plus the blocked reason when blocked.
- Errors map as the app does: 400 for invalid evidence or a refused verified reference, 404 for
  unknown or foreign items or no access, 409 for conflicts.
- `AppError::Timeout` keeps the app's existing timeout mapping. `cancelled` cannot happen,
  because the token is never cancelled.
- **The child rule:** a child run's actions are reconciled through the child itself. This
  command records evidence about the child's own frozen action and touches no parent control. The
  route therefore does **not** apply `controllable_run`. B to challenge.
- `deny_unknown_fields`; the note is bounded by `MAX_EVIDENCE_NOTE_BYTES` (the command validates
  it); the router body limit applies.

### D3.6 Invariants

1. Every reconciliation authorizes the current command actor four times:
   - owner/admin of the company (the service preflight, and in each transaction);
   - the run's stored association (channel/thread visibility);
   - the action's resource under that actor, for connections and messaging;
   - provisioning and unknown local kinds never.
2. No verifier means no reopened job, no receipt and no replayed action in production.
3. The command is idempotent by key. The same key with a different request is refused by the
   existing command namespace (`request_digest`).
4. Company and run come only from the path; every body id is checked against the frozen snapshot
   by the existing service and persistence.
5. No schema change, and no change to the reconciliation contract.

### D3.7 Failure modes

- **Lost authority between the transactions** (demotion, connection deleted, channel access
  revoked): the second transaction re-authorizes and refuses with nothing granted. This is the
  existing contract.
- **Concurrent reconciliation of one action:**
  - the same key replays;
  - different keys serialize on the run/execution locks, and the second sees a changed revision
    (409) or a recorded outcome.
- **Reconciliation against a cancel or completion:** the existing settlement guards hold; only
  waiting/reconciliation jobs reopen, never terminal runs.
- **A provider-backed claim attempted** (a `VerifiedReference`): 400, nothing recorded.
- **Budget exhausted:** `AppError::Timeout`, and each transaction rolls back.

### D3.8 Affected contracts

- **New:**
  - `CompanyReconciliationResources`;
  - `WorkflowEngine::reconciliation()`;
  - the run-detail `actions` read: `ActionSummary` in `application/workflow/reads.rs`, and the
    persistence read;
  - the route and its body types.
- **Unchanged:** `ActionReconciliationService`, `PostgresActionReconciliation`,
  `SqlReconciliationResources`, `MessageActionAuthority`, the evidence schema, and the
  settlement.
- **WF-C003 / WF-C002:** unaffected, since there is no child-control change (D3.5).

## Verification plan (B3b-2)

- **Fixture:** a crate-visible `#[cfg(test)]` support module, for example
  `persistence/workflow/reconciliation_test_support.rs` (as `schedule_test_support` is). It
  produces an HTTP action whose outcome is unknown through the existing path: publish → bind →
  claim → `prepare_step` → dispatch marker → retire with `EffectOutcomeUnknown`. The run is then
  waiting with reason `reconciliation`. A message-delivery variant exists if the existing delivery
  fixtures allow it cheaply; otherwise that target is covered by the persistence reconciliation
  tests that already exist.
- **Route tests**, on the route tests' own databases:
  - the run detail lists the action with its marker;
  - an unknown note records `UnknownRecorded` (200) and the run stays waiting;
  - the same key replays (`replayed: true`); the same key with another note → 409;
  - a stale `expected_revision` → 409;
  - a `VerifiedReference` → 400, with no evidence row;
  - a stranger (own and foreign company paths) and a plain member → 404; an admin kept off the
    run's allowlist channel → 404;
  - another tenant's run or marker id → 404;
  - a provisioning target is refused by a direct `lock_resource` test through
    `CompanyReconciliationResources`, since provisioning never becomes uncertain;
  - competing claimants: two concurrent reconciliations of one action with different keys; one
    applies and the other gets 409 or a recorded outcome.
- **Unit:** `CompanyReconciliationResources` dispatch by target kind; the body → command mapping.
- **Per-batch focused checks:**
  - the new route tests;
  - the existing reconciliation suites:
    - `action_reconciliation_authority_tests`, `mcp_reconciliation_tests`,
      `action_reconciliation_snapshot_tests`;
    - the message delivery reconciliation tests;
  - `scripts/stack-budget.sh` on the new tests;
  - clippy and fmt.
- The final full-suite gate is the user's call at the end of the phase.
