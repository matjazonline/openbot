# 02WR-P02-C1-R1 — Department framing timing clarification

2026-11-11, Reviewer B /root/workflow_roles_reviewer; narrow read-only contract assessment requested by root during partial B1 implementation. This is not an implementation review, new design authorship, or acceptance of current B1 source. HEAD103004a8ac2fd643cf14bde7e09022ae79f72053; scoped inspected file hashes in 02WR-P02-C1-R1-read-identities.json. Whole source is not frozen for this assessment and no whole-artifact digest is asserted. Coordination revision5, I001-I003/r1, C001/r1 and active C002/r2; pin02WR-P02-C1-R1-coordination.md.

## Disposition

Preserving the established department-framing preparation timing is justified. P02's frozen-role contract applies to authoritative role resolution: selected principal, resolved department, agent revision, use mode and authority/grant selection. Existing informational department context is captured as part of the first frozen agent preparation and replayed from that preparation thereafter. It must not select or replace the agent/revision, confer tools/grants, or substitute current holders for immutable role rows.

The blanket README sentence that nothing in a running run dereferences a mutable role table is literally broader than the preserved implementation and must not be used as a literal verification claim. Root should explicitly record this boundary in p02's requirement/coordination disposition and qualify equivalent D1 wording. Preserving the already accepted timing does not itself require a new schema/transaction architecture or a fresh implementation gate. Moving this framing to admission would be a separate behavioral change outside the current preservation disposition.

## Requirement evidence

- Workflow-roles README decisions2/8 describe freezing the resolved principal and current published revision. The same README allows live authority events and separately opts into human reassignment, so its last immutability bullet already requires contextual interpretation rather than a universal ban on mutable organization reads.
- Original p02 02B.2 explicitly says everything after the agent lookup remains unchanged: frozen revision prompt/tools/grants and live authority checks. It does not request an admission snapshot of the department roster or desk framing.
- Accepted prerequisite user decision01DEP-C003/r1, dated2026-11-10 (later than workflow-roles plan decisions2026-11-09), places the department context/roster after the agent's own prompt and captures it once in frozen preparation. Plan01 p03 TASK D11 and accepted verification mapping explicitly implement/test step-preparation capture and frozen retry. This accepted prerequisite was carried into p02; no later user instruction supplied here replaces its timing.

## Actual source and observable boundary

- agent_history/preparation.rs:220-265 loads the authorized selection first, builds PreparedAgent from it, then calls department_context_on with prepared.agent_id(). Its scope follows the desk; it does not choose an agent from the roster.
- department_roster.rs:239-278 reads current desk ownership, charter and whether the already selected agent currently leads; roster_on:283-353 reads current member/role placements for display addresses. These are live role-table reads at initial preparation.
- application/workflow/agent.rs:277-287 and499-518 show with_department changes only the contextual preamble/department field; preamble begins with the selected published revision's own instructions and uses that selection's skills. No selected agent, revision or grant is replaced.
- agent_history/frozen.rs:15-51 restores the saved department before reconstructing/comparing frozen preparation, preserving retry context despite later changes.

Observable consequence: a role reassignment after admission but before first preparation can change the roster or lead sentence seen by the already frozen agent. It cannot change which agent/revision executes. Once preparation is frozen, retry uses the saved context. Therefore p02 tests should distinguish selected identity/revision instructions from the entire assembled contextual preamble; claiming all prompt bytes were frozen at admission would be false. Existing preparation/retry regression assertions remain applicable.

No source edits, tests or database checks run. No actionable implementation finding raised on this unchanged framing path. The disposition qualifies prior D1/r2 role-table wording; it does not extend that architecture PASS to current partial B1 code.
