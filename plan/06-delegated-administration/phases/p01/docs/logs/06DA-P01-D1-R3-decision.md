## 06DA-P01-D1/r3 — model source authority after actual runtime counterexample

Frozen first integrated inputba5e79b9…ddd48 ran515tests:506PASS/8FAIL/1ignored,
216.93s after4m14build. Seven new hierarchy/owner/lead-transition/revocation tests
passed; new native Deployment admitted the run but failed at the draft agent step
with task_attempts.workflow_failure_code=agent.rejected. Read-only bounded repro
retained actual executions/events/attempts in B1-native-repro-* logs; initial state
sampling missed the disposable DB teardown and is retained as unavailable evidence.
No passing runtime assertion is claimed. Existing failures include intended root
role404/private-configuration policy changes and a now-misplaced company-port outage
stand-in; retain/refactor their assertions at the responsible current boundary.

Root cause traced: agent_history/preparation.rs::locked_on and
agent_preparation.rs::current_on still require the *human run-as* conversation View
at the model boundary, even for an owned automatic department reply whose actual
selected agent has its own valid desk grants. Simply dropping those checks would
also let a manual session continue after its person's grant was revoked. That would
violate the preserved manual/history policy; it is explicitly ruled out.

**D1/r3 decision (adds to r2; independent PASS required before affected edits):**

- Configuration remains person-scoped unit management. Actual human desk/thread/
  history reads and manual model sessions retain their current grant requirements.
  The existing human association authorization path still succeeds exactly as before.
- Introduce a narrowly owned model-source authority helper in agent_preparation,
  used by both initial locked agent preparation and the current checkpoint guard.
  Resolve the frozen execution selection and verify current agent definition/use
  authority first. Require current person management of the run association under
  the existing account/company protocol. Attempt the existing human association
  View guard; errors propagate, except an explicit NotFound may enter the following
  narrow automatic-agent proof path. No generic association read helper changes.
- The fallback proves an actual channel_reply_runs owner joined to its immutable
  channel_reply_context and actual workflow_run: exact company/run, actor, binding,
  channel/principal and thread match; top-level native automatic reply ownership.
  Manual, observer, schedule and unrelated runs do not inherit it. A current
  same-company agent principal must match the already verified frozen selected agent.
  For a desk, lock that principal and its actual View and Participate grant rows
  (both required) in deterministic order before accepting the agent's source access.
  A selected agent replying on its own direct principal target uses that exact
  self-inbox identity; other direct principals do not inherit access. Missing,
  foreign, changed or revoked witnesses refuse. Direct grant revocation is fenced
  by row SHARE locks; scoped management is separately fenced by the company lock.
- This is server execution as the authenticated selected agent over a proven owned
  reply, not human conversation visibility. It cannot authorize config on behalf
  of an agent lead, change a person's level, or expose history APIs. The helper
  does not create a View grant or add a new permission vocabulary.
- Keep agent manifest/revision/use-mode, actual role and credential eligibility,
  source proof, classification/consumption, checkpoint/lease and native sorted-account/
  NOWAIT contracts. Initial preparation is reordered only to obtain/validate its
  selected agent before the model-source authority decision; company still precedes
  agent/grant and checkpoint gates. Current model guard reuses the same helper after
  restoring and checking the exact selection identity, avoiding diverging decisions.

**Decisive verification for r3:** the existing real native department journey must
produce/deliver its exact output with the delegated lead run-as while the human
read rule remains false; separately remove the selected agent's View/Participate
witness and assert provider calls/output remain0. A manual private session whose
human grant is revoked must still refuse; an observer/manual/schedule run cannot
manufacture owned reply proof. Retain existing revoked-agent/publication, read/
history and native-MCP current-authority/race suites. Root-only resources remain
separate. Architecture PASS does not waive any of these execution obligations.
