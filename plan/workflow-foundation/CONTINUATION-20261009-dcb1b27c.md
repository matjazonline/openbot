# 04.7 continuation — root dcb1b27c (2026-10-09)

Scope: finish original 04.7 only; no 04.8. Supplements
[CONTINUATION-20261005-01a10bcd](CONTINUATION-20261005-01a10bcd.md).
Workflow: implement-opus-reviewed (A/B = opus/high general-purpose workers).

## Baseline

- HEAD `8ed25b4` ("workflow pending 4.7") — the user committed the previously
  inherited WIP; working tree clean at start. Prior frozen source838 manifest
  (SHA256 `9ebfe264…2012678`) is verified by A gen1 step 0.
- Root session `dcb1b27c-5daa-4f2c-92f7-25613eeb0a15`, model claude-opus-5-5.
  Context window declared 200,000 as a conservative lower bound (no /context
  reading); root 78,274 tokens at 2026-10-09T16:57:55Z.
- User ran /context in root: 104.9k/1M (10%) for claude-opus-5-5 — root window
  is now declared 1,000,000 (provenance: /context); 141.7k/1M (14%) at second /context. Workers keep the 200,000
  conservative lower bound (no authoritative worker reading).

## Workers

| Role | Gen | Nonce | Status |
| --- | --- | --- | --- |
| A | 1 | run=k7q2dcb1 role=A gen=1 | retired 50.18% after step0 + design (no code); baseline 838/125 match; NEWPG 55440 started by A, identity verified |
| B | 1 | run=k7q2dcb1 role=B gen=1 | retired 52.95%; design gate conditional PASS, F1–F5 to verify at code gate (`reviewer-row3d-arrays-k7q2/DESIGN-REVIEW.md`) |
| A | 2 | run=k7q2dcb1 role=A gen=2 | done, 49.1% (98,167/200k) — no headroom, retired; all gates PASS per author |
| B | 2 | run=k7q2dcb1 role=B gen=2 | code PASS, 44.37% — retired (next review is unrelated scope) |
| A | 3 | run=k7q2dcb1 role=A gen=3 | design written (G1 validity, G2 chronology), reuse check 45.28% (≥45% cutoff) — retired without G1 edits |
| A | 4 | run=k7q2dcb1 role=A gen=4 | G1 implemented, author gates PASS, 46.03% — retired; g1-diff.patch sha256 42232371…14fcd |
| B | 4 | run=k7q2dcb1 role=B gen=4 | G1 code PASS, 63.49% — retired |
| A | 5 | run=k7q2dcb1 role=A gen=5 | G2 edits done, focused 2/2 PASS; retired 53.31% before gates; 4 F4 deviations reported |
| A | 6 | run=k7q2dcb1 role=A gen=6 | F4 deviations closed (d: execution_id exists — predecessor claim wrong), all G2 gates PASS, 45.83% — retired; g2-diff.patch sha256 581b588c…3a14 |
| B | 5 | run=k7q2dcb1 role=B gen=5 | G2 code PASS (N1 comment nit non-blocking), 55.6% — retired |
| A | 7 | run=k7q2dcb1 role=A gen=7 | done 47.85% — retired: N1 comment fix (n1-diff.patch sha256 2d302e40…bd5c) + storage DESIGN.md (S1/S2/S3) |
| B | 6 | run=k7q2dcb1 role=B gen=6 | STOPPED by root on user stop request at 29.79%, before any record was written; N1 + storage design UNREVIEWED |
| B | 3 | run=k7q2dcb1 role=B gen=3 | design PASS w/ F1–F4 + Q1–Q5 decisions (`reviewer-row3d-chronology-k7q2/DESIGN-REVIEW.md`), 48.39% — retired |

## Points

