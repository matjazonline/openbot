# 07UR-P03: Integration and final verification

## Identity and authority
- Workflow implement-opus-reviewed-2_4; phase p03 = Plan07 doc 03 (03A docs, 03B final gate);
  record root plan/07-unit-resources/phases/p03/docs.
- Root/A: Claude Code Opus 5.5/xhigh, session 73d846ee-63bd-49f3-bbdf-7f3d92fcce54. B: Opus
  5.5/xhigh, agent a5e0b0f447b0f21fa (continued from p02).
- Worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan07, phase start 47c8074c (p01 and
  p02 complete in phase, committed locally). Coordination r9 (decisions a, c, d, f–k apply).
- Snapshot exclusions: plan/07-unit-resources/phases/p03/docs, plan/07-unit-resources/COORDINATION.md.
- Preserved untracked (not part of Plan07): the three .trail/learnings/*-mac02.md (user declined
  to commit) and plan/06-delegated-administration/phases/p01/docs/logs/*.snapshot.tar.
- Authorizations: commit locally after an accepted batch; push to main once p01–p03 are complete
  and accepted (user, 2026-11-20). No deployment or shared/production database reset.

## Requirements (doc 03)
- 03A: docs/departments.md gains a resources section; docs/custom_tools.md and
  docs/agent-authoring.md describe unit-scoped selection and the default model cascade;
  docs/workflow-operations.md describes unit budgets beside the root budget. Only behavior the code
  has. Startup plan paths: add plan/07-unit-resources to `NO_BACKWARD_COMPATIBILITY_NOTES`.
- 03B (once, after integration): fresh-schema `.sqlx` regeneration; `scripts/check.sh` on a
  disposable DB with every gate recorded; CI comparison; unavailable prerequisites recorded as
  unavailable; evidence for formatting, whitespace, locked offline all-target compile, clippy,
  fresh migrations, SQLx metadata, source-module reachability, DB suite, network isolation, stack
  and lease budgets; `trail build` (successor to graft build).
- Carried optional items (from p01/p02 reviews): B1-F7 (tests for the department label, the
  root-only "(default)" marker and `prompt_model` with no root default), B2-F2 (budget HTTP 400
  body, cap routes, production message refusal), p01 B3-F1 (an action run as a plain member and
  after that member's removal). Adopted where cheap in B1; the rest recorded.

## Plan
- B1 = 03A docs + startup plan paths + the cheap carried tests; B review.
- B2 = 03B final gate; B combined-integration review; accept; push to main (authorized).

## B1 implementation (03A + carried tests, 2026-11-21; uncommitted on 47c8074c)
- Docs (each claim checked against the code):
  - docs/departments.md: administration table row for unit resources and caps; new "Resources"
    section (ownership and creation default = `resource_default_unit`; reach for agents at
    publication/admission/native selection; workflows at publication, binding save and admission;
    unslotted HTTP resolution among the desk's reach; decision f; one provider per company, unit
    defaults and the cascade; move routes and the move's default rule) and "Spend caps" (decisions
    h–k, attribution to the admitted run's unit and ancestors, `budget.unit_exhausted`, JSON route
    and `cap_micros`, deletion keeps ancestors' counts = `forget_unit_spend` deletes only the
    unit's rows; usage page and dashboard visibility); Delete and JSON refusals name skills and
    connections (lifecycle.rs:241-260).
  - docs/custom_tools.md: unit ownership and reach codes; Builder `create_agent` reach and
    `list_company_agents` model department.
  - docs/agent-authoring.md: the default model cascade and explicit-model reach; library copies'
    fallback to the department default (channel.rs `prepare_library_agent_on`).
  - docs/workflow-operations.md: the root budget (`workflow.root_budget_exhausted`, in-run)
    beside unit caps (admission-time, `budget.unit_exhausted`, recorded on refused schedule
    occurrences and messages); the usage gauge row no longer says no monetary budget is enforced.
- Startup plan paths: src/main.rs `NO_BACKWARD_COMPATIBILITY_NOTES` lists README and docs 01–03 of
  plan/07-unit-resources (all four carry the note); main.rs unittests 6 passed.
- Carried tests adopted: `unit_budget_http_refusal_is_a_bounded_named_bad_request` (B2-F2 400 body),
  `prompt_model_uses_only_the_roots_default` (B1-F7), and in
  `agent_edit_pane_prefills_the_stored_agent_and_offers_delete` the department label (escaped) and
  no "(default)" for a department's own default (B1-F7). Not adopted (recorded): B2-F2's
  production message refusal path and cap route tests beyond the UI card, p01 B3-F1 (action as a
  plain member and after removal) — fixture-heavy, behavior covered at binding save/admission.
- Checks: logs/B1-focused.txt (bins 6, lib 18 passed), clippy clean (logs/B1-clippy.txt),
  `git diff --check` clean. Frozen digest in logs/B1-author-freeze.snapshot.txt; manifest
  logs/07UR-P03-B1-R1.evidence.json.

## B1-R1 corrections (for recheck 07UR-P03-B1-R2, 2026-11-21)
- F1: removed the duplicated plan/07 block from `NO_BACKWARD_COMPATIBILITY_NOTES` (the four paths
  were already listed at HEAD; now once each, main.rs:298-301). Bins tests 6 passed.
- F2: departments.md and custom_tools.md now say native selection checks tools and skills (the
  model only at publication and admission), and that editing an agent offers only what it reaches
  while the new-agent form lists every model connection with its department and refuses an
  out-of-reach one on submit.
- F3 (optional), adopted: move routes for all four kinds with `{"unit_id": …}` (UnitTarget) and
  forms for skills/MCP/HTTP only; library copy → nearest default up the tree; root budget token
  allowance may park for approval; knowledge documents' generic "still referenced" refusal;
  decision f's current-member requirement; at least one model connection; caps deleted with
  their department. Not adopted: messaging/JEV staying with root management (unchanged by Plan07,
  documented on their own pages).
- Frozen 1cbaafd74a01ad78dc18d66bfea5864cb033e0833ff01de1673146ac06f9c66a; manifest
  logs/07UR-P03-B1-R2.evidence.json.

## B1-R2 corrections (for recheck 07UR-P03-B1-R3, 2026-11-21)
- F2 remainder: custom_tools.md now says editing an agent offers no others and a new agent's form
  lists every model with its department and refuses one out of reach. Optional nit adopted: the
  Delete section says the desk refusal only says it still owns desks (lifecycle.rs:237); skills and
  connections are named. Docs only; no check rerun needed (git diff --check clean). Frozen digest
  in logs/B1-R3-author-freeze.snapshot.txt.

## Status
- p03 started 2026-11-21.
- **B1 accepted in phase** 2026-11-21: B PASS 07UR-P03-B1-R3 at
  43532e1f004ff6d7e87b3e69fe0619a1455eaec4c612d4e876886151a027721d (R1 F1–F3, R2 F2 remainder,
  R3 PASS); root re-digest identical (logs/B1-root-acceptance.snapshot.txt). Committed locally.
  Next: B2 = 03B final gate.

