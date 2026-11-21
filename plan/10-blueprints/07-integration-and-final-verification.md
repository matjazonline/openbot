# 07 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

Implement phases 01 through 06 in order after the accepted 01–09 sequence. Integrate all phases
before deploying the feature. Use the reset schema: `blueprint_applications` is added to the init
schema and no existing row is converted. Before starting, record here the outcome of each item
under "Proposed here, confirm before implementation" in the [README](README.md); batches that
depend on an unconfirmed proposal say so. All phase check families are proposed tests, not
evidence of implementation today.

## Batch 07A Complete journeys and regressions

Use scripted providers and real database workers in isolated fixtures.

1. **Build for a goal.** A person asks the Builder in the goals office for a team. The Builder
   reads the unit, plans, and calls apply; the run parks and nothing exists. The head of the
   office approves. The goal unit, its lead agent, its sponsor, its desks, the binding on its own
   desk and its budget exist from one commit, with one application row naming the head as actor
   and the Builder as proposer. A message to the goal's desk then runs 08's plan, approve and
   dispatch procedure; the runs carry the goal and their spend counts against the unit's cap.
2. **Change.** A manager exports a unit, edits one prompt and adds one member, and previews
   exactly two operations. An edit elsewhere in the company before apply does not make the plan
   stale. An edit to the same agent does, and the new preview shows why.
3. **Recreate.** A company's root is exported with units, agents, skills, desks, rooms, carried
   procedures, bindings and schedules. In a fresh database a new company applies it and exports
   an equal document. Before its connections and people exist, the plan lists every one of them
   as an unknown reference in one answer; after, it lists none. `omitted` names what did not
   travel.
4. **Share.** A document exported in one company is applied in another under a unit its importer
   chose. The importer's edits are ordinary text edits; no parameter mechanism is involved.
5. **A builder cannot exceed its approver or its unit.** In turn: a message that tells the
   Builder to grant itself a tool; a document that targets a sibling unit; one that opens a desk
   to the public; one whose only reviewer role is held by an agent; an approver who does not
   manage the target; a plan gone stale while parked. Each ends with no write, and each is
   visible as a refusal, a disabled approval or a refused apply.
6. **Concurrency and recovery.** The competing cases of [03](03-apply.md) batch 03E and
   [05](05-builder-agent-and-approval.md) run together with two workers: one application per
   approved invocation after a worker loss, no partial setup after an injected late failure, and
   no leaked lock order between an apply and a product write.
7. **Regressions.** Creating an agent, a desk and a department in the product; the
   assistant-channel route; protected-action approval for an HTTP tool; `decision.human`; 06's
   authorization matrices; 08's archive refusals, including an apply into an archived unit; 09's
   room access; `list_company_agents`.

Use `blueprint_journey_` plus each changed interface's focused regression filters, checking
nonzero test counts and recording commands and results before the final gate.

## Batch 07B Final full-suite gate, once after integration

1. Finish init-migration edits using fresh disposable databases; never reset a shared database.
   After SQL changes, run `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets` against the
   migrated disposable database and retain the generated `.sqlx` metadata.
2. Run `scripts/check.sh` with `DATABASE_URL` explicitly set for this task's disposable database.
   Record each gate, including failures; compare its entry points with
   `.github/workflows/ci.yml` and execute required gates it omits.
3. Evidence must include formatting, patch whitespace, locked offline all-target compilation,
   clippy, fresh migrations, SQLx metadata, source reachability, the full database-backed suite,
   network isolation, stack and lease budgets, frontend protocols and the required deployment and
   service checks.
4. Bounds. Keep every threshold unchanged. This plan adds bounds (document bytes, unit, agent,
   skill and desk counts, refusal and result sizes) and each has a test that fails at the bound.
   If an existing ceiling had to rise to admit a document, name what now fails early in CI
   instead, as the root `AGENTS.md` requires; the expected answer is that none rose
   ([01](01-document-and-export.md), batch 01B item 2). `scripts/stack-budget.sh` passes at the
   stock stack with the depth-8 document in the suite.
5. Rerun affected checks after verification fixes; repeat the full suite if the impact warrants
   it. Mark missing browser, Docker, model or other prerequisites unavailable, never passed. Do
   not claim complete verification with required gates missing or failing.
6. Run `python3 scripts/check-integration-docs.py`, check every new plan and document link, and
   verify all eight plan paths occur exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`. Run
   `trail build` after the implementation changes and `trail check` before any authorized
   publication.

## Completion record

| Evidence | Status |
| --- | --- |
| Proposals 1–5 of the README: confirmed, changed or declined | Pending |
| 01 document type, bounds, digest, export and its authorization | Pending |
| 02 plan matrix, digest, references and preview authorization | Pending |
| 03 apply, owners, application record, idempotency and competing writers | Pending |
| 04 bindings, schedules, carried procedures and extended round trip | Pending |
| 05 read tools, protected apply, approver authority, lock order and competing approvals | Pending |
| 06 department page, samples, documentation and field consistency | Pending |
| Complete journeys and product regressions | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Full database-backed suite and network isolation | Pending |
| Stack and lease budgets, bound tests and remaining required CI gates | Pending |
| Startup plan paths, links and graph refresh | Pending |

Plan-writing checks validate these documents and their startup inventory only. They do not change
the pending implementation evidence above.
