# 04.9 user-stop handoff — 2026-10-17

The user requested **“at next handoff write handoff and stop”**, clarified as
**“at next secure checkpoint.”** Work is stopped. A new instruction is required
to resume. 04.8 remains accepted. 04.9 implementation is incomplete and unreviewed;
architecture decision 04.9-D2 alone passed independent review 04.9-R002 and root
accepted that architecture gate. This is not implementation acceptance.

Author `/root/author` (Astra medium), reviewer `/root/reviewer` (Astra medium),
coordinator `/root`; root model metadata was not authoritatively exposed. Context
occupancy was unavailable. Earlier user steering to keep A until batch readiness
was superseded by this stop. No authorship takeover or further work occurs now.

## Scope and implemented working tree

04.9 supplies company fixed-origin HTTP connections, encrypted/rotatable secrets,
trusted operation registrations, publication/binding/admission authority, and the
HTTP step/direct tool composition through the existing ActionService. It includes
DNS/connect guards, bounded requests/responses, credential redaction and hostile
transport/database tests. See [brief](BRIEF-04.9.md), original EXPANSION point04.9,
and [TASK decision D2](docs/TASK.md). MCP04.10, messaging04.11, legacy/startup
replacement06/08 and authoring UI09 remain deferred. No commits, deployment,
production reset, migration squash or host firewall changes were performed.

New application files: `src/application/workflow/http.rs`, `http_runtime.rs`.
New adapter: `src/adapters/workflow_http/{mod,tests}.rs`.
New persistence: `src/adapters/persistence/workflow/http.rs`,
`http_publication.rs`, `http_runtime.rs`, `http_tests.rs`, `http_test_support.rs`.
New additive migration: `20261017100000_workflow_http_connections.sql`.
Existing publication/authority/readiness and credential inventory/rotation code
are integrated. Existing MCP destination policy was factored to expose its vetted
resolved-address builder; default restrictions and exact operator exceptions remain.
Graph refreshed successfully; `graft/` is an ignored local cache.

The last source revision includes typed method/path validation, saved-step kind
validation, post-DNS active-run/actor/credential checking, foreign/member/forged
publication tests, unknown-write replay test and stalled-response deadline test.
Those additions have **not completed compilation/testing**. Earlier 11-test pass
predates them. Current source was held stable while the interrupted check ran.

## Evidence and interrupted process

All evidence is under `docs/logs/04.9/`:

- `baseline-check.log`: baseline offline all-target check PASS.
- `baseline-migrations.log`: baseline migrations PASS.
- `http-fresh-migrations.log`: fresh task database migrations including current
  additive HTTP migration PASS; earlier `http-migration.log` predates the FK cascade
  correction and is retained history.
- `check-lib2.log`: implementation library offline check PASS, before latest changes.
- `http-tests-second.log`: earlier focused HTTP suite **11 passed / 0 failed**,
  including real lifecycle, receipt replay, dispatch/revision competitors,
  revocation, secret scoping, DNS pinning and hostile transport.
- `http-tests-current.log`: expanded current HTTP suite **interrupted/unverified**.
  Command: `TEST_DATABASE_URL=postgres://postgres:workflow049@127.0.0.1:55449/workflow049_test SQLX_OFFLINE=true cargo test --locked --offline --lib workflow_http_`.
  After over six minutes of active compilation without diagnostics, the user stop
  was honored by SIGTERM to task-owned cargo354655 and rustc354694. Shell status143;
  both processes confirmed absent. Unified exec session81588 completed. No tests
  from this invocation completed; do not infer a pass from its earlier counterpart.
- `fmt.log`, `diff-check.log`, `transport-boundary.log`,
  `transport-boundary-tests.log`: formatting/whitespace/static transport gates PASS.
- `graft-build.log`: graph refresh PASS.
- `final-gates.sh`: prepared serial command sequence **NOT EXECUTED**. It is a
  convenience record, not evidence. Review it and missing checks below before reuse.

