# P08-D3 rev 2: production action-reconciliation composition and route (B3b-2)

Rev 2 answers WF-P08-R016, F065–F070:
- F065: D3.1 dispatches by resource kind, whichever form carries it;
- F066: D3.5 has the outcome→status table, the fresh-key rule and the ordering;
- F067: D3.4 adds reconcilability facts;
- F068: D3.2a adds the readiness decision;
- F069: the verification plan is rewritten;
- F070: timeouts answer 503, and p09 labels the control.

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
2. It decides by the frozen target's `resource_kind`, whichever form (`Connection` or `Local`)
   carries it. Dispatch does the same: `http_target` accepts both forms, and an HTTP operation
   published without a slot is stored as `Local { resource_kind: "http" }`.
   - `messaging` (`Local`): delegate to `MessageActionAuthority`'s implementation. It checks that
     the target is the company itself and authorizes the company under the command actor.
   - `http` (`Connection` or `Local`) and `mcp` (`Connection`): first
     `authority::authorize_company(tx, company, actor)`, which re-runs the company/principal check
     on the transaction's own connection; it is idempotent with the transaction's earlier call.
     Then `resources::inspect_on(tx, company, resource)`, which returns the resource's current
     status with the connection row `FOR SHARE`; a missing resource is NotFound.
   - `provisioning`, and every other kind in either form: **NotFound**. This fails closed, per
     D2.2a item 4. Provisioning markers always carry receipts and are `local`, so none is ever
     uncertain.
3. **The actor is the command actor.** It is not the frozen run actor: recording past truth
   requires current access, as the trait documents.
