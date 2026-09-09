# Step 8 — SOP Execution Bindings and Sample Runs

## Outcome and dependencies

Run installed workflows through the SOP engine and test candidate revisions without customer
effects. Depends on [step 7](07-atomic-installation.md) and the complete SOP foundation, including
structured results, human review, task transfer, cancellation, and reconciliation.

## Production execution binding

1. Add `workflow_run_bindings` with company/run/installation/published-revision IDs and the
   binding hash. Create it atomically with the SOP run and first tasks. Enforce matching company,
   installation, revision, and procedure version. Workers follow the run's pin, not the
   installation's current active revision or global catalogue pointer.

2. Freeze effective agent execution configuration at revision publication: system instructions,
   supported harness configuration, resolved model/provider choice, setup data, and per-step
   capability ceilings. Add a shared `AgentExecutionSpec` consumed by harness adapters rather than
   a second agent runner. SOP publication owns skill snapshots; reference those snapshots instead
   of maintaining conflicting copies in the installation. Resolve permitted role replacements to
   explicit execution specs when admitted to a run.

3. Configuration pinning does not freeze company authority or external data. At start, resume,
   tool invocation, and result commit recheck current principals, account status, model access,
   agent/tool policy, channel/thread visibility, and connection data grants. Intersect current
   authority with the pinned per-step ceiling. Old skill instructions cannot restore a revoked
   tool; a required missing capability parks through SOP configuration-exception handling.
   Ordinary prompt edits do not rewrite an active run; deleting a bound agent or withdrawing
   relevant authority blocks further use. Record live source values and freshness separately
   from configuration.

   Task transfer and role rebinding still use their existing commands. Validate a new agent
   owner's compatibility with the pinned step instructions, harness, and capability ceiling;
   transfer cannot silently select a different runtime configuration or widen data access.

4. Keep connector record IDs, setup inputs, task handoff text, and previous results as bounded,
   delimited data. A connector read returns provenance usable as SOP result evidence. Submit
   structured results through the existing lease/owner/occurrence/run fences; a natural-language
   final answer never completes a step or automatically becomes an outbound reply.

5. Keep human review and external delivery separate. The reviewer authorizes an exact immutable
   artifact and recipient/action payload through existing review/approval commands. A later
   delivery step publishes that artifact through the canonical message/delivery transaction and
   waits on its authoritative delivery state. Retries reuse the same logical delivery. Changed
   content or recipients require a new review; a review-form checkbox cannot grant tool authority.
   Surface provider-unknown or terminal delivery failure through existing exceptions.

## Sample execution

6. Add a shared, persisted `ProcedureExecutionMode` distinction for production and simulation;
   this is a capability of the SOP execution adapter, not a second graph engine. Regular
   production start routes reject test-only procedure versions. Legacy production runs retain
   their behavior; the new tagged mode must be recognized explicitly at all effect boundaries.

7. A draft cannot be executed as though it were a published production version. Capture a bounded
   immutable `WorkflowValidationSnapshot` with candidate hash, compiled procedure, execution
   specs, selections, example inputs, and validator version. Publish its procedure under an
   isolated test-only procedure identity using the normal validator, then start a simulation SOP
   run. Link the run to this snapshot through a typed alternative to the production revision
   binding. The draft remains editable, but edits cannot alter the test's frozen inputs.

8. Use the same task ownership, transition engine, result schemas, and human-review forms. Replace
   connector reads with synthetic fixtures and outbound delivery/outreach/write capabilities with
   recording adapters. Enforce simulation mode in application/tool/delivery boundaries; omitting
   a Send button or adding a prompt instruction is insufficient. Never persist synthetic
   conversation facts into company/user production memory, enqueue real notifications to external
   people, invoke unwrapped native tools, or mutate CRM records. Real connection readiness remains
   the separate authenticated read check from step 5.

9. Store sample outcome, result evidence, validation findings, reviewer, candidate hash, and time.
   Require a schema-valid completed sample and a manager's review before initial activation or
   an executable update. This proves the sample path only; do not claim general correctness.
   Refresh/repeat when the candidate or relevant authority changes. A successful sample is not
   permission to send a customer message in production.

10. Bound sample admission using existing task budgets plus at most 2 concurrent samples/company
    and a 24-hour sample expiry including human waits. Apply provider execution deadlines within
    that window. Expired samples are cancelled through SOP commands; retain evidence for 30 days
    or the company's stricter retention policy. Cleanup never removes production run pins.

## Verification and acceptance

- Run the same fixture in production with fake external adapters and in simulation; compare
  transition/result behavior, while only production may enqueue an authorized real delivery.
- Attempt external outreach, native tools, memory persistence, notification and delivery bypasses
  from simulation and prove they are blocked or recorded as synthetic effects.
- Race review with artifact changes, result submission with transfer/cancellation, and connector
  revocation with a pending read result. Reuse SOP concurrency tests and add the new binding cases.
- Edit a reused agent, skill, package, or active installation revision mid-run; old execution
  configuration remains pinned while current revocations still block unauthorized work.
- Accept when a candidate can be tested end to end and later production uses the reviewed
  configuration through the existing SOP/task/delivery engines.
