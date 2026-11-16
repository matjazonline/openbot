# Plan03 p04 — core accepted; publish and continue Plan04

User authorized all work pushed to main, then Plan04, stopping before Plan05.
Remaining Doop work is explicitly deferred to `plan/fix-doop.md` by C008/r1.
Do not resume Doop investigation before Plan04 or claim whole CI is green.

Worktree: `/Users/mac02/orca/workspaces/openbot/plan03-04`; branch `feature/plan03_04`.
Root/A `/root` is user-confirmed Sol 6.1/xhigh. Retained independent B
`/root/p04_reviewer` is Astra/high and has completed R17. Root owns TASK/HANDOFF/
coordination; B owns review records. Authoritative coordination is
`plan/03-principal-addresses/COORDINATION.md`, revision59; C008/r1 active and
acknowledged, previous I/C contracts unchanged. Review base:
`3fceb7d3435f3b821c3b37820200e1c5400206d9`.

## Accepted artifact

ALL source, evidence, six learnings and fix-doop plan committed as a3194fe4.
Upstream 59936435 planning changes normally merged as
`c1ccbc73384d0ccd5cb0cb64d65a7a5fd29691ed`. Final accepted artifact:
`d27557291a01be97a3856d6cf1867e06900df104736a9c69f07754b3000f88fb`.
Independent **03PA-P04-B1-R17/N1 PASS**, no new findings, previous source findings
resolved. Root acceptance recorded in TASK and coordination59. Final manifests:
`logs/B1-R17-final-core.evidence.json`, `logs/B1-R17-reviewer.evidence.json`.

Current stock 67/67 PASS, actual history fixture/browser 1+2 PASS, Node26/Python10
PASS, formatting/strict Clippy/offline all-target compilation/docs PASS. Initial
ENOSPC compilation failure retained; only owned incremental cache removed before
successful retry. Historical 4678-case stock and 139-case lease measurements remain
historical; B R16/N2 approves scoped reuse with current 67 bridging correction.
Actual full native CI: 4677 PASS / 1 unchanged Cloudroom FAIL / 27 ignored. B R16/N1
traced 107 unchanged relevant files and outside-scope branch; precise socket close
cause unknown. Preserve full FAIL. No whole-CI-green claim.

Doop lifecycle still reaches original 80-second deadline; pinned service has two
intermittent browser failures and private four-profile acceptance is incomplete.
All retained in `plan/fix-doop.md` and evidence. Reviewed catalog/Chrome/TMPDIR/
diagnostic fixes are included. R23 prototype reverted, proposal patch retained.
No unfinished Doop source remains local and no test limits/assertions are weakened.

## Immediate next action

Only workflow records/evidence remain uncommitted. Recompute accepted snapshot
using exact exclusions below; validate evidence using a new output file; run Trail
check; commit final records; prove commit changes only excluded records, then normal
SSH push HEAD:main and verify remote SHA. No force push, no stash. Record publication
in Plan04 p01 prerequisite, then implement Plan04 using the same workflow.

## Resources and Plan04 preparation

Environment: `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh`.
Node24/GNU/Python3.13/PG18, owned PostgreSQL localhost:51167. No active local Cargo
or runtime worker. Preserve binaries, backups, databases and task-owned caches.
Docker daemon unavailable locally. Normal SSH origin: git@github.com:matjazonline/openbot.git.
Context controls unknown; use durable checkpoints.

Plan04 originals read; read-only scope at external `pa04-readonly-scope/` beside env
file, including 150 files / 263 company_members hits and FK/caller constraints.
No Plan04 records or implementation yet. First batch needs cohesive architecture
gate for root department/membership/owner/admin/schema/FK/lifecycle. Read nested
HTTP pages AGENTS before HTML changes. Read original Plan04 docs and register its
own coordination and phase records; no need to repeat exhaustive scope discovery.

Exact seven snapshot exclusions:
- plan/03-principal-addresses/phases/p04/docs
- plan/03-principal-addresses/phases/p03/docs
- plan/03-principal-addresses/phases/p02/docs
- plan/03-principal-addresses/phases/p01/docs
- plan/03-principal-addresses/COORDINATION.md
- plan/02-workflow-roles/COORDINATION.md
- plan/02-workflow-roles/phases/p05/docs

Historical full prior handoff retained in logs/R24-prior-handoff.md. Source and
evidence identity files retained in R24 integration and B1-R17 logs.
