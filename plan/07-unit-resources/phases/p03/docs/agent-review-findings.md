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
