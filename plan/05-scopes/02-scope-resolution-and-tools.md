# 02 Scope resolution and tools

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A run's readable units and namespaces are computed from the organization tree once, at admission,
and frozen into the run; the memory tools and steps speak that vocabulary, and knowledge search
([03](03-knowledge-scopes.md)) reads the same snapshot. Continues in [03](03-knowledge-scopes.md).

## Batch 02A Resolution

1. One pure function `run_scopes(chain, agent, roles_held)` in
   `src/application/workflow/scopes.rs` takes the desk's department chain (desk unit first, root
   last), the acting agent principal and the departments where that agent holds `lead`, and
   returns `RunScopes { units: Vec<DepartmentId>, personal: PrincipalId, write_unit:
   Option<DepartmentId> }` per the README's decisions 4 to 6 and 9:
   - units = each in chain ∪ each direct child of a department where the agent holds lead; this
     set is both the memory unit namespaces readable and the knowledge units searchable;
   - write_unit = chain[0], or none when there is no chain;
   - no chain and several departments → units = root only.

   Memory namespaces readable are `Principal(personal)` ∪ `Unit(each of units)`; the default
   memory write is `Unit(write_unit)` or `Principal(personal)` when there is none.

   It has no I/O and is unit tested across every combination, including the depth bound of 8.
2. Admission (`create`, `src/adapters/persistence/workflow/admission_write.rs:7`) computes it with
   the department chain already loaded for role resolution
   ([02-workflow-roles](../02-workflow-roles/02-admission-resolution-and-snapshots.md)) and stores
   it in `workflow_run_scopes (company_id, run_id, unit_id, access)` with `access` in `read`,
   `write`, plus one `personal` row naming the principal, immutable after insert. Later tree
   changes affect later runs only, like every other admission snapshot. Memory namespace ids are
   looked up from the unit and principal at use, so a namespace provisioned after admission becomes
   readable without re-admission.
3. `ScopeAccess` in the memory tools runtime reads that table instead of deriving company, agent and
   user owners per call. Namespaces not yet `ready` are reported `not_ready` as today; a namespace
   missing from the table is `scope_unavailable`.
4. The read ceiling is unchanged: minimum of the admitted run ceiling, the current authorized
   audience and the credential grant. Unit reads do not raise it; a sub-department run with a
   level-3 audience reads root memory at 3.

## Batch 02B Steps and tools

1. `memory.load` / `memory.save` scope literal: `{"unit": "own" | "root"}` or `{"agent": key}`
   replaces `{"company": true}`. `own` is the desk's unit; a company-wide binding has no `own` and
   activation refuses it with `binding.memory_scope_needs_desk`, as role bindings are refused.
2. Agent tools (`MemoryToolScope`, `src/application/workflow/memory_tools/ports.rs:42`): `scopes`
   on search and timeline accept `personal`, `unit` (the desk's unit), `ancestors` (the rest of the
   chain) and `reports` (children readable through a lead role); default is all readable. `scope` on
   save accepts `unit` (default) or `personal`. `company` and `user` are removed; a manual session
   by a person uses `personal` for that person's namespace, under the same own-session rule the
   `user` scope has today (`memory_scope_user_memory_requires_the_actors_own_manual_session`,
   `src/adapters/persistence/workflow/memory_tool_tests.rs:506`).
3. Save accepts an optional `subject`, a principal id the run can see (a thread participant or
   directory entry), recorded as provenance (decision 3). A purge-by-subject operator command lists
   and deletes records across the company's unit namespaces through the service's existing
   correction and deletion operations.
4. Results label each hit with its scope (`unit:<slug>`, `personal`, `reports:<slug>`) so the model
   knows where a fact came from, and the system context for a run on a department desk names the
   unit it will write to.
5. Open question, deferred: an explicit "share up" save (`scope: {"unit": "parent"}`) for a fact a
   sibling department will need. Start strict; if asked for, require the run's agent to hold `lead`
   in the writing department and record the decision here.

## Focused checks

- `run_scopes_resolve_`: unit tests of the pure resolver for every case in 02A item 1.
- `memory_scope_admission_`: with the scripted-model `Deployment` harness, the same agent on two
  department desks reads two different unit chains; a lead reads one step down and a member does
  not; a sub-department run cannot write the root; a direct-message run with one department writes
  that unit and with two writes only its own namespace; moving an agent between departments changes
  the next run and not the current one; the ceiling bounds a root read.
- `memory_scope_tools_`: the new scope vocabulary round-trips through search, timeline and save;
  `company` and `user` are refused; `subject` is recorded and purged; result labels match.