4. **Lock order:** company → run/execution (the persistence's head) → connection (`FOR SHARE`).
   Dispatch (`prelock_on`), both reconciliation transactions and connection edits (`edit_on`) all
   lock the company row `FOR NO KEY UPDATE` first, so they serialize there (R016 confirmed this).

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

### D3.2a Readiness (F068)

The existing service checks, in both transactions, that the action's resource is authorized,
**Ready**, and still supports the frozen contract (`validate_resource`). Otherwise the result is
`NotFound` ("Workflow action authority"). Rev 2 keeps that contract unchanged:
- annotating an action on a disabled connection, one missing its bearer secret, or one whose
  operation was unregistered is refused with 404, the same answer as no access;
- the run can still be cancelled; annotation works again once the resource is ready;
- the `SqlReconciliationResources` doc line "does not require current I/O approval" refers to the
  approval policy, not readiness. B3b-2 corrects that line so it says so.

Rejected alternative: relaxing readiness for reconciliation would change the 04.x service
contract that the verified path relies on. With no production verifier (D3.2) the only loss is
an annotation that waits for the resource. Revisit this when a verifier is added.

### D3.3 Engine composition

`WorkflowEngine::reconciliation()` returns `ActionReconciliationService<
PostgresActionReconciliation<CompanyReconciliationResources>, ProductionAuthorizer,
PostgresPersistence>` with verifier `None`. The route calls `reconcile(&command, &token, 5 s)`:

- `token` is a fresh `CancellationToken`, never cancelled. Dropping the request future drops the
  operation;
- each persistence step is its own bounded transaction with 5 s lock and statement timeouts,
  already set by `PostgresActionReconciliation`.
- **F070:** the workflow routes answer `AppError::Timeout` with **503** and `Retry-After: 1`,
  through `WorkflowError::into_response`, instead of the app-wide 500. Lock contention is
  retryable, and the per-statement timeouts equal the whole 5 s budget.

### D3.4 Discovery read: what a run needs reconciled (F067)

`RunDetail` gains `actions`, at most 200 with `actions_truncated`, newest dispatch first. They
come from the run's `workflow_action_dispatches` with `effect_kind` `remote` or `message`.
`local` markers are left out, because no reconciliation target names them. Each carries:
- `execution_id`, `invocation_id`, `argument_digest`, `marker_id` (the dispatch id),
  `effect_kind`, `attempt_number` and `created_at`;
- `has_receipt`: a `workflow_action_receipts` row exists for the marker;
- `evidence_conflict`: `workflow_action_has_evidence_conflict`, computed once per distinct
  execution of the page, not per marker;
- `reconcilable` for `remote`: no receipt, and the run is `waiting` with reason
  `reconciliation`;
- for `message`, `destinations`: one row per `workflow_message_destinations` entry (bounded by
  `MAX_MESSAGE_DESTINATIONS`), with `ordinal`, `delivery_id`, the delivery `status` and `parts`.
  A destination is `reconcilable` only while its status is `outcome_unknown` and it has one part,
  which are the command's own subject conditions (delivery.rs).

The listing grants nothing, and eligibility stays the command's own snapshot and settlement
checks. Bounds: 200 markers × at most 20 destinations.

### D3.5 Route

`POST /api/companies/{c}/workflow-runs/{r}/reconciliations` takes:

```json
{"execution_id", "invocation_id", "argument_digest", "marker_id",
 "target": {"kind": "remote"} | {"kind": "message_delivery", "ordinal", "delivery_id"},
 "expected_revision", "command_key",
 "evidence": {"kind": "unknown_note", "note", "claimed": "applied"|"not_applied"|"unknown"}
           | {"kind": "verified_reference", "registration", "reference"}}
```

The `evidence` field is the command's own `EvidenceInput` shape, passed through unchanged, so
the command decides everything about it, a verified reference included (refused without a
verifier, D3.2).

- The run comes from the path; company and run come only from the path.
- `ApprovalSubject` is built from `invocation_id` and `argument_digest`; the digest is parsed with
  its own validation.
- The command key is an `IdempotencyKey`.
- The body field is `delivery_id`, which the route maps to `ReconciliationTarget::MessageDelivery
  { ordinal, delivery }`. The route body is its own type; the command's serde shape is not
  exposed.
- **Outcomes are results, not errors (F066).** The command records a refusal under the key and
  returns it as an outcome. The route answers every outcome with the body
  `{outcome, revision, evidence_id, replayed}` and this status:

  | Outcome | Status | Meaning |
  | --- | --- | --- |
  | `unknown_recorded`, `applied_recorded`, `not_applied_recorded`, `scheduled`, `delivery_reopened` | 200 | recorded. Only the first is reachable without a verifier |
  | `revision_conflict` | 409 | the run changed after the caller read it; recorded under the key |
  | `idempotency_conflict` | 409 | the key was used for a different request |
  | `evidence_conflict` | 409 | the action already has conflicting evidence |
  | `blocked { reason }` | 409 | not reconcilable in its current state (`stale_snapshot`, `terminal`, `ineligible_job`, …); recorded under the key |

  Errors that are not outcomes:
  - 400: invalid input, including a `VerifiedReference` at the current revision, which records
    nothing;
  - 404: unknown or foreign ids, no access, or a resource that is not ready (D3.2a);
  - 503: timeout (D3.3).
- **Fresh-key rule.** The request digest includes the expected revision and the evidence, and a
  refusal is recorded under its key. A client that gets 409 must re-read the run and send a **new
  key**: retrying the old key with a corrected revision answers `idempotency_conflict`. A replay
  of the same request under the same key returns the recorded outcome with `replayed: true`.
- **Ordering, as the existing command does it:**
  1. the digest checks;
  2. the company preflight;
  3. the association;
  4. the snapshot: a stale revision or a key reused for a different request is recorded here;
     note that a `VerifiedReference` with a stale revision is therefore recorded as
     `revision_conflict`;
  5. the resource readiness check;
  6. the evidence: a fresh-revision `VerifiedReference` is refused with 400 and nothing recorded;
  7. the settlement.

  Each evidence insert advances the run revision (trigger `workflow_action_evidence_revision`).
  So of two concurrent commands with different keys, one records its outcome and the other gets
  `revision_conflict` or `blocked { stale_snapshot }`, depending on the interleaving.
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
  - **The same key, concurrently:** the command namespace's uniqueness serializes the two. One
    records and the other replays it (`replayed: true`), or both observe the one recorded outcome.
  - **Different keys:** both pass the company lock in turn. One records; the other then fails the
    revision check (`revision_conflict`, recorded under its key) or the settlement's snapshot
    check (`blocked { stale_snapshot }`).
- **Reconciliation against a cancel or completion:** the existing settlement guards hold; only
  waiting/reconciliation jobs reopen, never terminal runs.
- **A provider-backed claim attempted** (a `VerifiedReference`): 400, nothing recorded.
- **Budget exhausted:** `AppError::Timeout` → 503, and each transaction rolls back.
- **The resource stopped being ready:** 404 (D3.2a); cancel is the path.

### D3.8 Affected contracts

- **New:**
  - `CompanyReconciliationResources`;
  - the `WorkflowError` timeout mapping (503 with `Retry-After`) for the workflow routes;
  - `WorkflowEngine::reconciliation()`;
  - the run-detail `actions` read: `ActionSummary` in `application/workflow/reads.rs`, and the
    persistence read;
  - the route and its body types.
- **Unchanged:** `ActionReconciliationService`, `PostgresActionReconciliation`,
  `SqlReconciliationResources`, `MessageActionAuthority`, the evidence schema, and the
  settlement.
- **WF-C003 / WF-C002:** unaffected, since there is no child-control change (D3.5).
- **p09 (pending notice):** the UI labels this control an annotation ("record what happened"),
  not a resume. A run waiting on reconciliation shows cancel and rerun as its recovery. This is
  published as a coordination notice when B3b-2 is accepted.

## Verification plan (B3b-2) (F069)

Every test below goes through the **production composition** (`WorkflowEngine::reconciliation()`
or the route over it), never a test double of `SqlReconciliationResources`.

- **Fixtures:** a crate-visible `#[cfg(test)]` support module,
  `persistence/workflow/reconciliation_test_support.rs`, as `schedule_test_support` is. Through
  the existing paths it produces an action whose outcome is unknown, with the run waiting with
  reason `reconciliation`:
  - an HTTP action published **with a slot** (`Connection { http }`);
  - an HTTP action published **without a slot** (`Local { http }`);
  - an MCP action (`Connection { mcp }`);
  - a message delivery left at `outcome_unknown` (`Local { messaging }`), through the existing
    delivery test support.

  If one of these cannot be built cheaply, that form is covered by a direct `lock_resource` test
  of `CompanyReconciliationResources` on a stored frozen action, and the gap is reported.
- **Target forms (F065):** each of the four forms above records `unknown_recorded`. A
  provisioning target and an unknown kind are refused by `lock_resource` with NotFound (direct DB
  tests). Five forms in all.
- **Outcome mapping (F066)**, asserting exact outcomes, rows and revisions:
  - an unknown note → 200 `unknown_recorded`; one evidence row; the run revision advances; the
    run stays `waiting`/`reconciliation`;
  - the same request with the same key → 200 `replayed: true`, and no new row;
  - the same key with a different note → 409 `idempotency_conflict`;
  - a stale `expected_revision` under a new key → 409 `revision_conflict`, recorded;
  - the corrected revision under that same key → 409 `idempotency_conflict`;
  - under a fresh key → 200;
  - a `VerifiedReference` at the current revision → 400 and no row; at a stale revision → 409
    `revision_conflict`.
- **Competing claimants:**
  - two concurrent commands with different keys, from the same read revision: exactly one
    `unknown_recorded`; the other is `revision_conflict` or `blocked { stale_snapshot }`; exactly
    one `unknown` evidence row; the final revision is the read revision plus one per recorded
    row;
  - two concurrent commands with the same key and request: one evidence row, and both answers
    carry the same outcome, one of them `replayed`.
- **Readiness (F068):** a disabled HTTP connection, and an unregistered operation, each give 404
  for an otherwise valid annotation and record nothing. Re-enabling the connection gives 200.
- **Tenancy:**
  - a stranger (own and foreign company paths) and a plain member → 404;
  - an admin kept off the run's allowlist channel → 404;
  - another tenant's run or marker id → 404.
- **Discovery (F067):**
  - the run detail lists the remote marker as `reconcilable` with `has_receipt: false`;
  - the message destination lists its `outcome_unknown` status and one part;
  - no `local` marker is listed.
- **Timeout mapping (F070):** a unit test that `WorkflowError::App(AppError::Timeout(..))`
  answers 503 with `Retry-After`.
- **Per-batch focused checks:**
  - the new tests;
  - the existing reconciliation suites, for no regressions: `action_reconciliation_authority_tests`,
    `mcp_reconciliation_tests`, `action_reconciliation_snapshot_tests` and the message delivery
    reconciliation tests;
  - `routes::workflows`;
  - `scripts/stack-budget.sh` on the new tests;
  - clippy `-D warnings` and fmt.
- The final full-suite gate (plus `npm run check:css`) is the user's call at the end of the phase.
