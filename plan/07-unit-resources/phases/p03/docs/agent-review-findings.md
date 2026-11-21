# 07UR-P03 agent review findings

## 07UR-P03-B1-R1 findings

Context for P03-B1-F1 … F3: 2026-11-21, workflow implement-opus-reviewed-2_4, task
[TASK.md](TASK.md) "B1 implementation", review [REVIEW.md](REVIEW.md), round note
[07UR-P03-B1-R1](agent-review-notes.md#07ur-p03-b1-r1-2026-11-21); batch B1 = doc 03A
(documentation, startup plan paths, carried optional tests), batch review. Reviewer B Claude Opus
5.5 (claude-opus-5-5). Phase 07UR-P03, worktree /Users/mac02/orca/workspaces/openbot/plan06
(plan07). HEAD 47c8074c plus the working tree, digest
d30f98a13277aab7980f5bd3bddc8ac2f666becca082adc8f21182970dd2e62e (exclusions
plan/07-unit-resources/phases/p03/docs, plan/07-unit-resources/COORDINATION.md). All findings are
in the implementation.

## 07UR-P03-B1-F1 — The startup notes list now carries every plan/07 path twice

- **Classification:** actionable defect; **low**; correctness (root AGENTS "startup notes list").
- **Evidence:** HEAD 47c8074c already lists the four plan/07 entries in
  `NO_BACKWARD_COMPATIBILITY_NOTES` (`git show HEAD:src/main.rs`, lines 298-301). B1 adds the
  same four again (src/main.rs:302-305), so `serve`'s startup warning (main.rs:401,
  `notes = ?NO_BACKWARD_COMPATIBILITY_NOTES`) lists README and docs 01–03 twice. TASK records
  this as adding the plan's paths. No test checks the list for duplicates, so main.rs's unit
  tests (6 passed) cannot show it.
- **Required:** drop the duplicated block; the requirement was already met at HEAD. Optionally
  add a uniqueness assertion over the list.
- **Instruction lesson:** check the current state before adding an entry an earlier phase may
  already have added.
- **Resolution history:** 2026-11-21 open (P03-B1-R1).
  - 2026-11-21 recheck 07UR-P03-B1-R2 (digest 1cbaafd7…c66a): **fixed**. src/main.rs is unchanged against HEAD, with the four plan/07 entries listed once (lines 298-301). Bins tests: 6 passed (logs/B1-R2-bins.txt).

## 07UR-P03-B1-F2 — The docs claim checks and offers the code does not make for models

