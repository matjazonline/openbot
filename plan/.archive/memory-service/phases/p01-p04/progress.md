# Memory service implementation — root progress record

Workflow: implement-opus-reviewed. Root owns acceptance; A authors; B reviews.

## Identity
- Root session UUID: 6fedfb2e-40aa-486c-bd0b-051b5a3add7b (claude-opus-5-5, session settings)
- Run id: edd1eeb1. Context window: 1,000,000 (user-stated, 1M, all roles). Retire workers at 75% (user instruction ~20:05Z; was 60%); root checkpoint at 85% (user instruction ~20:07Z; was 60%).
- Base commit: a6bed48a89d986204d7df13d4b4c5a58458176cf (HEAD, detached worktree)
- Freeze helper: tmp/memory-service-run/freeze.sh <label> (temp index; excludes tmp/ and ignored files)
- Baseline freeze: tree a74abc49d48450868106583578471f28205a9658, empty diff.

## Scope and originals
- Originals: plan/memory-service/README.md, 01..06 (all phases, in order).
- Repository instructions: AGENTS.md (root), nested src/**/AGENTS.md / CLAUDE.md as applicable, graft mandate.
- Preserve: untracked tmp/ (tmp/claude-mem research checkout at 3ab6e1a215c7bbcf716b9d8d763aa78bacba52a0 — read-only reference).
- No commits/pushes/deploys authorized. No dev/customer DB migration or reset.

## Product decisions (user, 2026-11-08)
- Local env: pgvector 0.8.7 installed via brew for postgresql@18 (user-approved). Docker unavailable locally → container/non-root-image checks recorded unavailable.
- Generation provider: Anthropic Messages API, model claude-sonnet-5-5 (user choice). Tests/CI: counted local test provider only; no paid calls without a named budget.
- Embedding: reuse OpenBot pin intfloat/multilingual-e5-small@614241f622f53c4eeff9890bdc4f31cfecc418b3, fp32, mean pooling, 384-d; service owns its own manifest/prepare step (~490 MB one-time download, gitignored).
- Ops budgets (documented as operator configuration):
  - Topology: same host as OpenBot first, separate DB + role; separate-host move proven via config only.
  - Retention: canonical memories until corrected/deleted; raw evidence & generation inputs 30 days after operation ends; idempotency results 7 days; audit 400 days; deletion barriers indefinitely (opaque IDs only).
  - Spend: every generation request carries its own allowance; per-tenant daily token cap set by operator, default 0 = generation disabled.
  - Load (local hw): 100k observations/tenant, 20 req/s search; search p95 < 250 ms; context pack p95 < 400 ms; 4 generation workers, ≤1 in flight per tenant.

## Ordered points
| Pt | Scope | Depends | Status |
| --- | --- | --- | --- |
| P1 | Architecture expansion (service-wide decisions 01–04) + B architecture gate | — | architecture-reviewing (r3 limited recheck) |
| P2 | 01A/01B + 02A: skeleton, config/startup, contracts (OpenAPI/MCP), credentials, CI, boundary | P1 | verified locally (accepted); CI jobs + Docker image + Linux isolation UNVERIFIED (unavailable) |
| P3 | 02B storage, idempotency, correction/deletion | P2 | verified locally (accepted); n1–n3 minors carried into P4 review |
| P4 | 03A keyword + semantic retrieval (+ P3 n1–n3) | P3 | verified locally (accepted) |
| P5 | 03B progressive recall, context packing, MCP | P4 | verified locally (accepted); m1–m3 carried into P6a |
| P6 | 04A/04B generation, jobs, spend — split P6a (04A + P5 minors), P6b (04B) | P5 | verified locally (accepted) |
| P7 | 05 expansion + gate (PASSED r4, tree 6238f2bc), then P7b–P7f (author split: b config/ports/adapter/creds/lifecycle; c audit+content_exact, steps, intents, delivery, samples; d tools + recall/save race; e auto recall; f capture/gen intents/lifecycle races) | P6 | P7b accepted; P7c next |
| P8 | 05B automatic context, capture, delivery, lifecycle | P7 | pending |
| P9 | 06A/06B operations, standalone/e2e proof | P8 | pending |
| P10 | 06C final full gate + combined integration review | P9 | pending |

