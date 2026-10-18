# User-stop handoff — phase 05A, 2026-10-18

No backward compatibility is required; reset the database when a schema change does not
apply cleanly. Pre-release, the init schema is edited in place (see
`src/adapters/persistence/AGENTS.md` §1).

**Stopped at the user's requested safe checkpoint. Further work requires a new
continuation instruction.**

## Workflow and roles

Workflow `implement-opus-reviewed-2.1`. Root could not confirm xhigh effort, so it stays a
thin coordinator. Author A and reviewer B were Opus/xhigh subagents of the stopped
session; a fresh root must start fresh workers (skill recovery rules), never reuse handles.

## Authorized scope (user, 2026-10-18)

Migration squash, then phase 05 (05A, 05B, phase 05 gate). Not authorized: commits,
deployment, production data, phases 06+.

User decisions:
- Reviewers are explicit only: a list of 1–16 company members. No groups.
- No external (email-only) reviewers and no decision credentials in phase 05.
- Email-link approvals are dropped together with the legacy approval code in phase 08.
- A timeout is Terminal `decision.timed_out` over the declared `error` route. With no
  route, the run fails.
- Production wiring stays in phase 08.
- Self-approval is allowed: any listed owner or admin, including the run actor.
- Migrations are squashed. `src/adapters/persistence/AGENTS.md` §1, the composite-FK
  bullet and the credential paragraph were amended to the pre-release in-place model.
- The startup warning flags the model for removal.
- Plans 05–10 are added to `NO_BACKWARD_COMPATIBILITY_NOTES`.

## Accepted

- **Batch S (migration squash):** root-accepted on S-R003 PASS, digest `03ca0bea…cbae`,
  base `feefe3e`. Gap: the stack-budget command exited 101 on timing-bound tests only,
  with no overflow; accepted on current-tree evidence. See TASK.md, "Batch S ROOT ACCEPTED".
- **Phase 05 architecture gate:** 05-R004 PASS for the effective decision in
  `docs/logs/05/`:
  - D1 `4e3a46ebb03137ba5c86e1b3d64e1d3b0abf756c54eaa59efc8cee8301707931`
  - D2 `831890270a87273b2cbbccf580a942c4582841cc2c4430b15d542d95754815b3`
  - D3 `bd937b8630e48e6c7e2bf0bfeb223cd879004e4c3cfb898bb302b0d432af0b69`
  - D4 `e8ed88c46e1420e3ededa3da33b282a8e6c9cefc9495e3146a420ed77fe1fc86`

  05-F001…F010 are resolved. Deferred: the phase 07 timeout-code mapping, phase 08 routes
  and wiring, and bounding member email length.

## Unfinished 05A (WIP, unreviewed)

WIP snapshot: base `feefe3e060a851fd8860e4bacb90c11c7b284888`, digest
`c90f78c3733ccb02ffb7c66b474efc8cd335f1b05334f33abc368a863454f52d`. Root recomputed it
and it matches. Uncommitted changes: the S AGENTS.md/`main.rs` edits plus the 05A work.

- **Done:** every first-edit item in D2 §15, i.e. the plan texts, EXPANSION 05.3,
  NOT_PLANNED, REPLACEMENT-MAP §4, the notes list and the warning wording.
- **Implemented, compiles, mostly untested:**
  - the language (`reviewers`, the static-binding check, event payload schemas);
  - the init-schema tables and guards;
  - the application decisions and event ports;
  - persistence for park, settlements, submit, comment and timeout;
  - the run-scoped event inbox.
- **Compile state:** `cargo check --all-targets` passes with one dead-code warning
  (`DecisionNotices::unrenderable`).
- **Checks:** the focused language tests ran 69 passed, 1 failed. The failure is
  `workflow_registry_each_type_validates_dynamic_required_inputs`, which still binds the
  old `reviewer` field; log `logs/05/05a-focused-language.log`. No DB-backed tests or gates
  have run.
- **Not started:** all new 05A tests and the registry test-pin updates.
- **Deviation awaiting B:** the static-reviewer rule rejects `/input`, so the phase 07
  placeholder fixture `human-revision-round.yaml` now takes its reviewer from
  `/params/reviewer`. This leaves phase 07 an obligation: child rounds must get
  reviewers from binding parameters, or phase 07 needs a language change.

Full detail is in TASK.md under "05A USER STOP".

## Resources at stop

- PostgreSQL 18.6 fixture `/private/tmp/workflow0410-20261017`, system id
  `7697669855432439122`, 127.0.0.1:55450, role `workflow0410`. Stopped with
  `pg_ctl -m fast` and its data kept. It carries CI's `pg_stat_statements` settings.
- Databases `workflow0410_test` and `workflow0410_fresh_test` must be reset, because the
  init schema was edited.
- `workflow0410_presquash` and `workflow0410_postsquash` are batch S evidence.
- Root verified that no cargo, rustc, test or postgres process is running.
- Disk is about 7.7 GiB free (97% used).
- The host shows intermittent failures in the lease/dispatch/reconciliation timing test
  family under load. Report them, never count them as passes.

## Resume order

1. Read RESUME, this handoff, the latest TASK/REVIEW entries, D1–D4 and the 05-R001…R004
   notes and findings.
2. Recompute the WIP digest and reconcile the worktree, preserving everything.
3. Start fresh A and B workers (opus/xhigh).
4. Start the fixture, reset both test databases, and migrate.
5. Fix the registry test pins, then the focused language tests, then the focused DB
   filters.
6. Write the new 05A tests (`decision_tests.rs`), run the full gate list, and freeze.
7. B reviews the whole 05A batch, including the `/params` fixture deviation.
8. Root accepts 05A, then 05B (per D2 §16 and D3), then the phase 05 gate.
