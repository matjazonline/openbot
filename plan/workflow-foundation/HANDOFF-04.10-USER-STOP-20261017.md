# 04.10 user-stop handoff — 2026-10-17

No backward compatibility is required; reset the database when a schema change does not
apply cleanly (see `AGENTS.md`).

**04.10 incomplete; stopped at the user's checkpoint request.** 04.9 remains accepted
(commit `1b3b82a`). User authorized 04.10 then 04.11 under `implement-opus-reviewed-2.1`;
04.11 has not started. Not authorized: commits, deployment, migration squash, production
data, host firewall mutation, phases 05+.

## Architecture gate — accepted (decision level only)

Effective decision: 04.10-D2 (`docs/logs/04.10/decision-D2.md`, SHA256 `84d532ee…0ded5`)
as amended by 04.10-D3 (`docs/logs/04.10/decision-D3.md`, SHA256
`f89b0a89afa11ec9b789ce8e25fc3370a9678bbb43ff1e60c7dcb3e050e7b32b`). D1 is superseded.
Independent reviews 04.10-R001 (D1 changes required), R002 (D2 changes required), R003
(**PASS** on D3). Findings F001–F011 resolved/dispositioned at decision level (F004
declined as limitation). Root decision: aligning the accepted 04.9 HTTP handler's
pre-marker outcomes (F008) is in 04.10 scope; D2 also changes the shared phase03/04.6
no-marker recovery default (`marker_owned_effect_kind`), judged sound in R002.

Open, must be closed before batch acceptance: **04.10-F012** (low; preparation enforces
reservation's connection readiness — author reports implemented, untested).
04.10-F013 optional: (a) implemented, (b) I9 rewording not done.

## Implementation state — unverified, not review-ready

Author's exact checkpoint (files, done/not done, resume steps):
[TASK.md "04.10 author USER STOP checkpoint"](docs/TASK.md). Summary:

- Written: D2 items 1–10 and D3 A–D, new migration
  `migrations/20261017110000_workflow_mcp_operations.sql`, application MCP handler and
  direct-tool path, shared `actions/effect.rs` and `actions/failure.rs`, persistence
  `workflow/mcp*.rs`, `adapters/workflow_mcp/`, shared `adapters/redaction.rs`, MCP adapter
  outputSchema/typed result/endpoint-first permits, F012, F013a, F011 documentation,
  13-test MCP DB matrix and HTTP outcome tests.
- Not done: adapter tests (outputSchema discovery, call-result redaction, permit
  starvation/shutdown); BRIEF-04.10.md and its `NO_BACKWARD_COMPATIBILITY_NOTES` entry in
  `src/main.rs`; 06/08 obligation for F011; F013b; `logs/04.10/gates.sh` and the full batch gate.
- **Gated-plan deviation to raise with B:** the D2/D3 matrix case "one new `tools/call`
  after proven not-applied via the lazy in-`invoke` session" is not achievable as a DB test
  because MCP has no registered evidence verifier; the author proposes an application-level
  test with fakes. B must judge this (it may need a decision note) before batch review.
- Checks: new migration applied to fixture PASS (`migrate-mcp.log`); lib offline check PASS
  with 2 warnings since fixed (`check-lib-1.log`); all-target check FAIL twice
  (`check-all-1.log`, `check-all-2.log` — 3 test compile errors edited at stop, not re-run).
  **No tests have run.**

Checkpoint snapshot (identity only, not a review freeze): base
`8ce1fd49b5a961da705fddf0b92cac3182da427b`, digest
`6d5b89b7a612505bf209e74b18a3483a5999d5920ecb905bca7f4a8979de0c30`, manifest
`docs/logs/04.10/user-stop-snapshot.txt` (excludes `plan/workflow-foundation/docs`).

## Resources and workers

- Task fixture (PostgreSQL 18.6) stopped cleanly, data retained:
  `/private/tmp/workflow0410-20261017`, 127.0.0.1:55450, role `workflow0410`, db
  `workflow0410_test`, system id `7697669855432439122`. Start with
  `/opt/homebrew/opt/postgresql@18/bin/pg_ctl -D /private/tmp/workflow0410-20261017 start`.
  The 04.9 fixture `/private/tmp/workflow049-crash-20261017` was not touched.
- No author build/test/postgres processes remain. Workers (author, reviewer — both
  Opus 5.5/xhigh) are idle; handles are recorded in TASK.md but a fresh session should start
  fresh workers that read these records rather than trust stale handles.
- Root ran at effort `high`, so it coordinated only. `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` was
  added to `.claude/settings.local.json`; it applies to sessions started after 2026-10-17.

## Resume steps

1. Start the fixture; re-run `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`.
2. Run focused MCP/HTTP/recovery/publication/binding/admission/receipt/reconciliation suites.
3. Write missing adapter tests, resolve the lazy-path test deviation with B, finish
   BRIEF-04.10/main.rs note, F011 obligation, F013b.
4. Write and run `logs/04.10/gates.sh` (per D2/D3 verification plan, incl. scoped stock
   2 MiB run), freeze with `snapshot.sh`, then independent whole-batch review.
5. After 04.10 acceptance: 04.11 messaging plus the combined phase04 gate.