## Roles
| Role | Gen | Agent ID | Nonce | Status |
| --- | --- | --- | --- | --- |
| A | 1 | afb5a69a11cda6759 | run=edd1eeb1 role=A gen=1 | RETIRED after P2 return 10:30Z (51.04%, next point would cross 60%) |
| B | 1 | a636617799682ea85 | run=e127dfad role=B gen=1 | P2 code review r1 returned (46.11%): 3 major, 12 minor; R1–R3 closed in code |
| A | 2 | a50e0e196034f65c7 | run=42011c65 role=A gen=2 | P2 corrections r1 returned (34.8%): F1–F3, m1–m12 fixed; DB suite + verify.sh + extracted pass |
| B | 1 | (same) | | RETIRED after P2 recheck r2 PASS (49.12%); next review is a large new diff |
| A | 2 | (same) | | P3 returned (42.6%) |
| B | 2 | a80bac1035a115508 | run=d756cf6c role=B gen=2 | P3 review r1 returned (21.31%): F1–F3 major, F4 minor(decision), m1–m8 |
| A | 2 | (same) | | P3 correction r1 returned (47.8%) |
| B | 2 | (same) | | P3 recheck r2 PASS (25.29%) w/ minors n1–n3 |
| A | 2 | (same) | | RETIRED after P3 (47.8%; P4 too large for headroom) |
| A | 3 | afb217285d5f22f53 | run=64ae264f role=A gen=3 | P4 returned 12:26Z (45.19%) |
| B | 2 | (same) | | P4 review r1 returned (33.98%): F1 major keyword identifiers, F2/F3 minor |
| A | 3 | (same) | | P4 correction r1 returned (49.22%) |
| B | 2 | (same) | | P4 recheck r2 returned (36.0%): F1a major (all-caps rule floods) |
| A | 3 | (same) | | P4 correction r2 returned (50.52%) |
| B | 2 | (same) | | P4 recheck r3 PASS (37.1%) |
| A | 3 | (same) | | RETIRED after P4 (50.52%) |
| A | 4 | a8ada481219541d60 | run=11351297 role=A gen=4 | P5 returned 13:07Z (30.63%) |
| B | 2 | (same) | | P5 review r1 PASS (40.79%) w/ minors m1–m3 |
| A | 4 | (same) | | P6a returned 13:30Z (41.96%) |
| B | 2 | (same) | | RETIRED after P6a review r1 (49.44%): F1 major (pre-redaction hash), m1–m4; unread: observation_tests.rs, recall m2 rerun path, generation outcome tail |
| A | 4 | (same) | | RETIRED after P6a correction r1 (49.1%); P6b handoff in decision record |
| B | 3 | afbba0fc2ed0e068e | run=08e8e605 role=B gen=3 | P6a recheck r2 PASS (22.17%) |
| A | 5 | a826920599a1b4624 | run=c18dec0f role=A gen=5 | P6b returned (44.3%) |
| B | 3 | (same) | | P6b review r1 returned (34.28%): M1 major (missing competing lease/heartbeat/sweeper tests), m1 sweep stall, nits n1–n6 |
| A | 5 | (same) | | P6b correction r1 returned (48.8%) |
| B | 3 | (same) | | P6b recheck r2 returned (38.64%): M1 residual (mutations A, D, A+D survive), m2 n6 side effect |
| A | 5 | (same) | | P6b correction r2 returned (51.4%) |
| B | 3 | (same) | | P6b recheck r3 returned (42.07%): M1 residual resolved, m2 fixed; F1 flaky stalled-heartbeat (shared renew_fault Arc) |
| A | 5 | (same) | | P6b correction r3 returned (52.1%) — RETIRE after P6b |
| B | 3 | (same) | | P6b recheck r4 returned (43.71%): F1 fixed; F2 boundary test speed-dependent (2/28 concurrent) |
| A | 5 | (same) | | RETIRED after P6b correction r4 (53.4%) |
| B | 3 | (same) | | RETIRED after P6b r5 PASS (45.27%); P7 gate needs fresh reviewer |
| A | 6 | a4c17713f9f4e7b2b | run=d3ca475a role=A gen=6 | P7a expansion returned 15:52Z (32.74%) |
| B | 4 | a5916c4a63f198c6f | run=8e7c1e85 role=B gen=4 | P7a gate r1 returned (26.57%): 7 major, 9 minor, 6 nits |
| A | 6 | (same) | | P7a correction r1 returned (36.23%) |
| B | 4 | (same) | | P7a recheck r2 returned (32.14%): N-M1 major (disable vs later intents), N-m1..m5 |
| A | 6 | (same) | | P7a correction r2 returned (39.22%); U2 structured as one match arm, pending user |
| B | 4 | (same) | | P7a recheck r4 PASS (35.6%); R4-m1 (P7c), R4-m2 (P7f) carried |
| A | 6 | (same) | | RETIRED mid-P7b after substep 1 (49.16%); handoff in record |
| A | 7 | a550ec35ea3a3f984 | run=dff69bbd role=A gen=7 | P7b complete returned 18:43Z (37.71%) |
| B | 4 | (same) | | P7b1 findings recheck PASS (47.31%); new nit R-n1 (assert_clean poisoned mutex); RETIRE (next review large) |
| B | 5 | a217255500990fff2 | run=e471aa3f role=B gen=5 | P7b review r1 returned (28.08%): NOT ACCEPTED — M1 permanent ns failure, M2 terminal after outage, M3 inherited attempts, M4 two reproduced deadlocks; m1–m5 |
| A | 7 | (same) | | P7b correction r1 returned (45.5%) |
| B | 5 | (same) | | P7b recheck r2 returned (34.38%): majors resolved; r2-m1..m4 minor, n1–n2 |
| A | 7 | (same) | | P7b correction r2 returned (48.45%) |
| B | 5 | (same) | | P7b recheck r3 returned (37.64%): r2 resolved; r3-m1 (409 conflict vs namespace_deleted) |
| A | 7 | (same) | | P7b correction r3 returned (49.61%) — RETIRE after P7b |
| B | 5 | (same) | | P7b recheck r4 PASS (38.54%) |
| A | 7 | (same) | | RETIRED after P7b (49.61%) |
| A | 8 | af38660db9d70c7bd | run=411dc8ab role=A gen=8 | P7c (exactness, steps, delivery, samples) assigned ~20:00Z; told 75% limit |

