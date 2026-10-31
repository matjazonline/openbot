# P07 handoff — session 2 (2026-10-22)

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Identity and records

- Phase `p07`, plan `plan/workflow-foundation/07-child-workflows-and-review-revisions.md`,
  workflow `implement-opus-reviewed-2_2` (skill dir
  `/Users/mac02/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Records: `/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p07/docs/` — TASK.md
  (authoritative requirements, batches, decisions, checks, status), REVIEW.md /
  agent-review-notes.md / agent-review-findings.md (B-owned), `logs/decision-D1.md` (rev 1),
  `-r2`, `-r3`, `logs/decision-D1-r4.md` (rev 4, passed, the B1 contract), `logs/coordination-r3.md`.
- Root/A: this session, Opus 5.5/xhigh (user-confirmed 2026-10-22). B: agent
  `a5e5548971a73e411` (opus/xhigh, fresh context); rounds R002–R004 (gate PASS); idle, continue
  it with `SendMessage` for the B1 batch review. Session-1 B `a46b56e8565db0c84` is unreachable. A fresh root starts a new B
  from `references/reviewer-assignment.md`.
- Worktree `/Users/mac02/dev/openbot`, branch `workflows`; phase start `921079d`; review base
  `2d608f2` (user commit with session-1 records). p07 (B1+B2) committed as `bd2ef36` and pushed
  to `origin/workflows`.
- Snapshot exclusions (exact): `plan/workflow-foundation/phases/p07/docs
  plan/workflow-foundation/COORDINATION.md marketing website`. Digest at `2d608f2` without
  edits `31c52a4a…1b21`; **B1 review digest `1e1b0a032b4d93b16d2a09774ab5a13cb925741860d16358f334f2698f5ec9e4`**
  (`logs/b1-snapshot.txt`); **R006 recheck digest `d82fa02802bfd4c2056f89ffa737282cdbb3573e8364261088c6207d2a1b5c11`** (`logs/r006-snapshot.txt`).
- Coordination: `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md` r4 (sha256
  `89563c7e…0f5b`, copy `logs/coordination-r4.md`), writer = this root. WF-C001 r2 (→ p08),
  WF-C002 r1 (→ p09), WF-C003 r1 (→ p08, p09) active. NOTE: the snapshot exclusions include
  COORDINATION.md, so r4 does not change the B1 digest.
- Compaction: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` added to `.claude/settings.local.json`
  (user-approved); not confirmed active for this session.

## State

- B1 gate P07-D1 **passed**: rev 1 → R001 changes; rev 2 → R002 changes (F008, F009 Low);
  rev 3 → R003 changes (F008 open); rev 4 → **R004 PASS** (sha256 `5c076606…adc4`). Revision 4
  (`logs/decision-D1-r4.md`) is the B1 implementation contract; optional F012 adopted in the
  `lineage_child` helper contract. Coordination r3: WF-C001/C002 active.
- B1 **implemented** (2026-10-22). Deviations 1–7 and new test modules are in TASK "B1
  implementation notes"; every moved test is in `logs/b1-fixture-moves.md` (60 affected tests,
  all present in the baseline log). Verification logs: `logs/b1-clippy.log`,
  `logs/b1-workflow-tests.log` (same command as the baseline), `logs/b1-stack-budget.log`,
  `logs/b1-migration-fresh.log`. `cargo sqlx prepare --check` is unavailable here (no
  `sqlx-cli`); no compile-time-checked query touches a changed table.
- B1 verified (TASK "Checks": clippy clean, `workflow` filter 1137/0, stack budget 102/0, fresh
  migration ok; `sqlx prepare --check` unavailable) and frozen; B1 batch review requested from B
  (round WF-P07-R005). R005: changes required (F013–F016; F017 optional). All corrected in one
  round (TASK "B1 corrections for WF-P07-R005", incl. deviation 8), verified (one load-sensitive
  unrelated failure recorded) and frozen. **R006 PASS; B1 accepted in phase** at digest
  `d82fa028…5c11` (root recomputed it; coordination r3 unchanged). Uncommitted, not integrated.
- B2 (agent workflow tools): user decisions P07-Q1 (named registration; test that tools are the
  agent's company's) and P07-Q2 (revoke stops, re-pin doesn't); gate P07-D2 rev 4 **PASS** (R010,
  `logs/decision-D2-r4.md`). Implemented; see Next action.

## Resources

Phase PostgreSQL 18.6 cluster `/private/tmp/workflow07-20261022`, `127.0.0.1:55470`, superuser
role `workflow07` (trust, loopback only), databases `workflow07` and `workflow07_test`;
`own_database` creates `mail_agents_own_*` there. Use
`PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH` and
`DATABASE_URL=postgres://workflow07@127.0.0.1:55470/workflow07`. Restart with
`pg_ctl -D /private/tmp/workflow07-20261022 -l /private/tmp/workflow07-20261022/server.log start`.
Never touch the user's 5432 server. Builds with `CARGO_INCREMENTAL=0`.

## Next action

Phase p07 is implemented, accepted and fully verified: B1 R006 PASS, B2 R012 PASS, and the
test-only full-suite fix R013 PASS at
`af49b63b765cc6e1d54d0005c805023a3ead87c31ea092aa69ce6809d403f2cb` (`logs/r013-snapshot.txt`).
Final phase gate passed; full suite (`--all-targets`, network-isolated) lib 2744/0, bin 5/0;
stack budget 2744/0 at 2 MiB; lease budget 130/0 (TASK "Checks"). Unavailable here:
`cargo sqlx prepare --check` and `graft build`. Reviewer B `a5e5548971a73e411` is idle.
Committed as `bd2ef36` on `workflows` and pushed to `origin/workflows` (2026-10-22); not merged
into `main`. Nothing outstanding for p07.
