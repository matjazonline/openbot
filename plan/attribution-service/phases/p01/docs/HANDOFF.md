# Attribution service — current p01 handoff

## Active task and roles

Implement **all ten ordered phases** in `plan/attribution-service` using
`implement-astra-reviewed-2-3`. The task is implementation, not planning alone.
Root/A `/root` is **Sol 6.1/xhigh**, explicitly confirmed by the user in separate
“you are xhigh” and “sol6.1” replies. Do not ask again. Independent B is
`/root/attribution_reviewer`, requested **Astra/high**, spawned with no inherited
conversation; reuse this handle. B owns REVIEW/notes/findings/logs only and never
implements. Actual worker telemetry/context occupancy is unavailable; tool accepted
the requested settings. No strict compaction control is exposed; use checkpoints.

## Confirmed scope and policy

- API-first, store-neutral commerce. Native platform adapters/signatures and a real
  store pilot are deferred, not falsely passed.
- Sign in with Openbot through a dedicated code/PKCE bridge, keeping account/company
  ownership in Openbot and attribution sessions/cookies/secrets separate.
- User explicitly accepted owner/admin read/manage, members read, explicit browser
  consent, no raw IP storage, 30-day attribution, five-minute maximum company-status
  freshness, and retryable 503 on click commit failure/uncertainty.
- Production region/retention/load targets remain deployment choices; implementation
  uses explicit fixture budgets/configuration. No commits/push/deployment or
  production database resets were authorized by the skill.

## Repository and coordination

- Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
  `attribution-service`, starting/review-base/current HEAD
  `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Original tree was clean.
- Authoritative coordination:
  `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
  revision 2, named writer root. Notices AS-C001/r1 and AS-C002/r1 acknowledged;
  no pending cross-phase requests. Recheck before acceptance/next phase.
- Record root is this directory. [TASK.md](TASK.md) has criteria, exact source
  discovery, decisions and evidence. [REVIEW.md](REVIEW.md) and B's notes/findings
  retain all independent review history. Do not rewrite old rounds.
- Exact snapshot exclusions: `plan/attribution-service/phases/p01/docs` and
  `plan/attribution-service/COORDINATION.md`. Exclude no deliverable/source files.
- macOS needs command-local GNU coreutils:
  `PATH="/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH" bash .agents/skills/implement-astra-reviewed-2-3/scripts/snapshot.sh plan/attribution-service/phases/p01/docs plan/attribution-service/COORDINATION.md`.

## Current artifact and review

**Source is frozen for B's AS-P01-B1-R2 correction recheck and D2/r1 gate.**
Digest `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`.
Retained snapshot and validated manifest: `logs/AS-P01-B1-R2.{snapshot.txt,evidence.json}`.

p01 deliverables: amended original plan scope, service contract README, both OpenAPI
3.1 documents (canonical identity owner copy under root `docs/contracts/`), examples,
locked Python contract tooling and standalone check script. **No Rust service,
application feature or migration exists yet. p02–p10 have not started.**

Review history:
- AS-P01-G1-N1: changes required on D1/r1 (account deletion's independent company
  deletion path; same refund ID with different event IDs).
- AS-P01-G2-N1: PASS D1/r2; G1-F1/F2 closed at contract/design level, runtime tests
  still required in p03/p07.
- AS-P01-B1-N1: whole contract batch changes required. B1-F1 lacked management
  retry identity/ambiguous credential-secret recovery; B1-F2 omitted report reason
  totals and distinct refund counts.
- Current B1-R2: A corrected both together; B is reviewing. D2/r1 amendment covers
  management receipts/key timestamps/one-time-secret recovery and explicit retry
  exceptions. Only B may close findings and pass this artifact/decision.

Verification: **12/12 contract checks passed** in place and service-only temp copy;
all 9 earlier names retained plus 3 new cases. Both OpenAPI documents validate;
identity copies match; docs/registration/links/whitespace checks pass. Evidence
validator passed. No runtime auth, database, browser, container or load proof is
claimed. No phase is accepted yet.

## Resources and machine findings

- Phase-owned contract venv `/tmp/attribution-service-contract-env`, pinned by
  `services/attribution/contract/requirements.lock`. System Python 3.9 emits a
  harmless LibreSSL warning for these local-only schema tests. Homebrew Python
  3.13 is available for later tooling; do not alter system Python.
- No owned database/port/build cache/server process yet. No cleanup of unrelated
  processes, databases or caches permitted.
- Local Homebrew PostgreSQL 18.7 responds on localhost:5432; `mac02` role can use
  maintenance database `postgres`. SQLx CLI 0.8.6 and stable Rust 1.99.1 installed.
- No Docker/Podman/Colima/OrbStack runtime found. Container checks will need an
  available runtime or remain an explicit verification gap; do not claim them passed.
- About 13 GiB free disk at start. Keep build profiles/caches modest and isolated;
  do not delete other tasks' artifacts to make space. A personal Trail learning
  warns other suites share this PostgreSQL cluster; use uniquely owned databases
  and investigate interference before calling performance failures regressions.

## Next action

Wait for B's actual R2 result. Correct all actionable findings in one batch if any.
After explicit artifact and D2/r1 PASS, recompute exact digest, recheck coordination
and validate manifest; record p01 acceptance. Then register p02 with phase-local
records and implement the standalone Rust/Axum/PostgreSQL foundation, its migration,
startup/limits/shutdown, isolated tests, boundary/extracted-copy checks, Dockerfile
and independent CI. Reuse B for sequential phases with explicit new record paths.
The final p10 gate must reconcile both applications' actual CI and run the required
full verification. Do not stop after accepting just p01.


## p01 acceptance — 2026-11-15

Root recomputed digest `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`,
checked unchanged coordination revision 2 and notices, and validated retained R2
manifest. B explicitly passed D2/r1 and the full p01 artifact in AS-P01-B1-N2;
all four findings across architecture/batch rounds are closed by B. 12/12 contract
checks passed in place and standalone. **p01 is accepted in phase and present in
the current sequential worktree**. This is contract acceptance only; runtime and
p02–p10 implementation/full-suite gates remain unfinished. Next: p02 foundation.