## Context samples
| Time (UTC) | Agent | Input tokens | % | Source |
| --- | --- | --- | --- | --- |
| 2026-11-08T05:22Z | root | 80,186 | 8.02 | helper, flag window |
| 2026-11-08T05:40Z | A1 afb5a69a11cda6759 | — | 24.56 | helper via nonce (matches ID) |
| 2026-11-08T05:41Z | root | 135,676 | 13.57 | helper |
| 2026-11-08T10:30Z | A1 | — | 51.04 | self-report at return |
| 2026-11-08T10:33Z | root | 171,868 | 17.19 | helper |
| 2026-11-08T12:50Z | root | 260,640 | 26.06 | helper |
| 2026-11-08T14:35Z | root | 308,789 | 30.88 | helper |
| 2026-11-08T19:02Z | root | 458,460 | 45.85 | helper |

## Gates / acceptance log
- P7a architecture gate: PASS r4 (reviews/p7a-arch-r4.md, tree 6238f2bc) incl. confirmed U2 refinement. Earlier: PASS on r3 text (reviews/p7a-arch-r3.md, tree 4bbd84f1). U2 refinement + R3-m1/R3-n1 being applied by A6 → B4 limited recheck before P7b edits.
- P7b ACCEPTED (local) after B5 r4 pass (reviews/p7b-code-r4.md; earlier B4 substep review + recheck). 4 correction rounds. Frozen p7b-code-r4 tree 113801e4. Carried: browser check of settings memory section (both themes, busy states); live-service tests; full suite + stack-budget at 05 gate/06C; transport-boundary pre-existing failure; P7c obligations (ensure_namespace outside run txn + pool accounting; disable fails pending intents; owner hooks cancel intents; E9 disable race test; delivery vs lifecycle permit contention); 06A: settled-row retention, 1,000-cred ceiling (service change), managed_by reassignment vs 404/409 settles, provisioner rotation.
- P6b ACCEPTED (local) after B3 r5 pass (reviews/p6b-code-r5.md): 5 review rounds; verify.sh 153/153; generation 20 seq + 8×4 concurrent all pass; mutations A, D, R caught (separate target). Service phases 01–04 complete locally. Frozen p6b-code-r5 tree 7043063f.
- P6a ACCEPTED (local) after B3 r2 pass (reviews/p6a-code-r2.md): verify.sh 135/135; thinking=between_tools confirmed via claude-api skill. Carried into P6b first batch: m1 prefixed-key + URL-password redaction, nits n1–n5. Frozen p6a-code-r2 tree 54270382.
- P5 ACCEPTED (local) after B2 r1 pass (reviews/p5-code-r1.md): verify.sh 120/120 incl. Python independent client + real model. Minors m1 (validate/valid_until provenance), m2 (full-detail body loading), m3 (byte_count/token doc) carried into P6a. Frozen p5-code-r1 tree 0e841af1.
- P4 ACCEPTED (local) after B2 r3 pass (reviews/p4-code-r3.md): verify.sh 105/105 incl. real-model + keyword selectivity guards. Carried (CI only): Linux isolation, model cache/download, extracted copy on fresh target, image job model-check ≤2,560 MiB (first Linux run = calibration). Frozen p4-code-r3 tree 6402e15c.
- P3 ACCEPTED (local) after B2 r2 pass (reviews/p3-code-r2.md): B2 reran verify.sh 77/77, race x3 with lock waits, probes F1–F4 fixed. Minors n1 (expires_at retry semantics in schema docs), n2 (µs timestamp normalization on re-record), n3 (replay existence oracle → not_found) carried into P4 scope + review. Frozen p3-code-r2 tree 08f82f2a.
- P2 ACCEPTED (local) after B r2 explicit pass (reviews/p2-code-r2.md): B reran verify.sh 59/59 on private cluster, authority x3 stable. Carried to final gate as unavailable: first CI run of memory-service.yml (verify job incl. Linux isolation + image job), Docker non-root image. Frozen p2-code-r2 tree 9e95a871.
- P1 gate: CLOSED 10:50Z. Majors fixed r1, minors r2/r3 fixed; R1–R3 verified closed in code by B (p2-code-r1).
- P1 expansion: plan/memory-service/implementation-decisions.md (+ main.rs registration). Frozen p1-arch-r1 tree 8a87f16b sha256 f136fd6a…. Under B review.
  A-flagged user decisions: (1) provider_max_level per tenant / ZDR / inference_geo for Anthropic (blocks enabling real generation, not building); (2) OpenBot provisioner admin credential vs manual provisioning (05 gate); (3) paid smoke budget (06).
  B r1 (reviews/p1-arch-r1.md): findings to resolve; sent to A as correction round 1.
  A r1 corrections: 6 major fixed, 13 minor fixed, m8 accepted. Frozen p1-arch-r2 tree 2029bb80 sha256 dd90dd6e…; B recheck assigned.
  B r2 (reviews/p1-arch-r2.md): 5 new minors (n1 split-batch keys, n2 D13 leftovers, n3 source retention vs live gen, n4 credential endpoint/provisioner enable, n5 non-[env] parent cargo key). Sent to A as correction round 2.
  A r2 corrections: n1–n5 fixed (n5 by detection: boundary check fails on any parent cargo key). Frozen p1-arch-r3 tree fe47e569 sha256 1b0ef1c0…; B limited recheck assigned.
  B r3 (reviews/p1-arch-r3.md): R1 sticky operator disable, R2 grant-vs-namespace-delete lock, R3 array-valued parent cargo keys always fail + boundary in focused.sh. Folded into P2 (A updates record + implements); B verifies in P2 code review → P1 gate closes then.
  User decisions 2026-11-08 (2nd round): provider gets ALL levels by default; no ZDR/region requirement now; OpenBot holds one provisioner admin credential (create/disable/delete tenants, namespaces, client creds; no direct content read).
  A-flagged env: local rustc 1.96 vs MSRV 1.95; local sqlx-cli 0.9.0 vs required 0.8.6.