- **Classification:** actionable defect; **low**; requirements (03A "Name only behavior the code
  has").
- **Evidence:**
  - docs/departments.md "Resources → Agents" says an agent's "tools, skills and model" are
    checked "when the agent is published, when a run that uses it is admitted, and whenever a
    native session selects its bundle". docs/custom_tools.md likewise says "A native session's
    bundle is checked the same way". The model reach check (`check_agent_model_reach_on`) is
    called only from publication capture (agent_publication/capture.rs:88) and the admission
    manifest (agent_manifest/capture.rs:88). The native paths (local_agents/native.rs:26, 114)
    call only `check_agent_reach_on`, which covers tools and skills. True for tools and skills;
    not for the model.
  - "Authoring offers only what the agent can reach" (departments.md) and "authoring offers no
    others" (custom_tools.md) hold for an existing agent's offers. For a new agent, the create
    form lists every company model connection, labelled with its department
    (`list_company_model_connections(.., None)`, use_cases/agent.rs), and refuses an
    out-of-reach choice only on submit.
- **Required:** narrow the two sentences (native selection checks tools and skills; a new agent's
  form offers every model, labelled, and refuses one out of reach at creation), or add the
  missing check.
- **Instruction lesson:** none new; this is the 03A rule.
- **Resolution history:** 2026-11-21 open (P03-B1-R1).
  - 2026-11-21 recheck 07UR-P03-B1-R2 (digest 1cbaafd7…c66a): **partly fixed; still open**.
  - 2026-11-21 recheck 07UR-P03-B1-R3 (digest 43532e1f…721d): **fixed**. docs/custom_tools.md:16-21 now says editing an agent offers no others, and that a new agent's form lists every model with its department and refuses one out of reach. That matches `list_company_model_connections` (filtered only for an existing agent) and `resolve_model_on`'s reach refusal.
    - Fixed: native selection is narrowed to tools and skills in both docs, and departments.md now states the new-agent form's behavior (every model, labelled, refused out of reach on submit).
    - Residual: docs/custom_tools.md:16-19 still says, for "HTTP connection, MCP connection, model connection and skill", that "authoring offers no others". That contradicts departments.md and the code for a new agent's model choices.
    - Required: one clause, e.g. "editing an agent offers no others (a new agent's form lists every model and refuses one out of reach)".

## 07UR-P03-B1-F3 — Small documentation precision and completeness points (optional)

- **Classification:** optional suggestion; **low**.
- **Evidence:**
  - Move routes (departments.md) list JSON moves for MCP/HTTP/model connections and "the skill
    move form". The skill JSON move (`/api/companies/{company_id}/skills/{skill_id}/move`) and the
    MCP/HTTP move forms exist too, and the move body (`{"unit_id": …}`) is not named. The response
    names only uses the mover manages and counts the rest.
  - agent-authoring.md: a library model out of reach "is replaced by that department's default".
    It is the cascade's nearest default from the department up, which may be an ancestor's.
  - workflow-operations.md: the root budget is "enforced during the run, failing it". A token
    allowance can instead park the run for reviewer approval.
  - departments.md Delete: "the refusal names what it still owns". That holds for skills,
    connections and desks; knowledge documents surface as the generic "still referenced"
    conflict (FK). Workflow definitions and role history also block a delete (pre-existing).
  - Not mentioned:
    - decision f's requirement that the run's actor is still a company member;
    - messaging and JEV staying with root management;
    - a department's caps being deleted with it;
    - a company always keeping at least one model connection.
- **Resolution history:** 2026-11-21 open (P03-B1-R1, optional).
  - 2026-11-21 recheck 07UR-P03-B1-R2 (digest 1cbaafd7…c66a): mostly adopted and checked against the code:
    - all four JSON move routes with `{"unit_id": …}` (`UnitTarget` in each handler) and forms for skills, MCP and HTTP;
    - the nearest-default library fallback;
    - root-budget token-allowance parking (`decisions::tokens::park`);
    - the knowledge-document refusal wording;
    - decision f's current-member requirement;
    - the ≥1-connection rule;
    - caps deleted with their department (FK CASCADE).
    Messaging/JEV not adopted (recorded). One new nit, optional: "The refusal names the desks" — the desk refusal says only "still owns desks" (lifecycle.rs:237) without naming them; skills and connections are named.
  - 2026-11-21 recheck 07UR-P03-B1-R3 (digest 43532e1f…721d): the desk-refusal nit adopted: departments.md now says the refusal "says it still owns desks, or names the skills and connections it still owns", matching lifecycle.rs:237-260. **Closed.**

## 07UR-P03-B2-R1 findings

Context for P03-B2-F1 … F3: round note
[07UR-P03-B2-R1](agent-review-notes.md#07ur-p03-b2-r1-2026-11-21), batch B2 = doc 03B final gate
plus the combined-integration review of Plan07 (p01 + p02 + p03 B1). HEAD b154b3cc plus the
working tree (assets/app.css, TASK), digest
f53289cf382cb4fc2d08a61d9de50a1336f4b04f93f826e0c9f7517ff8e1f564, exclusions unchanged.
Coordination revision 9. Plan07 diff base 5bf6a669. Reviewer B Claude Opus 5.5.

## 07UR-P03-B2-F1 — Doc 03's completion record is still "Pending" on every row

- **Classification:** actionable defect; **low**; requirements (03B "completion record").
- **Evidence:** plan/07-unit-resources/03-integration-and-final-verification.md's "Completion
  record" table is unchanged at HEAD and in the working tree: all seven rows read "Pending". The
  gate results exist only in p03 TASK.md's gate record. Doc 03 ends with "Do not declare the
  implementation complete while a required check is unavailable"; root AGENTS.md says to
  "explicitly report unavailable checks, and do not claim full verification when required checks
  could not run".
- **Required:** fill each row with its actual status and evidence pointer, keeping unavailable
  and deferred items as such:
  - 01 and 02 focused checks: passed (p01, p02 records);
  - formatting, offline compilation, clippy and reachability: passed;
  - fresh migrations and SQLx metadata: passed;
  - database-backed full suite: red only on the deferred Doop assertion;
  - Linux network-isolation scope: not exercised;
  - stack budget: clean signal, red only on that assertion; lease budget: passed;
  - Docker image gates: unavailable;
  - managed Doop: deferred and red;
  - docs, startup paths and graph refresh: passed.
  Plan06 did the same in its post-acceptance record.
- **Instruction lesson:** the final-gate batch should treat the plan document's completion table
  as a deliverable, not only the phase TASK.
- **Resolution history:** 2026-11-21 open (P03-B2-R1).
  - 2026-11-21 recheck 07UR-P03-B2-R2 (digest c2876c0d…ec6e): **fixed**. Doc 03's completion record now carries each row's actual status, matching the retained evidence:
    - passed: the 01/02 focused checks, formatting/compilation/clippy/reachability, migrations/SQLx, the lease budget, and docs/startup/graph;
    - red only on the deferred Doop assertion: the DB suite and the stack budget;
    - Linux isolation not exercised;
    - Docker unavailable;
    - Doop deferred.
    It ends with "accepted in phase, not declared complete".

## 07UR-P03-B2-F2 — Plan07 cannot be declared complete in doc 03's sense; the push condition needs the user's confirmation

- **Classification:** actionable; **medium**; process/authorization (for root; it does not
  concern the artifact's code).
- **Evidence:**
  - The final gate ran (logs/final-check.txt, EXIT 1). These required gates are unavailable or
    not passed:
    - three Docker production-image gates: unavailable;
    - Linux network-isolation scope: not exercised on macOS;
    - managed Doop: deferred and red, 42 failures under host Chrome;
    - the database-backed suite and the stack budget: red only on the deferred Doop assertion
      (05SC-C001/04RU-C005).
    Doc 03 item 4 lists network isolation among the required evidence, and its last line forbids
    declaring completion while a required check is unavailable.
  - The coordination file's authorization reads: "push to main once all of Plan07 (p01–p03) is
    complete and accepted; not before." In Plan06's case, the push came on an explicit user
    request ("push to main"), after the record stated "accepted in all three phases but NOT
    declared complete" (plan/06-…/p03/docs/TASK.md "Post-acceptance").
- **Required:** record Plan07 as accepted in all phases but not declared complete, listing the
  gates above, as Plan06 did. Before pushing, obtain the user's confirmation that the
  authorization covers that state, or follow the Plan06 precedent of a push on the user's
  explicit request. Do not describe the push as following completion.
- **Instruction lesson:** a conditional push authorization should name the completion standard
  ("accepted" versus doc 03's "complete").
- **Resolution history:** 2026-11-21 open (P03-B2-R1).
  - 2026-11-21 recheck 07UR-P03-B2-R2 (digest c2876c0d…ec6e): **fixed**. TASK records Plan07 as accepted, not declared complete, and the push waits for the user's explicit confirmation (requested 2026-11-21). B has no reason against the fast-forward push once the user confirms.

## 07UR-P03-B2-F3 — Combined-integration and pre-push observations (optional)

- **Classification:** optional suggestion; **low**.
- **Evidence:**
  - Company teardown with p02 data (a child unit owning a model connection, caps, totals and
    attributed usage) is verified on inspection only. The company cascade removes
    `company_model_connections`, `unit_spend` and usage before `delete_company_tree` deletes the
    departments, after which `unit_budgets` cascades and `forget_unit_spend` finds nothing.
    `unit_resource_schema_delete_names_what_to_move_and_teardown_keeps_working` (p01) covers
    skills and connections only.
  - Managed Doop shows 42 failures here against 1 in Plan06's run. Plan07 changes nothing in
    integrations/, services/, scripts/ or CI (`git diff 5bf6a669..HEAD` empty there), and the
    failing tests are Doop's own browser cases (elementResize "reading 'deselect'", drawing, the
    known privatePdfRendering). The environment attribution (host Chrome instead of CI's
    `DOOP_TEST_UI_CHROME`) is plausible but unverified. Compare with CI's run after the push.
  - Before the push: run `trail check` (as Plan06 did), commit the CSS fix with B2, and keep the
    preserved untracked files out. The rebase onto fa58df8a is conflict-free (no file overlap
    with Plan07; Plan07 never edited src/main.rs). After it, confirm the startup list has each
    plan/07 path once.
- **Resolution history:** 2026-11-21 open (P03-B2-R1, optional).
  - 2026-11-21 recheck 07UR-P03-B2-R2 (digest c2876c0d…ec6e): dispositions recorded in TASK "B2-R1 corrections": the CSS fix committed with B2; `trail check` before the push; a one-entry check of the startup list after the rebase; the Doop CI comparison after the push; the teardown test not added (recorded). **Closed.**