| Point | Status | Evidence |
| --- | --- | --- |
| row3d native array + SQL-jsonb-text 16384/16385 | **ACCEPTED (scoped)** 2026-10-09: independent code PASS (`reviewer-row3d-arrays-k7q2/CODE-REVIEW.md`, reviewer rerun focused 4/4) + author focused 4/4, fmt/offline all-targets/strict Clippy/SQLx prepare+check/graft/manifest (837 unchanged + 2 new + 2-line registration, 125 protected) PASS; frozen diff sha256 46824a73…f277 | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-arrays-k7q2/` |
| row3d G1 registered validity | **ACCEPTED (scoped)** 2026-10-09: independent code PASS (`reviewer-row3d-chronology-k7q2/G1-CODE-REVIEW.md`, reviewer rerun native_validity 2/2 + native_evidence_tests 6/6) + author fmt/offline all-targets/strict Clippy/SQLx check/graft PASS; g1-diff.patch sha256 42232371…14fcd | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-chronology-k7q2/` |
| row3d G2 covered-entry chronology | **ACCEPTED (scoped)** 2026-10-09: independent code PASS (`reviewer-row3d-chronology-k7q2/G2-CODE-REVIEW.md`, reviewer rerun native_chronology 2/2 + native_evidence_tests 8/8; F4 a–d verified incl. exact NULL-safe equivalence of split disjunct) + author all gates PASS; g2-diff.patch sha256 581b588c…3a14. Open non-blocking N1: fixture :79-81 comment wording | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-chronology-k7q2/` |
| row3d N1 comment-only fix (chronology fixture :79-83) | implemented, NOT reviewed (n1-diff.patch 2d302e40…bd5c; base copy chronology_fixture.g2.sql) | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-storage-k7q2/` |
| row3d stored-evidence relational owner + safe-storage/no-secrets scope | design written, NOT reviewed; S2 needs USER decision | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-storage-k7q2/DESIGN.md` | `/Volumes/ssd1/dev/workflow-recovery-20261005/author-row3d-storage-k7q2/` |
| row3e natural COMMIT / full logical / injected rollback | queued | |
| row3f populated upgrades | queued | |
| final 04.7 integration + independent review | queued | |

## Notes

- The 125 protected hashes live in `row3c-protected-baseline-20261005.json`
  (sha 31a62566…bb44e; non-src .cargo/.github/.sqlx files), not in source838.json.
  B gen4 verified all 125 match after G1.

## Stop checkpoint — 2026-10-09T18:37Z (user: "stop when adding author return")

Stopped on user request after recording A gen7's return. All workers quiescent:
B gen6 was stopped (TaskStop) before writing anything; no worker running.
Root 240,673/1,000,000 = 24.07% at 18:36:43Z.

Uncommitted tree (nothing staged): RESUME.md (new top pointer section), 2-line
registration in `action_reconciliation_native_diagnostic_tests.rs`, six new native
test/fixture files (object, validity, chronology), this record. NEWPG 55440 left
running (started by A gen1; identity 7693072113847127473, 0 other clients at stop).
55439 never touched. No commit/stage/deploy/production/schema/dependency/bound change.

### Next action (fresh A/B)

1. Fresh B: review N1 (comment-only) and the storage DESIGN.md. **Independently
   verify** its factual claims before the user decides S2: no stored-evidence
   decoder (`VerifiedEvidence` private and not Deserialize; diagnostic column
   write-only at facts.rs:31,40); immutability triggers 20260930180000:114-118;
   no production caller or verifier (test support only); three stored
   channels kept as written, size-bounded only — `UnknownNote.note` ≤4096
   (verification.rs:155), `attestation.diagnostic` arbitrary JSON (verification.rs:96-103,207-213),
   recovered output schema-checked only; no sanitizer. Assess A/C blast radius.
2. Put S2 to the user with the verified facts (BRIEF-04.7:108 "No secrets/raw
   transport payloads are stored" is not met application-wide today):
   - A: production change — closed typed diagnostic + drop/restrict free-text notes.
   - B: scoped criterion statement, no code; confidentiality deferred to first
     production verifier (append-only rows mean a leaked secret is permanent).
   - C: B + typed diagnostic only; notes remain operator content.
3. Then S1 criterion map (+ optional T1/T2), S3 projection test under the chosen
   scope; row3e, row3f, final 04.7 integration (broad suite still has no PASS).

### Usage (this transcript only)

`--totals` sees only the current root transcript (root 50 req: input 113, cache
write 586,531, cache read 7,042,764, output 50,037; A gen7 and B gen6 rows). Earlier
worker generations in this root are not in the visible transcript set: their spend
is unknown here, not zero. Per-agent subagent token counts from task notifications:
A1 106.5k, B1 116.8k, A2 ~127k, B2 92.9k, A3 91.2k, B3 99.5k, A4 97.0k, B4 132.8k,
A5 108.9k, A6 94.9k, B5 115.4k, A7 97.2k (context totals, not billed spend).

## User decision — 2026-10-09: S2 = option C

User chose **C**: scoped criterion statement for 04.7 (bounds, grammar, no transport
fields in the verifier port, no free text in outcome/coverage/conflict rows, S3
allowlist proof), PLUS a production change making the verifier attestation
diagnostic a closed typed structure. Operator notes (`UnknownNote.note`) stay
operator content; their confidentiality and recovered-output confidentiality are a
named follow-up for the first production verifier. Resumed after the stop
checkpoint: fresh B gen7 reviews N1 + storage DESIGN facts + C blast radius before
any production edit.

## Resume after stop — root session 8bab464f-f25b-4a38-81ec-8a76f826f79f

Root session UUID after the stop is `8bab464f-f25b-4a38-81ec-8a76f826f79f`
(earlier workers are under `dcb1b27c…`); count both in usage. Workers from B gen6
on are measured against 8bab464f.

- B gen7 (`run=k7q2dcb1 role=B gen=7`, measured by root 113,139/200k = 56.57%,
  retired): N1 PASS; storage DESIGN PASS, claims C1–C6 verified; correction D4 —
  typed diagnostic serialized `{"ledger":…,"version":1}` keeps stored native
  fixture bytes; real rework is Rust boundary tests that pad the diagnostic
  (boundary_tests.rs:34-37, :74-98; tests.rs:256). D5 (notes path) → notes
  follow-up. Record: `reviewer-row3d-storage-k7q2/REVIEW.md` (incl. BRIEF:108
  scoped criterion text, Part 3).
- **N1 ACCEPTED** (comment-only; independent PASS; n1-diff.patch 2d302e40…bd5c).
- Option C architecture gate PASS (reviewed shape + blast radius): production
  change in verification.rs only; no migration/.sqlx change.
- Next: A gen8 implements C.
- A gen8 (`gen=8`, retired 54.3%): option C implemented — `AttestationDiagnostic`
  in verification.rs; tests.rs verifier loop 0..9 → 0..8 (oversized-diagnostic
  variant dropped — reviewer must judge); boundary tests replaced (closed
  serialization, max envelope = 954 bytes literal, canonical 16384/16385).
  c-diff.patch sha256 fa88cc48…99a2 in `author-row3d-typed-diag-k7q2/`.
  Boundary 6/6 PASS. `--lib -- reconciliation` NOT clean: 186/3 then 187/2,
  varying lease/timing tests (each passed alone in run 1). Cause unknown.
- A gen9 (`gen=9`): reruns of failures + remaining gates for C.
- A gen9 (`gen=9`, ~40%, done): no code change; c-diff.patch unchanged
  (fa88cc48…99a2). All 5 earlier failures pass alone. Third broad
  `--lib -- reconciliation` 187/2 FAIL (claim_cancellation_claim_first lock-wait
  Elapsed; bounds_128_prior_consuming_129_total_then_lost_refuses lease timeout —
  same test/message in pre-C BROAD835.md); both pass alone. Not relabelled.
  application reconciliation 16/16, fmt/diff-check/offline all-targets/strict
  Clippy/sqlx prepare --check/graft PASS; only verification.rs in production;
  migrations/.sqlx unchanged; 125/125 protected; NEWPG 0 clients.
- B gen8 (`gen=8`, retired 50.88%): option C code PASS
  (`reviewer-row3d-typed-diag-k7q2/CODE-REVIEW.md`): shape exact; dropped
  oversized case unconstructible (ids ≤128 bytes); 954-byte max verified (4-digit
  years); canonical test hits production freeze::canonical; no other JSON path;
  C cannot plausibly cause the two broad failures. Reran app reconciliation 16/16,
  native_evidence_tests 8/8. Non-blocking notes: 954 comment "4-digit years";
  const assert uses literal 16_384.
- **Option C ACCEPTED (scoped)** 2026-10-09: c-diff.patch sha256 fa88cc48…99a2.
  Broad `--lib -- reconciliation` remains failing on varying lease/timing tests
  (pre-existing pattern, cause unknown) — blocks final 04.7 acceptance, not C.
- Next: A gen10 — S1 criterion map + S3 owned-adapter projection test (+T1/T2 if
  small) per storage DESIGN.md as corrected by REVIEW.md, plus scoped BRIEF:108
  evidence record.
- A gen10 (`gen=10`, retired 50.29%): S1-CRITERION-MAP.md (incl. scoped BRIEF:108
  text + follow-ups) and S3 projection test (new
  `action_reconciliation_native_projection_tests.rs` + 2 registration lines in
  `action_reconciliation_native_evidence_tests.rs`; projection + T2 catalog; T1
  omitted, covered by append_only tests). s3-diff.patch sha256 6da5b245…7728.
  Author gates: new 2/2, native_evidence 10/10, app reconciliation 16/16, static
  gates, manifest, 125 protected, NEWPG 0 clients — PASS.
- B gen9 (`gen=9`): review of S1 map + S3.
- B gen9 (`gen=9`, retired 58.3%, no test rerun): CHANGES REQUIRED
  (`reviewer-row3d-storage-k7q2/S1-S3-REVIEW.md`): F1 sentinel leak check
  vacuous (wrapper never passes raw); F2 BRIEF:108 text overclaims —
  recovered_result and authoritative_reference are verifier-supplied and stored;
  F3 conflicts-table immutability trigger (M:122) has no test; F4 scan blind to
  bytea (whole-table delta covers it).
- A gen11 (`gen=11`): fix F1–F4 (F3: add conflict-row UPDATE/DELETE test or mark
  explicit unproven follow-up).
- A gen11 (`gen=11`, retired 50.46%): F1 vacuous sentinel scan removed, docs
  credit closed type; F2 BRIEF:108 text now names stored verifier-supplied
  authoritative_reference (M:16 grammar CHECK) and recovered_result (schema only);
  F3 new `action_reconciliation_conflict_immutable_tests.rs` (genuine service-path
  conflict row; UPDATE×2/DELETE rejected P0001 by M:122, rows unchanged),
  registered at late_truth_tests.rs:8-9; F4 map wording. fix-diff.patch sha256
  bc5b4d40…0b75. Author gates: 3/3 + 37/37, static, 125 protected PASS.
- B gen10 (`gen=10`): recheck F1–F4 + first independent test rerun.
- B gen10 (`gen=10`, 42.07%, eligible for reuse): S1/S3 recheck — F1, F3, F4
  PASS; code + tests PASS with first independent rerun (native_projection 2,
  conflict_immutable 1, native_evidence 10, append_only 4, late_truth 10, app
  reconciliation 16). F5 (map wording): recovered output also bounded by
  MAX_ARGUMENT_BYTES (dispatch.rs:202-203) and receipts.result CHECK ≤131072
  (20260930120000:39). Record `reviewer-row3d-storage-k7q2/S1-S3-RECHECK.md`.
- A gen12 (`gen=12`): F5 wording fix only.
- A gen12 (`gen=12`, 33.22%, done): F5 map wording, f5-diff.patch df2b7316…59aa6.
  B gen10 F5 recheck PASS (44.79%, retired — no headroom for integration review).
- **S1 map + S3 projection + F3 conflict-immutability test ACCEPTED (scoped)**
  2026-10-09: s3-diff.patch 6da5b245…7728 + fix-diff.patch bc5b4d40…0b75 +
  f5-diff.patch df2b7316…59aa6; reviewer rerun focused tests all PASS.
- Next: fresh B gen11 — whole original row3d integration review (all scoped
  acceptances vs BRIEF-04.7 row3d), then combined affected test gate.
- B gen11 (`gen=11`, retired 62.14%): row3d integration CHANGES REQUIRED
  (`reviewer-row3d-integration-k7q2/INTEGRATION.md`): G1 no native test for
  evidence column grammar/length CHECKs (20260930180000:13-16); W1 BRIEF:103
  "note ≤4096 at SQL" only indirectly bounded (diagnostic ≤16384) — direct bound
  needs a schema change → USER DECISION pending. Proposed combined gate (subset
  filters, not bare receipt_tests) + static + stack-budget + independent rerun.
  Broad intermittent failures block final 04.7, not row3d, if subset is clean.
- A gen13 (`gen=13`): G1 native grammar/length matrix + S1 map row fix.
- A gen13 (`gen=13`, retired 55.5%): G1 native grammar matrix — 4 columns × 3
  cases (128 commit / 129 → 23514 / bad grammar → 23514), new
  `action_reconciliation_native_grammar_tests.rs` + `_fixture.sql`, 2 registration
  lines in native_object_tests.rs. g-diff.patch bd44696f…2d17; map-g1-diff.patch
  eac21d75…68d. Author gates PASS (grammar 4/4, native_evidence 14/14, static,
  125 protected). Flagged: map still credits native tests for command_key/digests/
  disposition/signature CHECKs — possibly also untested.
- B gen12 (`gen=12`): G1 code review + check that second map row.
- W1 (note ≤4096 at SQL) awaiting user: A accept indirect bound + follow-up
  (recommended) vs B add direct CHECK (schema change).
- B gen12 (`gen=12`, retired 55.02%): G1 grammar code PASS, reviewer rerun
  native_grammar 4/4 + native_evidence 14/14 (`reviewer-row3d-grammar-k7q2/CODE-REVIEW.md`).
  **G1 grammar matrix ACCEPTED (scoped)**: g-diff.patch bd44696f…2d17,
  map-g1-diff.patch eac21d75…68d (in author-row3d-grammar-k7q2/). New gap F1:
  command_key/request_digest/coverage_digest/disposition/operation_signature
  CHECKs (M:9-12,17) untested; map credited nonexistent native scalar/array tests.
- A gen14 (`gen=14`): F1 matrix + map row fix + scan map for other phantom credits.

## User decision — 2026-10-09: W1 = option A

BRIEF-04.7:103 "note ≤4096 at SQL" is accepted as bounded indirectly for 04.7:
Rust `validate()` enforces note ≤4096 (contracts.rs:39-43) before write; SQL
bounds the whole diagnostic (which carries the note) at ≤16384 octets
(20260930180000:21). No schema change. A direct SQL note CHECK is a named
follow-up for the first production verifier, alongside notes/recovered-output
confidentiality and D5. The S1 map W1 row must state this exactly (next author
edit after A gen14 returns; reviewer verifies wording).
- A gen14 (`gen=14`, retired 51.6%, no edits): F1 design notes in
  author-row3d-grammar-k7q2/IMPLEMENTATION.md "A gen14".
- A gen15 (`gen=15`): F1 constraint matrix from those notes (code only).
- Then: small author for S1 map edits (F1 row, W1 row per user decision A, scan for
  other phantom test credits) → reviewer → combined row3d gate.
- A gen15 (`gen=15`, retired 55.5%, no edits): arrived ~45% (system prompt +
  injected AGENTS.md files); an output-compression hook (caveman) hides Rust
  function bodies in Read/cat output and caveman_retrieve was denied —
  workaround `sed -n A,Bp FILE | sed 's/^/|/'`. Pinned all 5 constraintdefs,
  single-constraint negatives verified in catalog, 128-byte command_key positive
  looks provable. Ready-to-implement spec in IMPLEMENTATION.md "A gen15".
- BLOCKED on worker capacity: two consecutive authors retired before editing.
  Asked user (a) authoritative worker context window (200k is only a declared
  lower bound) and (b) whether the compression hook may be disabled/allowed for
  workers in this repo.

## User decision — 2026-10-09: worker window 1M; caveman voice off

User stated worker context window = 1,000,000 (provenance: user statement).
Workers from gen16 on measure with --context-window 1000000; 50% retirement =
500k. User said "stop caveman": terse response voice turned off. The output-
compression hook is separate config; workers are told to read code with
`sed -n A,Bp FILE | sed 's/^/|/'` and to report any elided/compressed output.
- A gen16 (`gen=16`, 13.7% of 1M): F1 constraint matrix implemented — new
  `action_reconciliation_native_constraint_tests.rs` (484 lines) + `_fixture.sql`,
  2 registration lines in native_object_tests.rs (fmt-ordered after :371).
  17 single-constraint negatives (23514 each) + 5 positives incl. 128-byte
  command_key boundary; constraint lookup also filters table (sibling tables share
  definitions). f1-diff.patch 40d19428…8957. Author gates: new 5/5, grammar 4/4,
  native_evidence 19/19, static, 125 protected PASS. Reused for S1 map edits
  (F1 row, W1 row per user decision A, phantom-credit scan).
- A gen16 map edits (18.8%): map-f1w1-diff.patch 109db8c1…fcb9 — F1 row → new
  constraint tests; W1 row per decision A (+ follow-up 4 direct SQL note CHECK);
  citations corrected to real tests. GAP rows (no test): M:27 time order;
  check1/check2 M:28-34; evidence→dispatch FK M:25-26; coverage FKs M:43-46;
  command→dispatch/audit/job FKs M:62-63/67/68; scheduled-job CHECK M:71-72;
  commit-guard missing-coverage and >128-coverage.
- B gen13 (`gen=13`, 1M window): review F1 code + map, and decide which GAP rows
  original row3d actually requires.
- B gen13 (`gen=13`, 21.75% of 1M): F1 code PASS + map PASS
  (`reviewer-row3d-constraint-k7q2/REVIEW.md`); reran constraint 5/5, grammar 4/4,
  native_evidence 19/19. **F1 constraint matrix + map-f1w1 edits ACCEPTED
  (scoped)**: f1-diff.patch 40d19428…8957, map-f1w1-diff.patch 109db8c1…fcb9.
  Required row3d gaps: R1 M:27 time-order CHECK; R2 check1 all-or-none (registered
  NULL-each ×5 + unregistered with one registration value + unregistered positive);
  R3 check2 grant_eligible on Unknown — root decision: test it in row3d (same
  fixture, cheap) rather than defer. Out of row3d (relabel in map, carry to final
  04.7): evidence→dispatch/coverage FKs, command→dispatch/audit/job FKs,
  scheduled-job CHECK, commit-guard missing-coverage, >128 coverage (row3c).
  Non-blocking: N1 ref not measured, N2 config table emptiness, M1 applied_request
  owners (181000:3-22) listed as out of row3d.
- A gen16 reused for R1–R3 + map relabels.
- A gen16 R1-R3 (24.6%): new `action_reconciliation_native_coherence_tests.rs` +
  `_fixture.sql` (9 single-CHECK negatives, 5 measured positives, N2), map relabels
  (no GAP rows left). r-diff.patch 514d890b…f901, map-r-diff.patch 14ecb687…c412.
  Author: coherence 4/4, native_evidence 23/23, static, 125 protected PASS. First
  native_* run 46/1 (`high_bounds_native_owners` lease transaction timeout, not a
  new file; passed alone), rerun 47/47 — not relabelled; noted for combined gate.
- B gen13 reused: review R1-R3 + map.
- B gen13 R1-R3 review PASS (25.94%), reran coherence 4/4 + native_evidence 23/23.
  **R1–R3 coherence tests + map-r ACCEPTED (scoped)**: r-diff.patch 514d890b…f901,
  map-r-diff.patch 14ecb687…c412. Non-blocking N3: coherence file 597 lines
  (<1000 split rule, >EXPANSION's <500 guidance).
- All row3d points now scoped-accepted. Next: combined row3d gate on final tree
  (A gen16), then independent rerun (B gen13), then full row3d acceptance.
- Combined row3d gate (A gen16, 26.3%) **BLOCKED**: 196 tests, 195/1 —
  `bounds_128_prior_consuming_129_total_then_lost_refuses` lease transaction
  timeout at receipt_tests.rs:210 (concurrent claim_io); passes alone. Filter
  `receipt_tests::provider_tests` is a prefix that includes prior/high bounds
  tests (INTEGRATION.md wrong on that); append_only has 1 test not 4. Final
  manifest row3d-final-manifest.json 65538643…4bf7 (21 src files @ 8ed25b4).
  Static gates not run.
- Root decision: do NOT narrow the filter to exclude the flaky test. The same
  lease-timeout family blocks final 04.7; investigate root cause first
  (A gen16, investigation only, no fix), then design gate, then fix.
- A gen16 lease diagnosis (36.0%): `row3d-combined-k7q2/LEASE-DIAGNOSIS.md`.
  Mechanism (author, unreviewed): fixed 1 s client-side wall-clock bound around
  whole lease ops (lease.rs:26-95 persistence_timeout) and 300 ms probe bounds,
  vs synchronous-commit WAL write stalls (0.35–0.94 s) on the shared external
  disk, amplified under parallel load by per-test DB setup WAL (~12 MB) and
  DROP DATABASE forced checkpoints. Ruled out: pool exhaustion, shared rows/locks,
  server timeouts. Runs: 195/1, 194/2 (4 threads), 196/196. Production risk:
  timeout after COMMIT sent → outcome actually unknown but reported as failure.
  Candidate fixes: (1) lease.rs bound redesign + post-timeout fence re-read;
  (2) harness template DBs / batched drops (+ checkpoint/latency gate);
  (3) 300 ms test deadlines → lock-state waits with measured gated durations.
- B gen14: independent review of diagnosis + candidate fixes before any decision.
- B gen14 (`gen=14`, 16.8%): lease diagnosis CONFIRMED with corrections
  (`reviewer-lease-k7q2/REVIEW.md`): one commit WAL wait 1.055 s (scripts dropped
  IO:WalWrite case-sensitively); DROP DATABASE forced checkpoints write ~3 GB/run;
  0.54 s read stall also seen; template DBs save ~1.3 s migrations, not WAL.
  Production risk real but liveness-only: post-COMMIT client timeout → phantom
  claim retired as Expired after lease expiry and attempt charged (recovery.rs:56-75);
  fencing stays consistent, no double ownership.
- Root authorizes (test/harness only): rewrite 300 ms probes (BEGIN…ROLLBACK,
  server lock_timeout, generous client timeout, measured duration logged);
  defer/batch per-test DROP DATABASE with leftover sweep + CI check on requested
  checkpoints; acceptance needs ≥5 green default-parallel combined runs + broad
  suite. No parallelism/sharding/bound changes. Fallback synchronous_commit=off on
  per-test DBs only if recorded and still failing.
- USER DECISION pending: production lease bound redesign + post-timeout fence
  re-read (BRIEF-04.7:20 forbids production bound change without approval).
- A gen16 reused: implement the root-authorized harness/probe fixes.

## User decision — 2026-10-09: lease fixes 1–3 AND production fix now

User approved the test/harness fixes (probe rewrite, deferred/batched drops with
checkpoint gate, ≥5 green combined runs) and the PRODUCTION lease fix inside 04.7,
explicitly overriding BRIEF-04.7:20 for this change: separate bounded pool
acquisition; overall bound derived from lease length instead of fixed 1 s;
post-timeout fence re-read so a committed claim/renew/release is not reported as
failure; applied at every site using the shared 1 s constant (adapters lease.rs,
action_dispatch.rs, action_receipts.rs, budget.rs, actions/dispatch.rs:176,
supervise.rs:54/72/87) with the four competing-claimant tests in
reviewer-lease-k7q2/REVIEW.md. Concurrency protocol change → design + independent
architecture gate before code. Serialized after A gen16's harness work (one author
at a time; both touch lease tests).
- A gen16 harness work (retired 43.4% — no headroom for production design):
  probe rewrite KEPT in action_reconciliation_completion_race_lock_support.rs
  (BEGIN…ROLLBACK, 100 ms server lock_timeout, 10 s hang guard, named failures);
  drop deferral REVERTED — removing per-test checkpoints moved writeback to client
  backends (277,803 vs 74,357 backend writes), max commit wait 0.66 s → 1.5–2.4 s,
  runs failed 14 and 6 (patch harness-deferral-rejected.patch 350272c4…7eee).
  Runs on final tree: 196/0, 195/1, 195/1, 195/1 (300 ms bound :284 wrapping a
  full completion commit), 192/4 (lease timeouts) — NOT 5 green. stack-budget.sh
  2462/1 (prior_bounds lease flake). lease-fix-diff.patch 2f768c92…97f8.
  Checkpoint gate script could not be wired: .github/workflows/ci.yml protected.
- Root: :284/:295 completion bounds get the same rewrite (within approved fix 1).
  Test-cluster writeback/synchronous_commit and Spotlight exclusion of PGDATA need
  the user. Next A gen17: :284/:295 rewrite + production lease design (no prod code).
- A gen17 (`gen=17`, 21.9%): part 1 bounds rewrite (:284 300 ms, :295 900 ms,
  plus :305 900 ms) via guarded() hang guard + backend-state dump; correctness
  from advisory gate/select! and complete_on lock_timeout 1000 ms
  (completion.rs:43). bounds-diff.patch 3f416388…5903; affected 3×2/2, static PASS.
  Part 2 DESIGN.md (lease-prod-k7q2/): bounds derived from lease length d
  (T=d/2, attempt=d/3, resolution=d/6, pool=A/2, server timeouts=T, outer guard
  T+d/12); post-COMMIT-timeout read-only re-read → applied/not applied/unknown;
  Timeout vs Conflict errors; invariant test, injection tests, lease-budget.sh;
  tests (a)–(k); no migration/.sqlx/dependency. Open Q1 cap at long leases;
  Q2 acceptance at 3 s test lease; Q3 include complete_io (completion.rs:27);
  Q4 CI wiring needs protected ci.yml; Q5 AppError::Timeout semantics.
- B gen14 reused: architecture gate on DESIGN.md + code review of bounds-diff.
- B gen14 design-gate review FAILED mid-run: API session limit (HTTP 429, resets
  01:40 Europe/Ljubljana). No reviewer verdict on DESIGN.md or bounds-diff.patch.
  Next on resume: re-send the same review assignment (B gen14 if still under 50%,
  else fresh B gen15); check reviewer-lease-k7q2/ for a partial DESIGN-REVIEW.md.
  No worker running; no production code edited.
- Correction: B gen14's DESIGN-REVIEW.md was written in full before the 429
  (`reviewer-lease-k7q2/DESIGN-REVIEW.md`). Code: ACCEPT bounds/probe rewrite with
  follow-ups C-1, C-2 (C-4, C-5 test-only). Design: ACCEPT DIRECTION, required
  D-1..D-6 before implementation. Decided: Q1 cap derived bounds; Q3 include
  complete_io (completion.rs:27); Q5 NotApplied → AppError::DatabaseTimeout.
  User questions: Q2 acceptance policy (+ fallback), Q4 ci.yml step for
  lease-budget.sh. Note: reviewer's Q2 list includes deferred/batched drops,
  which A gen16 measured as net-negative (reverted) — treat as open, not adopted.
- Next: A gen17 (reuse) revises DESIGN per D-1..D-6/Q1/Q3/Q5 and implements
  C-1/C-4/C-5; reviewer rechecks design before production code.
- A gen17 (25.4%): DESIGN.md v2 (ffacdd13…52d7e3; v1 DESIGN.v1.md) resolves
  D-1..D-6, Q1 caps (A=min(d/3,2s), R=min(d/6,1s), T=A+R, a=min(A/2,1s),
  G=T+min(d/12,0.5s)), Q3 complete_io via FencedWorkflowResult.policy, Q5
  DatabaseTimeout. Test fixes C-1/C-4/C-5 (c-fixes-diff.patch 8e6f90fd…dfd0):
  completion_race + claim_cancellation + control_serialization 3×11/11, static PASS.
  Remaining 300 ms-class bounds: control_serialization_lock_support.rs:128/:134,
  claim_abort_tests.rs:15, claim_race_tests.rs:50.
- B gen14 (22.87% before recheck): recheck design v2 + C-fixes.
- Still awaiting user: Q2 acceptance policy + fallback, Q4 ci.yml step.

## User decision — 2026-10-10: Q2 = b, Q4 = yes

- Q2: acceptance = deterministic resolution tests (a)–(o) + invariant test,
  5 consecutive green default-parallel row3d combined runs and 3 green broad
  reconciliation runs on NEWPG, each reporting the recorder summary (max phase
  times, resolved_applied, unknown) and 0 own DBs left. Deferred drops are NOT
  part of it (measured net-negative). If runs still fail: fallback (b) — accept
  recorder-evidenced `unknown` outcomes provided every affected test's assertions
  tolerate them through the lease-expiry backstop. No synchronous_commit change,
  no longer test leases, no lower parallelism.
- Q4: approved — add one step to protected `.github/workflows/ci.yml` after the
  stack-budget step (~:110) running `scripts/lease-budget.sh` (gates on the
  code-owned body-phase/statement budget, reports acquire/commit/unknown).
- B gen14 recheck (26.4%): design v2 PASS conditional on D-11 (heartbeat loops
  near run deadline → stop renewing once window.expires >= run_deadline; test (p)
  ≤1 renew in final stretch); zero safety margin spans 3–6 s leases → test (n)
  also at 6 s; D-5 server timeout = remaining A, lock_scope takes explicit
  duration; deleted job → Ok(None). Deferred-drop recommendation withdrawn.
  **c-fixes-diff.patch ACCEPTED** (8e6f90fd…dfd0; rerun 11/11).
  require_guard_above must rebase onto supervision_guard().
  Remaining short bounds: root-authorized hang guards now
  (control_serialization_lock_support.rs :128/:134/:255/:325/:350/:398/:413;
  claim_abort_tests.rs :37/:125/:148; delete private wait_for_lock copies in
  claim_race_tests.rs ~:50 and :37 → shared helper); with the production fix,
  claim_abort_tests.rs :114/:117/:172 rebased onto A/T/G.
- A gen17 (reuse): design v3 with D-11 etc., then implement production lease fix
  + tests (a)–(p) + lease-budget.sh + approved ci.yml step + all remaining bounds.
  D-11 verified by B at code review (B specified the fix).
- A gen17 (retired 42.3% — remaining test suite too large for headroom):
  DESIGN v3 (e0076076…; v2 kept). Production lease fix implemented at all
  sites: LeasePolicy capped A/R/T/acquire/guard bounds, persistence_timeout
  removed; supervise D-1/D-11; shared persistence_bound.rs wrapper for writes;
  read-only ops end in ROLLBACK; lock_scope explicit duration; NotApplied →
  DatabaseTimeout, unknown → Conflict. Tests: invariant, (k), (n) 3 s/6 s, (p);
  remaining bounds rewritten; private wait_for_lock copies removed. lease-budget.sh
  + ci.yml:115 step (ci.yml new sha 343e2864…f5fb, diff ci-yml.diff).
  prod-diff.patch 2562e09a…3e86 (61 files). Runs: affected 419/3 → 2 fixed
  message expectations, proof_expiry passed alone; 196-set once 194/2
  (proof_expiry 500 ms proof; 3 s test lease expired before assertion) — no
  lease-transaction timeouts, no unknown outcomes; stack-budget 2463/4 (load
  timing). Static gates PASS. NOT DONE: injection/competing-claimant tests
  (a)–(j),(l),(m),(o); recorder stall marker; release retry uses fresh bound
  (deviation from design: remaining T).
- Next: fresh A gen18 — finish tests + deviation + load flakes; then B code review.

## User steering — 2026-10-10: root checkpoint 70%

User raised this root's checkpoint threshold from 60% to 70% of its 1M window
(prospective, this root only). Worker retirement stays at 50%. Root at 41.83%
(418,271/1,000,000) when set. A gen18 (`gen=18`) writing lease tests (a)–(j),
(l),(m),(o), release-retry conformance, load-flake diagnosis.
- A gen18 (`gen=18`, 38.0%): tests (a)–(f),(g),(h),(i),(j),(l),(m),(o) +
  release-retry test with test-only slow-commit gate (lease_resolution_gate.rs);
  release retry conformed to design (≤A, capped at first deadline T; fresh bound
  could exceed guard G); recorder release mislabel fixed. Flakes fixed:
  proof_expiry (lifetime d/2), eligibility (expects provider Timeout),
  bounds_128 (loser may be definite NotApplied, exactly one claim). 196-set
  196/196 (resolved-not-applied 1, resolved-applied 1, unknown 0). stack-budget
  2479/2 (proof_clock 300 ms proof, 100 ms verification budget); lease-budget
  first FAIL (release body 568 ms > 500 ms) then PASS on rerun; affected 158/160
  (batch 250 ms deadline; reserve_remote NotApplied under load). Static PASS;
  ci.yml only protected change. prod-diff.patch 45854f12…1ab2 (71 files).
- Root decision (test-only, technical): retained_* scenarios get test heartbeats
  at snapshot-quiescent points, as a real worker would — not longer leases.
  Remaining sub-second test assumptions (proof_clock, batch 250 ms) and the
  reserve_remote NotApplied are diagnosed/fixed before review so the review
  covers a stable patch. lease-budget first-run failure kept on record.
- A gen18 part 2 (44.6%, retiring): retained_* heartbeats via production renew_io
  at snapshot-quiescent points; sub-second tests derived (proof lifetime T,
  verification budget 2T, batch deadline A); reserve_remote NotApplied accepted on
  protocol grounds (dispatch_settled retries only pre-provider timeout);
  lease-budget split lock_ms/work_ms via test-only locks_held() mark, gate on work
  at unchanged 50% (3× PASS). Runs: changed 23/23, affected 168/168, 196-set
  195/1 (commit_remote pool acquire timeout 503 ms = a, AFTER provider applied —
  not retry-recoverable in the test), stack-budget 2479/2 (claim_abort
  deadline_crossing; definite claim NotApplied episode_lifecycle:178). Static
  PASS. prod-diff.patch 8eec77b7…bed1 (71 files). Author suggests pre-warming
  test pools (min_connections).
- Next: B gen14 code review of the whole lease patch, incl. whether the pool
  acquire bound a=min(A/2,1s) is right for commit_remote after an external effect.
- B gen14 code review (35.5%): ACCEPT WITH FIXES
  (`reviewer-lease-k7q2/CODE-REVIEW.md`). Production (within approved scope):
  F1 lease_claim.rs:109 NotApplied when attempt row absent but claim_on may have
  committed other writes → Applied(None); F2 completion.rs:57 same gap → evidence
  reads our attempt row; F3 commit_remote acquire after external effect uses
  remaining A + one retry of definite NotApplied within T (+ test). Test/CI: F6
  new_claim / claim_boundary claims() — both claimants can be NotApplied → retry
  pair when nothing written (reproduced bounds_128 0 claims); F7 lease-budget.sh
  must also gate action.* and budget.reserve; ci.yml comment says body vs work.
  Low: action_dispatch.rs:269 read-only COMMIT→ROLLBACK; misleading "lease
  transaction timed out" messages at recovery.rs:184, completion_job.rs:62,
  budget.rs:159/168; thin margins test (l) and verifier budget. Root-authorized:
  explicit own-DB pool pre-warm (documented as modelling warm production pool;
  keep test (f) + acquire reporting); claim_abort deadline from gated_policy();
  episode_lifecycle:178 via pre-warm or retry DatabaseTimeout claim.
  Reviewer runs: resolution 18/18; lease_tests 396/397 (F6); 196-set 196/196;
  lease-budget exit 0.
- Next: fresh A gen19 implements F1–F3, F6, F7, lows, pre-warm, test fixes.
- A gen19 (`gen=19`, a3e40aa6…, 26.71%) hit API session limit (429, reset 14:10)
  just before its NEWPG check; resumed 14:21 on user instruction with a
  reconcile-state-first message (edits in tree, no stray processes, NEWPG clean).
- A gen19 (30.4%): F1 (lease_claim.rs:101-105 Applied(None)), F2
  (completion.rs:55-61,:175-206), F3 (persistence_bound.rs:160-185
  acquire_for_attempt; action_receipts.rs:168-250; DESIGN v4 8be2355a…), F4
  ROLLBACK, F5 messages; tests (a2),(o2), F3 test; (g) on 6 s gate policy; F6
  re-poll pair; F7 lease-budget covers 10 required ops; ci.yml sha 9f4bc488…f464;
  own-DB pool pre-warm (max acquire ~500 → 4 ms); claim_abort deadline derived;
  episode_lifecycle re-poll. Unauthorized-by-root extra: registered_invoke
  pre-provider DatabaseTimeout retry (same pattern as dispatch_settled) — sent to
  review. Test (l) unchanged: a = R so two serial commits must fit R. Runs:
  resolution 21/21, affected 107/107, 196-set 196/196 (unknown 0), lease-budget
  0, stack-budget 2484/0; lease_tests subtree 395/5 under load (2 reserve_remote
  unknown, 2 3 s fixture lease expiry, (g) fixed) — each passes alone.
  prod-diff.patch d3e0e133…d179 (75 files).
- B gen14 recheck next.
- B gen14 recheck Gen19 (38.8%): ACCEPT (`CODE-REVIEW.md` "Recheck Gen19").
  **Production lease fix ACCEPTED (scoped)**: prod-diff.patch d3e0e133…d179,
  ci.yml 9f4bc488…f464. Reviewer runs: resolution 21/21; lease_tests +
  supervision 400/400 default parallel; lease-budget 0 (13 ops). Decisions:
  (1) registered_invoke pre-provider DatabaseTimeout retry accepted, test-only;
  (2) test (l): a = R is not a design flaw — make (l) tolerate Unknown under Q2(b)
  asserting the backstop (no fence, one attempt row; cancel retires; expire →
  one Expired, retry_count 1), log branch, require Ok(None) branch ≥1 of 3 alone
  runs; (3) fixture leases append_only:212, receipt_sql:145 → test heartbeats via
  renew_io at quiescent points before 5+3; reserve_remote Unknown → production
  resolution within approved scope (mint entry UUID before timer, read entry under
  same locks; absent → definite NotApplied, present → Unknown never adopted) +
  injection test. Nit: new_claim re-poll when zero claims and ≥1 DatabaseTimeout.
  Watch: budget.reserve work 353/1000 ms.
- Next: A gen19 (reuse) implements (2), (3), nit.

## User direction — 2026-10-10: row 3f dropped (pre-release, DB reset)

User: "nothing deployed yet, we can reset db, why existing upgrades in Row 3f".
Origin found: populated-upgrade requirements were not in the user's brief; they
came from earlier sessions' own contracts (CONTRACT-04.7-AUDIT.md:98,144,
CONTRACT-04.7-BOUNDS.md:144, CONTRACT-04.7-CLAIM-BUDGET.md:4,204,
CONTRACT-04.7-UPGRADE-CATALOG.md), which assumed additive migrations upgrading
populated databases — contrary to the user's standing rule (memory
schema-changes-reset-db: edit init in place, reset DB). Row 3f is removed from
04.7 scope; fresh-schema tests remain. Existing upgrade tests
(action_reconciliation_upgrade_tests.rs etc.) and the 04.x additive migrations
are left as they are unless the user asks to remove/squash them (asked).
- User: "update all tasks in 5 and higher to reset data as well" → added a
  "Schema and data" section (after "Outcome and dependencies") to plan phases
  05–10: pre-release, no data preserved; edit squashed init migration in place;
  drop and recreate dev/test DBs; no additive migrations, backfills, compatibility
  shims or populated-upgrade tests; verify against a freshly migrated schema.
  Phase 10's cutover step (confirm deployment target before resetting) unchanged.

## User decision — 2026-10-10: squash migrations (option b)

After 04.7 completes, as its own step: squash the 04.x (and other post-init)
migrations into `migrations/20260817000000_init_schema.sql`, delete the
upgrade-only tests (action_reconciliation_upgrade_tests.rs and related), drop and
recreate the dev/test databases (and NEWPG's workflow_admission) from the squashed
schema, regenerate .sqlx, and re-run the full suite. Expected fallout to handle in
that step: tests/evidence that pin migration file names, line numbers, SHA256
checksums or the "53 migrations" count (e.g. BRIEF-04.7:478, pinned
pg_get_constraintdef texts — the latter should be unaffected if definitions are
unchanged). Not started during 04.7 so accepted 04.7 evidence stays valid.
- NEWPG 55440 outage 2026-10-10 16:15:41 CEST: postmaster 54203 gone, no
  shutdown/crash line in server log, stale postmaster.pid; 55439 also not
  listening (untouched by us); other "bizi44" test PGs on 55482/55484 running,
  host load ~24. Before outage on final tree: resolution 22/22, lease_tests
  401/401, affected 131/131, 196-set 196/196 (unknown 0), lease-budget 0.
  stack-budget run against dead server (999 connection refused) is INVALID.
  Root authorized restart of 55440 only via pg_ctl per postmaster.opts after
  read-only evidence capture (newpg-outage-20261010/); then identity check,
  stack-budget rerun and one 196-set rerun.
- A gen19 part 2 (37.7%, stopped): items 1–4 implemented (test (l) Q2(b) with
  backstop asserts, lease_resolution_tests.rs:576; fixture heartbeats via renew_io
  receipt_tests.rs:209; reserve_remote entry UUID minted before timer + resolution
  action_dispatch.rs:242/:276/:346-352, injection test
  action_dispatch_resolution_tests.rs:81, DESIGN v5; new_claim re-poll) plus an
  unrequested test-only `invoke` pre-provider retry (provider_tests.rs:124) — to
  review. Pre-outage final tree: 22/22, 401/401, 131/131, 196/196 (unknown 0),
  lease-budget 0. Under external load (~24): 20/2, 398/3, 190/6, each passing
  alone. Static fmt/diff/check/clippy/graft 0; sqlx --check failed (DB down).
- Restart: worker ran pg_ctl start with the sandbox DISABLED; afterwards the
  permission classifier denied all its Bash (identity query, mkpatch). Root did
  NOT run those commands on the worker's behalf; surfaced to user. NEWPG state
  after restart unverified. prod-diff.patch on disk is still d3e0e133 (excludes
  part 2). Outage evidence: lease-prod-k7q2/newpg-outage-20261010/ (macOS log:
  memory-pressure kills of other daemons, nothing for postgres; cause unknown).
- Cause of 16:15 outage per user: the external disk (/Volumes/ssd1) was
  disconnected and reconnected. Root, on user request ("fix/restart database"):
  * NEWPG 55440: running since 16:51 (A gen19 restart, crash recovery clean);
    root did a clean `pg_ctl -m fast restart` at 17:04 with the postmaster.opts
    options; identity 7693072113847127473 / max_connections 200 verified; temp
    write + CHECKPOINT OK; dropped 9 leftover task-created mail_agents_own_*
    databases (0 sessions); now only postgres/template0/template1/
    workflow_admission, 0 clients.
  * Main dev DB 5432 (brew launchd sh.brew.postgresql@18, data
    /Volumes/ssd1/postgresql/data18, system 7685732512755570191): launchd had it
    in penalty box (exit 78, spawned while disk absent). bootout + bootstrap of
    the LaunchAgent → started 17:07, crash recovery clean, ready 17:08:33.
    8 mail_agents_own_* DBs there were NOT created by this task → left alone.
    No mail_agents_test database exists on 5432 (reported to user).
  * 55439 (damaged original) untouched and still not listening.
- Next: fresh A gen20 to finish Gen19 part 2 verification (stack-budget, 196-set
  rerun, sqlx --check, protected, mkpatch) and B review of part 2.
- A gen20 (13%): verification only, no source change. Load 4.6–26 (external
  bizi44 PGs). Resolution 22/22; 196-set 169/27 at load 21.5 (all 27 pass alone at
  load ~5; unknown 2 fail-closed, not_applied 5); lease-budget 0; stack-budget
  2482/3 (batch_recovery:11, completion_race:177 pass alone; prior_bounds:355
  verification Timeout — same failure seen 2026-10-05 pre-change). Static incl.
  sqlx --check PASS; protected only ci.yml 9f4bc488…f464. prod-diff.patch
  ce6d0c34…aa18 (77 files; two part-2 files' base3 snapshots restored from HEAD).
- Acceptance series cannot be meaningful under the current external load; asked
  user to pause bizi44 PG servers. B gen14 rechecks part 2 meanwhile.
- B gen14 recheck part 2 (42.1%): ACCEPT. **Gen19 part 2 ACCEPTED (scoped)**:
  prod-diff.patch ce6d0c34…aa18. "Only aborted commits become definite" for
  reserve_remote is the intended fail-closed design (adopting a stalled-but-applied
  reservation would reverse D2 — a user decision, not needed for 04.7). Test-only
  `invoke` pre-provider retry (provider_tests.rs:~124) accepted. Load analysis:
  no code defect; ~15 of 27 failures = 3 s fixture leases lapsing at load 21,
  3 definite-NotApplied unwraps, 2–3 fail-closed Unknown, 1 conservative
  Replayed. Root authorizes test-only: more fixture heartbeats, re-poll helpers
  for definite NotApplied unwraps, eligibility_remaining:440 accepts Replayed when
  the reservation resolved applied. Acceptance-run rule: a run with external load
  average > 10 is void (recorded, not counted).
- A gen20 (reuse) implements those; focused reruns only.
- A gen20 load fixes (~25%): heartbeats (ledger(), append_only:222, bounds:127,
  sibling_boundary:241, sibling_tests:111, episode_lifecycle:223, completion_race
  window opened inside completion tx), definite-NotApplied re-polls
  (action_dispatch_tests:88, claim_sql:74, claim_recovery:119), eligibility_remaining:440
  Replayed(Granted) with asserted receipt, new_claim Unknown-beside-winner with
  backstop asserts. Reported not tolerated: sibling_boundary:238 reserve_remote
  Unknown breaks exactly-128 premise; consumption_identity:343 reserve_remote
  Unknown once under load (passes alone); class (e) claim_abort usage_deadline,
  completion_race_lock_support:387, claim_sql_candidate:86 unchanged.
  Runs: 28 addressed alone 28/28; 196-set 196/196 (unknown 0, load ≤13.5 from the
  run itself); lease_tests+supervision 400/1; lease-budget 0. Static PASS;
  protected only ci.yml. prod-diff.patch ff9981e3…058e (80 files).
- B gen14 rechecks the load fixes + decides reserve_remote Unknown handling.
- B gen14 recheck load fixes (44.5%): ACCEPT, ff9981e3…058e (11 test files, no
  assertion weakened); reran touched 37/37. **Gen20 load fixes ACCEPTED (scoped).**
  Root authorizes test-only follow-ups: reserve_remote Unknown in
  sibling_boundary:238 / consumption_identity:343 → assert backstop (provider
  calls unchanged; next reserve PossibleDispatchExists if entry exists, else
  nothing written), rebuild scenario on fresh state ≤3 attempts, fail if all void
  (never retry with a new key — corrupts 128/129 counts); claim_abort
  usage_deadline → observer+gate first, A/2 deadline just before claimants,
  `before` after it, void/restart ≤3 with ≥1 proven; completion_race:387
  quiescent → poll with pg_stat_clear_snapshot + 10 s guard;
  claim_sql_candidate:86 → heartbeat before invoke + void/restart on Unknown;
  claim_recovery comment nit (worker.rs:162 doesn't retry complete_io).
- A gen20 (reuse) implements them, then the 5+3 acceptance series.
- B gen14 retired (44.5%; final). Acceptance series (A gen20, series/series.log):
  196-1..5 GREEN (5 consecutive, 147–185 s, unknown 0, voids 0) — **row3d
  combined acceptance runs satisfied** on ff9981e3+final-fixes tree. Broad
  reconciliation: recon-1 GREEN (205/205); recon-2 VOID-LOAD (bizi44 PG 55486 +
  its tests, load 10.6→38); recon-3 counted FAILED — high_bounds_source:89 got a
  fail-closed reserve_remote Unknown where it expected the provider Timeout
  (needs the backstop/rebuild pattern). stack-budget/lease-budget not yet run.
- 20:20 user: "wait for bizi44 if necessary and we'll continue later" → series
  paused; A gen20 told to finish only the bounded recon-3 fix, re-freeze, write
  ACCEPTANCE.md, leave nothing running. Broad series restarts from zero after the
  code change on an unloaded machine.

## Pause checkpoint — 2026-10-10 ~20:35 CEST (user: continue later)

All workers quiescent; no task-owned process; NEWPG 55440 running, identity
verified, 0 clients / 0 own DBs. 55439, 5432, 55486 (bizi44) untouched by workers.
Root 544,574/1,000,000 = 54.46% (threshold 70% per user).

State of the lease work (uncommitted, test + approved production lease fix):
- Frozen: `lease-prod-k7q2/prod-diff.patch` sha256 2216c079…44b9a (82 files).
  Last INDEPENDENTLY ACCEPTED version: ff9981e3…058e. The delta 2216c079 vs
  ff9981e3 = A gen20 "final test fixes" (backstop/rebuild in sibling_boundary +
  consumption_identity, claim_abort void/restart, completion_race quiescence
  poll, claim_sql_candidate heartbeat, comment, plus an unrequested uncaptured
  log line for void attempts / accepted Unknowns) — **NOT YET REVIEWED**.
- Acceptance (ACCEPTANCE.md): row3d 196-set 5 consecutive GREEN on the 2216c079
  tree (counts once Part A is reviewed and unchanged). Broad reconciliation:
  recon-1 GREEN, recon-2 VOID-LOAD, recon-3 counted FAILED
  (high_bounds_source.rs:89 real_history: unasserted fail-closed reserve_remote
  Unknown; diagnosis only, fix not started). lease-budget GREEN (rerun at load
  9.25; first run voided). stack-budget stopped on pause — not counted.

Next action on resume (fresh A/B; only on a host without external load — check
`uptime` and that no bizi44 PG/tests run):
1. A: high_bounds_source:89 backstop + rebuild pattern (≤3 attempts), alone ×3,
   static gates, re-freeze.
2. B: review Part A delta (2216c079 vs ff9981e3) + recon-3 fix.
3. A: broad reconciliation series from zero (3 green; void if load > 10), then
   stack-budget + lease-budget once; if the code changed, re-run the 196-set ×5.
4. Root: full row3d acceptance (combined gate + independent rerun); then row3e
   design/implementation; final 04.7 integration review (incl. out-of-row3d FK/
   CHECK items carried from the S1 map). Row 3f is dropped (user). After 04.7:
   migration squash (user decision) + test-DB template follow-up.

Usage (`--totals`, this root session 8bab464f only; earlier workers under
dcb1b27c are not in this listing — their spend is unknown here, not zero):
root 242 req / input 486 / cache write 2,568,243 / cache read 82,534,008 /
output 197,794; workers listed in the --totals table (claude-opus-5-5).
- Resume 22:55 (load 1.7, no bizi44). A gen20 (35.2%): recon-3 fix in
  high_bounds_source.rs (provider_timeout helper: backstop + void + rebuild ≤3;
  also routed foreign/sibling dispatches of the same scenario). Alone 3/3, module
  1/1, runtime_tests 14/14, 0 voids (void path not exercised). Static PASS;
  protected only ci.yml. prod-diff.patch 28219859…fe84 (83 files).
  **Systemic exposure found:** 106 other reconciliation test sites don't handle a
  fail-closed reserve_remote Unknown (20 expect provider Timeout, 34 `is_err()`
  pass silently, 43 `unwrap()` panic, 9 other) — g20d/exposure.txt. Per-site
  void/rebuild would be a large, test-distorting change → reviewer assessment of
  root-cause options before deciding (possibly a user decision).
- B gen15 (`gen=15`, a5d231a1…, 17.87%): `reviewer-lease-k7q2/UNKNOWN-REVIEW.md`.
  Part 1 ACCEPT → **prod-diff.patch 28219859…fe84 ACCEPTED (scoped)** (10 test
  files vs ff9981e3; no assertion weakened; minor N1 single-read backstop, N2
  re-reserve unwrap can false-fail). Acceptance series must restart from zero on
  this tree (ACCEPTANCE.md named 2216c079).
  Part 2 root cause: reserve_remote Unknowns come from the D2 rule, not short R —
  4 Unknowns in 20,407 calls (~2e-4; ~22% chance per broad run); all 4
  resolutions read the committed immutable entry in 5–163 ms (< R=500 ms); D2
  turns proven Applied into Unknown when COMMIT is in flight as A fires. ~30 of 34
  `is_err()` sites can proceed on a wrong premise on that rare Unknown.
  Root-authorized now (test-only): refusal is_err() sites (prior_bounds:299,
  runtime_conflict:138, …) match exact expected error; assert calls == 1 after
  Pending-provider setups. USER DECISION asked: option (ii) adopt our own
  resolved-present reservation when live_window holds under run→execution→job
  locks (reverses DESIGN v5 D2; competing-claimant test); fallback (i) shared
  void seam. (iii) rerun-as-policy not recommended (P(3 green) ≈ 0.5–0.6).
- A gen20 (38.1%) exact-error/calls fixes: 4 refusal sites now require exact
  Conflict("Workflow lifecycle revision or command conflict"); 19 Pending-provider
  setup sites assert provider calls == 1 (11 already guarded). Touched modules
  211/211 (unknown 0). Static PASS; protected only ci.yml. prod-diff.patch
  7f417970…6a09 (90 files). → B gen15 review. User decision on (ii)/(i) pending.
- B gen15 (21.65%) exact-error/calls review: ACCEPT 7f417970…6a09 (exact Conflict
  traced to shared conflict(), mod.rs:62; calls==1 placed correctly). Remaining
  unguarded setup dispatches: F1 episode_lifecycle:18-24, receipt_sql:99-106;
  F2 runtime_mixed:111-134 (3 sites, missed by inventory). Optional exact message
  at attribution:262, late_truth:241. Rerun 52/52, unknown 0.
  **7f417970 ACCEPTED (scoped)**; F1/F2 queued (test-only, small).

## User decision — 2026-10-10: reserve_remote option (ii)

Adopt our own resolved-present reservation: after a post-COMMIT-timeout
resolution finds our pre-minted entry (immutable, PK, minted per call), the call
returns our Reserved outcome — but only if live_window holds under the
run → execution → job locks; otherwise it stays Unknown. Reverses DESIGN v5's D2
fail-closed rule for this case (→ DESIGN v6). Requires updated/new tests incl. a
competing-claimant test (other reservations still get PossibleDispatchExists;
enter_remote re-checks the fence before the provider). Sequence: design delta →
B gate → implement → B code review → acceptance series from zero (196×5, broad×3,
stack-budget, lease-budget) on an unloaded host.
- A gen20 (39.3%, retiring) F1/F2 calls: calls==1 at episode_lifecycle:21,
  receipt_sql:107, runtime_mixed:120/128/137; exact Conflict at attribution:266,
  late_truth:244 (Ok(Interrupted) alternative kept). Touched 39/39; static PASS.
  prod-diff.patch f6d64179…535c (90 files) — review folded into the (ii) code
  review. bizi44 tests ran again on its PG 55495 (load ~10.7).
- Next: fresh A gen21 writes DESIGN v6 for option (ii) (no code) → B gen15 gate.
- Open user question: collapse acceptance to broad suite ×5 (after confirming
  it contains every row3d-set test) instead of 196×5 + broad×3.
- User: keep acceptance as agreed (196-set ×5 consecutive + broad ×3 + stack-budget + lease-budget).
- A gen21 (`gen=21`, abf92707…, 21.92%): DESIGN v6 (7260cfa7…f4b5; v5 kept).
  Only D2 changes (action_dispatch.rs, persistence_bound.rs): RemoteReservation
  moved into a cell before commit; resolution takes lock_fence (same locks as v5),
  reads our entry by PK + marker/fence columns, live_window → adopted Reserved
  (resolved_adopted) / lapsed → Unknown(cause lapsed) / unresolved → Unknown.
  Tests (q) adopted + post-adoption expiry refusal, (r) competing reservers vs slow
  commit, (s) lapsed window; test-only Injected::outcomes() seam. 99 of 106 exposure
  sites become moot. No migration/.sqlx/dependency. Open Q1–Q4 → B gate.
- B gen15 (24.98%) DESIGN v6 gate: ACCEPT with RC1 (Injected::outcomes() append
  before recorder's early return; mark() clears) and RC2 (test (r) ordering with
  competitor queued on run row before R0, spawn ~A/2 on 6 s policy, pool ≥3).
  Q1 confirm window anchor; Q2 keep "commit outcome unknown"; Q3 seam OK (recorder
  is #[cfg(test)]); Q4 keep VOID loops through the series. Moot split corrected:
  98 moot / 8 unaffected, residual `unresolved` Unknown can still hit the 98.
  **F1/F2 delta f6d64179…535c ACCEPTED (scoped).**
- Next: A gen21 (reuse) implements v6 + RC1/RC2 (+ classify.py resolved_adopted /
  unknown-by-cause summary).
- A gen21 (35.92%) v6 implemented: prod-diff.patch 40bb1e7c…8851 (90 files; delta
  action_dispatch.rs, persistence_bound.rs, action_dispatch_resolution_tests.rs).
  Tests (q)+fence-loss, (r) 4 scenarios with RC2 (+ a "logged load branch"
  accepted after a load-7 failure — reviewer must judge), (s); mutation checks
  fail as expected. Deviation V6-7: expired Reconcile job goes to reconciliation
  (competing claim gets None), recorded in DESIGN "V6 as implemented". Runs: new
  alone 5/5×3, dispatch 27/27, lease_tests 398/398 (real unknown 0). Void runs at
  load 23–35 (bizi44) and one load-12 dispatch run with local test (i) Unknown
  unresolved. Static PASS; protected only ci.yml. → B gen15 code review.
- B gen15 (29.25%) v6 code review: production ACCEPT; tests T1 BLOCKING — all
  five pass on v5 code (unresolved accepted in place of adoption; (r) load branch
  accepts any Unknown cause; (s) accepts lapsed|unresolved). Required: each
  variant reaches its classified branch within ≤3 fresh fixtures or panics; load
  branches narrowed (ours: Unknown only with cause unresolved; lapsed / NotApplied
  when applied must fail in (q)/(r)); branch lines logged; rerun both mutations
  incl. no-adopt in the v5 `unresolved` form. (s)/V6-7 deviation ACCEPTED (design
  sentence: replay-supported actions only). Local test (i) at load 12 = residual
  unresolved, void by load; if recurs ≤10 → gated_policy + backstop + 3-attempt rule.
- A gen21 (reuse) fixes T1.
- A gen21 T1 (39.6%): 10 variants under ≤3-fixture `proven` rule; load branches
  narrowed (ours Unknown only cause unresolved; competitor NotApplied only with own
  not_applied); (s) must show lapsed; competitor at 3A/4. Mutations: v5
  unresolved() form fails 4/5 tests; no-window fails (s); production restored
  byte-identical. Runs (load <10): new 5/5×3 all PROVEN attempt 1, dispatch 27/27,
  lease_tests 398/398. Static PASS. prod-diff.patch cbaec66d…8c85; DESIGN b06fdaed.
  → B gen15 recheck.
- B gen15 (31.94%) T1 recheck: ACCEPT. **Option (ii) v6 implementation + tests
  ACCEPTED (scoped): prod-diff.patch cbaec66d…8c85** (production byte-identical to
  40bb1e7c). Nit: DESIGN V6-7 original test text lines 451/465 superseded by notes
  526-547. Next: acceptance series from zero on cbaec66d (fresh A gen22): 196-set
  ×5 consecutive, broad reconciliation ×3, stack-budget, lease-budget; load >10 void.
- A gen22 (`gen=22`, a8896526…, 11.1%) acceptance series v6 on cbaec66d
  (series-v6/, ACCEPTANCE.md): 196-1..3 GREEN (unknown 0; resolved_adopted seen),
  196-4 counted FAILED 195/196 at load 7.3→11.9 (not void):
  completion_race_actual_receipt_first panicked at
  completion_race_lock_support.rs:71 "exact run probe expected Free, observed Held"
  after both owners completed; holder unidentified; passes alone 3/3. Its 2
  Unknowns were asserted. Series stopped per rules; broad runs and budgets not run.
  Runner fixes: one.sh waits for load ≤10; sample.sh identifies our backends by
  parent pid (bizi44 backends on 55498 also run as mac03).
- Next: test-only diagnosis/fix of the Free probe (A gen22), B review, restart
  series from zero.
- A gen22 (19.1%) Free-probe: facts — return_authorized (action_receipts.rs:307)
  exits `return Ok(false)` after lock_scope (lease.rs:252) took a FOR UPDATE that
  returned no row but kept the row lock (proved on NEWPG), and DROPS the tx;
  sqlx 0.8.6 queues the ROLLBACK to a background task → run row briefly held by
  an idle-in-transaction connection. Timing under load = hypothesis (not
  reproduced in 49 tries). Test-only fix: released_run polls Free every 5 ms with
  holder dump, 10 s guard (verified both ways by temporary edits).
  prod-diff.patch d485d56e…d8ba5. Load-20 uncounted run also failed
  reopen_matching_final_positives (lease.claim timeout in setup) and
  high_bounds_native_owners (real_history:106 "expected provider Timeout: None").
- Root decision: the explicit ROLLBACK on return_authorized's early returns is
  conformance to accepted DESIGN ("read-only ops end in ROLLBACK"), within the
  user-approved lease-fix scope → authorized; audit all lease/dispatch read-only
  early returns for dropped transactions.
- A gen22 (30.5%) explicit rollback: persistence_bound.rs End/WriteBound::end/
  roll_back finish every path; sites fixed in action_receipts.rs, lease.rs,
  action_dispatch.rs, budget.rs, completion.rs (list in IMPLEMENTATION.md "Gen22
  explicit rollback"). New test refused_return_releases_run_before_returning
  failed 3/3 on old code (confirms Free-probe cause). Load-20 failures not this
  cause: reopen_tests.rs:77 setup claim timeout (→ setup retry on timeout);
  high_bounds real_history:106 Ok(_) likely Interrupted from lapsed fixture lease
  (→ renew before call / void + print Ok side). lease_tests 399/399 (after a
  void-by-load run). prod-diff.patch 17092ef1…4f5e.
- Next: A gen22 test-only fixes for those two, then B review of the whole delta
  vs cbaec66d (production rollback + tests), then series from zero.
- A gen22 (33.8%) test follow-ups: reopen setup re-polls DatabaseTimeout only;
  high_bounds real_history prints actual result, renews fixture lease before each
  invoke, voids only a lapsed-window Interrupted (fence names this attempt, window
  passed by DB clock) with backstop; verified by temporary edits. Runs 3/3, 3/3,
  6/6, 1/1. prod-diff.patch d050772d…6b92. → B gen15 review of delta vs cbaec66d.
- B gen15 (36.56%) Gen22 review: ACCEPT. **prod-diff.patch d050772d…6b92
  ACCEPTED (scoped)** — explicit end/roll_back on every bounded path; stalled
  error-path ROLLBACK → NotApplied/unresolved is correct (nothing committed);
  N1 a failed ROLLBACK now returns Err (fail-safe; best-effort optional);
  N2 optional uncaptured holder dump. Reruns 15/15 and 42/42.
- Next: acceptance series v6b from zero on d050772d (A gen22 reuse).
- A gen22 (36.4%) series v6b on d050772d: 196-set criterion MET (196-1,3,4,7,8
  GREEN; 196-2,5,6 VOID at load 21.9/23.8/39.6 from bizi44). Broad recon-1 counted
  FAILED at load 6.3→8.3: claim_recovery_lost_response_automatic_saved_retry:220
  unwrapped a definite DatabaseTimeout (retire_expired not applied before locks;
  another tx in its own DB held run/execution/job lock ~1 s; commit max 991 ms).
  Passes alone 3/3. Series stopped; budgets not run.
- Pattern: each series surfaces another test that unwraps a definite NotApplied
  under shared-SSD commit stalls. Asked user: (B) systematic test-only sweep of
  remaining unwraps of lease/claim/retire/release results (re-poll only definite
  pre-lock NotApplied, keep exactly-once asserts) before the next series, vs
  per-failure fixes; plus running the series when bizi44 is idle.

## User decision — 2026-10-11: option B sweep; authors at opus/xhigh

Systematic test-only sweep of reconciliation-test unwraps of lease/claim/
retire/release results (re-poll only a definite NotApplied before any lock or
effect; keep exactly-once asserts), then review, then one fresh series (ideally
with bizi44 idle). User asked that implementation agents run Opus at xhigh
effort from now on (fresh A gen23, effort xhigh). Reviewers stay opus/high.
- A gen23 (`gen=23`, a9fc5e8a…, opus/xhigh, 38.0%) sweep: SWEEP.md inventory of
  395 bounded-op calls: 314 setup → `.settled()` (one helper in lease_tests.rs,
  re-polls only DatabaseTimeout, 10 s guard, SETTLE log), 15 competing pairs →
  `.competing()` (incl. claim_recovery recover_discarded :220), 37 race sites
  unchanged, 21 already guarded. Proof: 3 deterministic settle_tests + mutation.
  lease_tests 402/402. Static PASS; protected only ci.yml. prod-diff.patch
  21043750…5619 (114 files; production hunks unchanged from d050772d).
  Open: 9 race sites inside the acceptance sets (claim_race compete ×3,
  claim_fairness held_ancestor_claims, claim_cancellation ×4, completion_race
  race); worker process() paths not covered.
- Next: A gen23 handles the 9 in-set race sites, then fresh B gen16 reviews the
  whole sweep.
- A gen23 (43.2%) race sites: claim_race compete ×3 void rule with backstop;
  claim_fairness held_ancestor_claims NotApplied = loser only beside proven
  winner; claim_cancellation ×4 timeouts from policy bounds, stall-voids with
  evidence, proven attempt needs elapsed < attempt_bound; completion_race `race`
  UNCHANGED (its DatabaseTimeout is the defect signal; needs lock-wait evidence to
  void safely — open); worker process() recorded as known gap. Race modules 9/9,
  0 VOID (void paths untested). prod-diff.patch 0d9e50fe…bfb4 (114 files,
  production identical to d050772d). → fresh B gen16 review of the whole sweep.
- B gen16 (`gen=16`, a19d79e2…, 23.96%) SWEEP-REVIEW.md: helper, 314 settled
  sites (all 115 in action_reconciliation files + seeded 70 others), 15 pairs,
  claim_fairness, claim_cancellation SOUND; production identical to d050772d.
  Required (test-only): F1 claim_race compete lost the debit assertion
  (claim_race_tests.rs:161 → restore before :208); F2 void_race (:196-206,
  :214-256) voids on any NotApplied without checking others (non-NotApplied must
  be Ok, debit Granted/injected error); J1 completion_race `race` needs the ≤3
  fresh / ≥1 proven void rule before the series. Minor M1 `competing` wording
  ("no winner", not "nothing written", lease_tests.rs:167,198); M2 classify.py
  should count SETTLE lines. J2 worker process() gap and 28 non-set race sites
  OK to leave. Reruns 3/3 and 11/11.
- Next: A gen23 (reuse) F1/F2/J1/M1/M2 → B gen16 recheck → series.
- A gen23 (retired 45.4%): F1/F2/M1/M2 fixed and proven (F2 scratch injection
  panicked as required). Stale-binary rerun logged (g23/review-race-stale-binary.log).
  Race modules + settle_tests 14/14. prod-diff.patch e85fab1b…ecdc (production
  unchanged). J1 NOT started — notes in IMPLEMENTATION.md "Gen23 review fixes".
- Next: fresh A gen24 (opus/xhigh) J1 → B gen16 recheck F1/F2/J1/M1/M2 → series.
- A gen24 (`gen=24`, a00e5cf3…, opus/xhigh, 30.64%) J1: race returns
  Raced::{Proven, Void(CommitQueue)}; receipt-first voids only with sampled
  evidence (completion waiting on Lock behind receipt in COMMIT, still in COMMIT
  ≤ A/4 before return); completion-first strict; ≤3 attempts ≥1 proven; backstop
  "completion wrote nothing". New file
  action_reconciliation_completion_race_queue_support.rs. Proven by real trigger
  stalls (void ×3 fails; stall in body fails without evidence). Runs 2/2 ×3,
  11/11. lock_support.rs 512 lines (slightly over ~500 guide). prod-diff.patch
  a3526207…0879 (116 files). → B gen16 recheck F1/F2/J1/M1/M2.
- B gen16 (27.12%) recheck: PASS — **prod-diff.patch a3526207…0879 ACCEPTED
  (scoped)**: F1 (claim_race_tests.rs:230-242), F2 (196-229), M1, M2, J1 sound; J1
  void rule cannot hide the ordering defect; size not blocking. Nits: lease_tests.rs
  :170 long comment line; extra blockers allowed beside receipt (proven attempt
  would still fail). Rerun 5/5.

## Handoff checkpoint — 2026-10-11 (root 8bab464f at ~65.5%)

Current accepted tree = HEAD 8ed25b4 + uncommitted work frozen as
`/Volumes/ssd1/dev/workflow-recovery-20261005/lease-prod-k7q2/prod-diff.patch`
sha256 a3526207847dc711c2ae0e6d9477ba335f64aaee1a4ad1a3979590100bbe0879 (116
files; tools/mkpatch.sh in that dir regenerates/compares; production hunks = user-
approved lease fix incl. option (ii) and explicit rollback). Plus the earlier
accepted row3d test additions and plan edits (05–10 "Schema and data", RESUME).

Remaining, in order:
1. Acceptance series v6c from zero on a3526207 (runner lease-prod-k7q2/series-v6b/
   scripts: one.sh waits for load ≤10, sample.sh identifies our backends by
   parent pid, classify.py counts SETTLE): 196-set ×5 consecutive, broad
   `--lib -- reconciliation` ×3, then stack-budget.sh and lease-budget.sh;
   VOID if external load > 10; any unasserted Unknown/other failure = stop and
   diagnose (no mid-series patching). Being run now by A gen24 (a00e5cf3…).
2. On green: root records full row3d acceptance (combined gate + independent
   rerun already satisfied by the series + B reviews).
3. Row 3e (natural COMMIT / full logical / injected rollback) — not started.
4. Final 04.7 integration review incl. out-of-row3d FK/CHECK items from S1 map
   and known gaps (worker process() NotApplied after effect; 28 non-set race sites).
5. After 04.7: migration squash into init (user decision) + test-DB template.
Workers: implementation agents opus/xhigh (user), reviewers opus/high; worker
window 1M (user), retire 50%; root threshold 70% (user). NEWPG 55440 rules
unchanged; never 55439/5432/other projects' clusters (bizi44 runs PG on 554xx).
- A gen24 (36.2%) series v6c on a3526207: STOPPED "too many voids" (13 tries):
  196-1, 196-4, 196-7 GREEN (3 consecutive counted; peak load 12.2–14.4; unknown
  0). 10 VOID-LOAD: 6 clearly external (load 24–89, bizi44 up to 53 procs); 4
  borderline voided by classify.py's own "max load > 16 with foreign processes"
  rule (196-3 max 19.9, 196-6 16.6, 196-8 16.4, 196-13 16.06 all-green). Broad ×3
  and budgets not run. Recurring: bounds_128_prior_consuming_129_total_then_lost_refuses
  failed in 8 of 10 voids in three forms (release_io false receipt_tests:197;
  verification Timeout prior_bounds:363; invoke not Timeout prior_bounds:90);
  passes alone 3/3; never failed in a counted run. J1 completion_race proven on
  attempt 1 in all 13 runs (even at load 88).

## FINAL HANDOFF — root 8bab464f stopped at 66.88% (2026-10-11 ~15:05 CEST)

All workers quiescent; no task-owned process; NEWPG 55440 up, identity verified,
0 clients, 0 own DBs. Nothing committed/staged. Root usage (this session's
transcript): 303 requests, input 603, cache write 5,126,701, cache read
116,783,787, output 256,299 (claude-opus-5-5); worker rows in `--totals`;
earlier root dcb1b27c and its workers not included (unknown, not zero).

Accepted tree: HEAD 8ed25b4 + uncommitted changes frozen as
lease-prod-k7q2/prod-diff.patch sha256 a3526207…0879 (tools/mkpatch.sh).

Next session, in order (fresh A opus/xhigh, fresh B opus/high; worker window 1M,
retire 50%):
1. Reconcile the void rule before running again: the binding rule says a run is
   void if external load > 10; series-v6b/classify.py actually voids at max load
   > 16 with foreign processes and counts greens up to ~14. Decide one rule (a
   green under load is valid evidence; voiding should apply only to failures with
   external-load evidence) and fix classify.py; record it.
2. Fix bounds_128_prior_consuming_129_total_then_lost_refuses load fragility
   (three forms above) test-only via the accepted patterns (settled/competing/
   heartbeat/≤3-attempt void with backstop), B review.
3. Acceptance series from zero (196 ×5 consecutive, broad ×3, stack-budget,
   lease-budget) — ONLY with bizi44 idle (ask the user to pause it first).
4. Record full row3d acceptance; then row 3e; then final 04.7 integration review
   (incl. out-of-row3d FK/CHECK items, worker process() gap, 28 non-set race
   sites, DESIGN V6-7 text nit, lease_tests.rs:170 long comment).
5. After 04.7: squash migrations into init (user decision) + test-DB template.