Prior import/type failures in first check logs were corrected before the earlier
11-test pass. They are retained as history. No current required full-suite claim
is made. Local CI network-isolation wrapper remains unavailable: on Linux it
changes shared-user host OUTPUT firewall and assumes PostgreSQL5432, whereas this
fixture uses55449. Root explicitly agreed to leave shared host firewall unchanged.

## Retained disposable resource

Only task fixture `workflow-foundation-049-20261017` was created/mutated. It is now
**stopped, data retained** (Docker state `exited`). Container ID:
`257e9aab04959a3c3e34a2f55cc16cd509df1899488a523f9f3781fa04e21768`.
Volume: `c5fed217cc0982b517310205a52114f8da66989402f78477f9bf8c9fcf6ce9fc`,
mounted `/var/lib/postgresql`. Image `postgres:18.6-bookworm`.
Host `127.0.0.1:55449`; database `workflow049_test`; PostgreSQL system identifier
`7697618204589658155`; task-only URL
`postgres://postgres:workflow049@127.0.0.1:55449/workflow049_test`.
`pg_stat_statements` is configured. Docker socket access requires `newgrp docker`.
Historical stopped04.8 fixture and all unrelated resources remain untouched.
No author-owned cargo/test/graph processes remain.

After a new continuation instruction, verify identity then start only this fixture:

```sh
newgrp docker <<'SH'
docker inspect --format '{{.Id}} {{.State.Status}}' workflow-foundation-049-20261017
docker start workflow-foundation-049-20261017
SH
```

## Next authorized continuation steps

1. Read skill2.1, repository/subsystem instructions, D2 and R002. Preserve current
   WIP and initial baseline changes; use graft-first discovery/call tracing.
2. Finish expanded focused HTTP compilation/test and fix actual failures. Add a
   real HTTP-envelope inventory/key-rotation row test: existing shadow-table
   fixture includes the new table but has no test that rotates a populated HTTP
   row. Existing MCP rotation test at credential_rotation_tests.rs554 is a model;
   test old-key rewrap, tenant/connection context, idempotence and ciphertext CAS.
3. Run related MCP/credential focused checks, then SQLx prepare/check, offline
   all-target check, strict Clippy, all-target full DB tests, stock-stack gate,
   lease-budget gate, formatting/static checks and final graph refresh. Existing
   fresh migration pass covers current migration, but run required migration
   verification against the identified task resource as appropriate. Serialize
   fixture suites/writes; never mutate shared host firewall. No limits were raised.
4. Audit acceptance matrix, including authoritative negative publication/binding/
   admission paths, current resource/credential/policy checks and competing owners;
   add missing coverage before declaring the batch ready.
5. Update brief/evidence and freeze complete batch for independent B code review.
   Root acceptance remains outstanding. The earlier user intent for root takeover
   at batch readiness is historical; obtain scope from new continuation instruction.

## Baseline and artifact identity

Baseline HEAD: `2729f2caaf58830bea8291bdcc6f9882a3ad03ee`.
Pre-existing modified `AGENTS.md` and untracked
`.agents/skills/implement-astra-reviewed-2/`, `implement-astra-reviewed-2.1/`
are preserved and included in snapshot identity, not claimed as04.9 changes.
Snapshot digest and exact file list are in `docs/logs/04.9/user-stop-snapshot.txt`
and recorded in TASK after this handoff/RESUME were saved. The digest intentionally
includes this handoff and brief; it excludes only these mutable workflow records:

- `plan/workflow-foundation/docs/TASK.md`
- `plan/workflow-foundation/docs/REVIEW.md`
- `plan/workflow-foundation/docs/agent-review-notes.md`
- `plan/workflow-foundation/docs/agent-review-findings.md`
- `plan/workflow-foundation/docs/logs/04.9`

Author's retained graft estimate tally is approximately4,297,415 tokens saved
(tool-reported estimates summed; these are estimated avoided full-file reads,
not billed model tokens). This is a resumable stop checkpoint, not a review freeze.