## Resources (root-owned)
- Disposable private PG cluster: tmp/memory-service-run/pg-cluster.sh (data tmp/memory-service-run/pgdata), PostgreSQL 18.6 Homebrew + pgvector 0.8.7, 127.0.0.1:55432, trust, superuser postgres, no unix socket, fsync off. URL postgres://postgres@127.0.0.1:55432/postgres. Created by root (B r1 suggestion) to unblock DB tests without user's server or credentials. Serialized: A only. Destroy at task end.
- sqlx-cli 0.8.6 local install target: tmp/memory-service-run/tools (A2).

## Open items
- 06A carry (service change): credential list unpaginated/limit 1000 + revoked creds never removed → tenants reaching 1,000 creds cannot provision/delete namespaces; needs label-filtered or paginated list.
- P7b: frozen p7b-code-r1 tree 0df92996 sha256 37e1ac74…; incr from p7b1 171746f7…, from p7a fbe4f511…. 119/119 focused+regressions; clippy clean. RESERVED_DATABASE_CONNECTIONS 12→13 (raised limit; B5 to verify early-fail check). Rotation tests previously swept real memory creds in shared test DB (shadow table fixed).
- P7b substep 1: frozen p7b1-code-r1 tree c2e6f609 sha256 a571f978…; incremental ea4c3ee9…. memory_integration_ 9/9, memory_scope_ 1/1, legacy-schema 1/1, credential_rotation 13/13 (private cluster db open_bots_p7b_test). transport-boundary-check.sh fails identically at base a6bed48a (B4 verified) — PRE-EXISTING, not in scope; report at final gate.
- Disk: root cleaned services/memory/target (15.2 GiB, task-created; model assets untouched) → 19 GiB free. Final gate will rebuild the service.
- USER DECISION 2026-11-08 (5th round): company-scope saves treat AuthoredConfiguration as inexact (ceiling fallback); agent/user scope keep exact 0. Sent to A6 (record text now, code in P7c); B to verify in P7b/P7c review.
- USER DECISION 2026-11-08 (4th round): U2 REFINED — ConversationalInput (human messages, manual/interactive input, decision.human, message.ask replies) → inexact (ceiling fallback = lowest-reader / actor level); AuthoredConfiguration (instructions, skills, workflow literals, schedule params) stays exact 0. Sent to A6 to apply.
- User decisions 2026-11-08 (3rd round): U1 memory opt-in per company (owner/admin); U2 human-authored inputs = level 0 (existing semantics; risk documented); U3 auto recall via SearchMemory binding; capture private sessions → actor user scope (no tool results) AND thread capture into agent scope (user chose over reviewer rec) — root direction: gate thread capture behind per-agent capture policy, off by default, owner/admin enabled, cross-channel consequence documented.
- P7a: section E1–E12 in implementation-decisions.md; frozen p7a-arch-r1 tree f723a92e sha256 328b268e…. A6 user questions U1 (per-company opt-in), U2 (authored config = level 0 for exact accounting), U3 (auto recall via SearchMemory binding; private-session capture to actor user scope, tool results never captured) — B4 assessing before asking user.
- Evidence invalidation (B3): b3-p6b-r2-generation-run1..5/-parallel1..3 ran a stale A+D mutant binary (shared target collision); B3 r2 verify 152/152 and mutation results stand. B3 ran `cargo clean -p memory-service` (~19 GiB freed, task-created build output). Rule: mutation probes use a separate CARGO_TARGET_DIR.
- Carried to 06A: rotated write-up credential loses reuse of predecessor-recorded revisions (rotation docs).
- P6b r3: frozen tree 482af7df sha256 92876843…; incremental 8ab7e91a…. generation 17/17 ×3; verify.sh 153/153; author mutations A/D/A+D now fail.
- Root direction P6b m2: write-up credential reuse of its own above-read revisions allowed only when digest matches AND same recording credential; else uniform 409 (keeps phase-05 capture working).
- P6b r2: frozen tree 42604f1e sha256 652c69a5…; incremental 8ebae747…. generation 16/16 ×3; verify.sh 152/152.
- Carried to 06A: UNIQUE(tenant, idempotency_key) vs idempotency sweep (drop or map 23505→409); auth-failure circuit breaker; purge (n2 parent rows); paid smoke budget. Carried to 06B: black-box serve test vs fake Messages API.
- P6b evidence: observation 13/13, generation 11/11, contract 12/12, mcp 6/6, authority 24/24, storage 14/14, idem 2/2, startup 14/14, search 18/18; verify.sh 147/147. Frozen p6b-code-r1 tree 41668d9e sha256 a93941dd…; incremental 097c217c…. New keys MEMORY_ANTHROPIC_API_KEY/BASE_URL/EFFORT, MEMORY_GENERATION_WORKERS. A5 open: no black-box serve vs fake Messages API; UNIQUE(tenant,idem key) vs 06A sweep; no auth circuit breaker.
- P6a r2: frozen tree 54270382 sha256 a1113cbc…; incremental e460e780…. observation 13/13, contract 12/12, storage 14/14, idem 2/2; verify.sh 135/135 (64 DB). Thinking: between_tools per claude-api skill (B3 to verify).
- P6a evidence: observation 11/11, context 11/11, recall 5/5, contract 12/12, mcp 6/6, storage 14/14, access 14/14, boundary 3/3, search 18/18, idem 2/2, authority 24/24, startup 13/13; verify.sh 133/133 (63 DB). Frozen p6a-code-r1 tree 668cd7d8 sha256 01b6957f…; incremental 7f695f92…. generations.* still 501 until P6b. New deps regex, reqwest =0.13.4 (aws-lc-sys).
- P5 evidence: recall 5/5, context 9/9, mcp 6/6, search 18/18, storage 14/14, access 14/14, authority 24/24, contract 12/12, startup 13/13, boundary 3/3; verify.sh 120/120 (60 DB). Frozen p5-code-r1 tree 0e841af1 sha256 0d69202a…; incremental 83724147…. A4 dev DB mema4dev on cluster (mema3dev dropped).
- P4 r2: frozen tree 6efd4f49 sha256 0930a4db…; incremental b9c8b2a7…. search 18/18, storage 14/14, startup 13/13, contract 12/12, verify.sh 105/105. RSS 2.34 GB, CI budget 2,560 MiB.
- Disk: user freed space; 35 GiB free (83%) at ~12:40Z. Root OpenBot build in this worktree (phase 05/06) now fits.
- P4 evidence: search 16/16 (real model), storage 14/14, idem 2/2, access 14/14, authority 24/24, contract 11/11, mcp 5/5, startup 11/11, boundary 3/3; verify.sh 100/100 (48 DB). Model downloaded+verified 515 MB (services/memory/assets/model, gitignored). Frozen p4-code-r1 tree 82be501c sha256 016a7de8…; incremental c2301076…. Root decision: keep two ORT sessions (≈2.5 GB, reserved query slot for latency target), CI RSS budget 3,072 MiB as early-failing check. A3 dev DB mema3dev on private cluster (A3 to drop). Disk 5.4 GiB free; services/memory/target 6.6 GB.
- Disk: user chose cleaning old fix-app worktree targets; root ran `cargo clean` in .worktrees/fix-app-p03 and fix-app-p04 (3.3 GiB each; their source/git state untouched, they have uncommitted source changes — preserved). Free 11 GiB at ~11:50Z.
- P3 r2: frozen p3-code-r2 tree 08f82f2a sha256 ed921567…; incremental from r1 c62c89e4…. Evidence storage 10/10, idempotency 2/2, access_level 14/14, authority 24/24, mcp 5/5, contract 10/10; verify.sh 77/77 (37 DB).
- Root decision (P3 F4): delete is a write; bounded by write_max (403 above), like correction. Routine authz choice from 01B.2, not a product decision.
- Root direction (P3 arch): keep cross-namespace classification-preserving copies (required by originals); may restrict recorded sources to target namespace; cited namespaces locked/rechecked in D13 order; link ON DELETE/sweep behaviour defined.
- P3 evidence: storage 7/7, idempotency 2/2, access_level 14/14, authority 23/23, contract 10/10, mcp 4/4, startup 9/9, boundary 3/3; verify.sh 72/72 (32 DB). Frozen p3-code-r1 tree 844c2d77 sha256 8a48100b…; incremental from P2 sha256 7c794334…. Disk 5.0 GiB free.
- P2 r2 evidence: focused authority 22/22 (15 DB), startup 9/9, boundary 3/3, access_level 12/12, contract 9/9, mcp 3/3, storage 1/1; verify.sh 59/59 (19 DB) incl. owner migrate + prepare --check; extracted 59/59 (MEMORY_EXTRACT_UNTRACKED=1, shared target). Frozen p2-code-r2 tree 9e95a871 sha256 92a87fc1…. Unavailable: Linux isolation (F3), CI jobs, Docker image.
- P2 return: 66 files, frozen p2-code-r1 tree 60bf672e sha256 04cea2c0…. Non-DB focused: contract 7/7, mcp 1/1, access_level 12/12, authority 5(+8 DB unavail), startup 7(+2), boundary 2(+1); probe, clippy, check, fmt pass. Unavailable: 11 DB tests, verify.sh, extracted-build, sqlx migrate/prepare, Docker. A1 handoff in implementation-decisions.md.
- Disk: 6.5 GiB free (97%) at 10:33Z; hit 100% during P2. services/memory/target 1.3 GB.
- sqlx-cli 0.8.6 not installed locally (install to a local --root when needed).
- DB access: user authorized silently sourcing /Users/mac03/dev/openbot/.env (DATABASE_URL) for test runs only; user is creating that file (absent as of 05:40Z). Verify before DB-backed work. Never print it; never migrate/reset the dev DB itself — disposable memory_test_* databases only.

## Workflow switch (2026-11-08 ~20:10Z)
User switched to implement-opus-reviewed-2_2. For phase 05 onward the authoritative records are
plan/memory-service/phases/p05/docs/TASK.md and HANDOFF.md (phase-local continuation; this file and
reviews/ are retained unchanged as history). Coordination: plan/memory-service/COORDINATION.md r1.
User decisions: root confirmed Opus/xhigh and authors batches after p05-B2; A gen8 (high) finishes
p05-B2 (whole 05A remainder incl. agent tools + recall/save race); a new xhigh reviewer reviews it.
Context: auto-compaction at 75% (env); no retirement threshold.
