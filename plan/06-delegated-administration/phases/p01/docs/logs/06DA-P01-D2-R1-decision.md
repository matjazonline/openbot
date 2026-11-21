## Proposed next batch B2 — desks and schedules (D2/r1, NOT GATED)

Investigation only until B1 is accepted and B independently passes this decision.
Original01B.2–3; source/JSON shapes stay the same. General definitions/agents remain
the subsequent B3 before the knowledge prerequisite in01B.6;02/UI/audit/final gates
remain ordered after complete01. No Plan05 source or contract is introduced.

1. Extend the existing ChannelPersistence management writes to require the current
   WorkflowActor and explicit company. ChannelUpdate carries its actor; delete
   carries actor/company/desk; create carries actor/company/write. These are internal
   port changes, not JSON fields. No actor is inferred from created_by or ownership.
   Existing internal `_on` insert/update helpers remain trusted transaction helpers;
   each production caller retains its own scoped management/fixture contract.
2. Desk create requires management of the chosen unit (root when absent); update
   requires the current unit and destination when changing ownership; delete
   requires the current unit. Application preflight reuses participant persistence's
   AdministrationPersistence. Every public persistence write rechecks under account
   then company locking before unit/desk/gate locks and its write. Reuse the current
   bounded edit transaction, namespace checks, exact settings revision/grant refresh,
   reviewer refresh and retained-history deletion refusal. A role removal or desk
   move cannot leave stale preflight authority sufficient to commit.
3. Desk configuration reads use management of the actual desk's unit, preserving
   explicit tenant checks. Actual readable-channel/thread/mailbox/simulation/history
   paths keep their own current grant/level checks. List/UI filtering remains02
   unless required for a migrated public owner path; never replace read policy with
   the management predicate. Root-owned model/resource control stays root-only.
4. Schedule persistence create/edit/delete/enable/configuration reuses the scoped
   association management guard over its immutable owning desk, without human View.
   Remove root-only preflights before resolving an existing scoped schedule: load
   within company/channel scope, then authorize its actual desk. List configuration
   filters by those same managed desks; missing/foreign/sibling targets are NotFound.
   Preserve binding compatibility, revision, cadence, occurrence and lease protocols.
5. Schedule run inspection and run-now are distinct from configuration: keep human
   View at application and persistence boundaries where a person reads or initiates
   a run. Actual scheduled admission retains the B1-F1 correction. Configuration
   alone cannot manufacture View or a message claim. Current run-as/resource/source,
   classification and immutable binding checks remain separate and unchanged.
6. Update every affected port implementation/caller; focused compile drives fixture
   literals. Existing DB fixtures may use a test-only helper that explicitly obtains
   their real company's owner actor and calls the same production checked write;
   never infer an actor in production or retain unchecked public company mutators.

**B2 verification mapping (planned):**

| Criterion | Production path / decisive assertion | Focused checks |
| --- | --- | --- |
| Desk owner | Use-case/API and actual persistence create/update/reassign/delete: managed target succeeds; sibling/foreign and direct bypass refuse; both units needed on move; no effect on refusal | delegated_admin_desk_, channel CRUD/edit/access-mode tests and channel authorization routes |
| Schedule owner | Real create/update/enable/pause/delete under delegated current unit; direct adapter sibling/foreign refusals; immutable channel and compatibility preserved | delegated_admin_schedule_, schedule management/API/UI authorization families |
| Privacy / initiation | Manager without View can configure; cannot inspect history/simulate/run-now/admit a schedule; granted person retains existing behavior | current channel visibility/read/foreign tests, schedule run/history tests and B1-F1 regression |
| Concurrency | Actual role removal competes with migrated desk/schedule writer under company protocol; refused post-revocation with no partial state | delegated_admin_competing_ plus current edit/settings/occurrence competing tests |
| Port/SQL/integration | All implementations and downstream consumers accounted for; current source inventory retained; no unchecked public company writes | locked offline all-target check, focused tests, source_modules, SQLx prepare, format, clippy, whitespace; B1 affected regressions only |

Trace pointers: logs/B2-owner-callers.txt, B2-channel-edit-callers.txt and
B2-root-authority-callers.txt; source ownership and public configuration-vs-read
callers were inspected before this proposal. Exact B2 frozen identity and final
consumer dispositions will be recorded only after implementation. Complete final
repository gate remains at the full-plan boundary and is not claimed here.

