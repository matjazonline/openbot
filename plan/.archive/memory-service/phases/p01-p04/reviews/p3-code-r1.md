# Review p3-code-r1: P3 = batch 02B (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`. I replace gen 1. I read the originals myself: README, 01 (01A.6, 01B, the access-level contract), 02 (02B), the decision record (D2–D6, D13, P2/P3 notes) and AGENTS.md.
- Frozen full diff: `tmp/memory-service-run/frozen/p3-code-r1.diff`, sha256 `8a48100b…ee8b8a`. I recomputed it and it matches. Base `a6bed48a`, tree `844c2d774d3a199f88c4f710683ec62b19408500`.
- Incremental diff from P2: `p3-incremental-from-p2.diff`, sha256 `7c794334…b2cb0`. Recomputed, matches; 42 files.
- I rebuilt the worktree tree with a temporary index (tmp/ excluded) at review time. It is `844c2d77…`, identical to the frozen artifact.
- Database: root's private cluster `postgres://postgres@127.0.0.1:55432/postgres` (PostgreSQL 18.6, pgvector 0.8.7). I did not touch localhost:5432 or any `.env`.

## Checks I ran

| Command | Result | Log |
| --- | --- | --- |
| `scripts/verify.sh` (sqlx-cli 0.8.6 from `tmp/memory-service-run/tools/bin`, Homebrew psql 18, `CARGO_INCREMENTAL=0`, no debug info) | **All gates passed.** fmt, whitespace, boundary with probe, fetch, offline check, clippy `-D warnings`, owner-role migrate, `prepare --check`, model manifest, test build, then **72/72** under macOS isolation, including all 13 `storage_tests` | `logs/b2-p3-verify.log` |
| Reviewer probes: 4 tests appended to a **scratch copy** of `services/memory` under the session scratchpad, sharing `services/memory/target`, run with `cargo test --lib probe_` against the private cluster. No project file was edited. | 4/4 ran; each one confirmed a defect below | `logs/b2-p3-probes.log` |

Not reproducible here, carried over from P2: the CI verify job (Linux isolation), the image job and the extracted build on a fresh target.

## Coverage of 02B and its focused checks

| 02B item / focused check | Status |
| --- | --- |
| 1. Tenant-consistent composite FKs, constrained columns, levels 0–9, parts keep the parent label | Done in the schema. The new `parent_revision` CHECK and the partial parent index are correct. The cross-tenant test covers observation→namespace, source→namespace and link→source. It does not cover session, parent-link or part borrowing (m5). |
| 2. Use cases; record, rows and links commit together; concurrent identical saves; frozen inputs; canonical request holds the label inputs | Done. There is a retention hole in the claim protocol (F2) and a cross-credential replay nit (m8). |
| 3. Correction with expected revisions, no declassification, parts invalidated; delete removes vectors and links | Done. Stale context references are deferred to 03B through the revision bump, which is acceptable. Provenance is overwritten on a carried level (m1). Delete authority question (F4). Timeline and context do not exist yet, so "not retrievable through timeline/context" must be re-proved in 03. |
| 4. FTS and pgvector, filter and claim indexes, embedding width | Unchanged from P2 and accepted. |
| 5. Isolated DB tests; metadata regenerated | Done. Each fixture gets its own database, and `prepare --check` passed (61 entries for 62 macro sites, one duplicated statement). |
| Fresh migrations, cross-tenant FK, concurrent identical saves, conflicting keys/levels, concurrent correction, copies, exact/fallback persistence, source-label tampering, delete/write races, vector cleanup | All present and passing. Two weaknesses: the races do not prove overlap (m5), and the HTTP/MCP parity required by 01 covers only save and fetch (m5). |

What I checked and found correct:
- The D13 parent locks and in-transaction re-checks: tenant, namespace and credential `FOR SHARE`, then state checks. They run before the claim in every write, and the test uses a stale principal, so the re-check really is the one exercised.
- The claim is `INSERT … ON CONFLICT DO NOTHING`, followed by a READ COMMITTED re-read. The loser waits on the uncommitted key, and a rollback lets it proceed. Only a new execution checks the deadline, and a 410 rolls the record back.
- The label is a pure function of hashed fields. A relabel under the same key gives 409. Correction stores `max(existing, computed)` and refuses a result above `write_max`.
- Fetch filters tenant, namespace, ceiling, namespace state and expiry in SQL before anything is returned. The `unavailable` list is uniform. `exposed_level` counts only returned items. Fetch runs in one REPEATABLE READ, READ ONLY snapshot.
- Delete removes parts, its own links and child parent-links by cascade.
- HTTP and MCP share `memory::execute`.
- `with_retry` retries 40P01/40001 three times with jitter, then answers 503.

## Findings

### F1 major: fetch returns link metadata without the D4 filter
`src/adapters/postgres/memory.rs:937-970`. The link query selects every link of each visible observation and only joins `sources` to get the revision. Parent observations and sources are never filtered by namespace grant, current level, ceiling or expiry. D4 says every read path filters "including … link metadata", and the access-level contract says "Return only records with `access_level <= read_ceiling`, including … provenance".

Probe 1 (`probe_fetch_link_metadata_is_unfiltered`):
1. A credential granted only namespace `b` at ceiling 6 fetches a copy in `b`.
2. The response includes `{"kind":"observation","observation_id":<P>,"revision":1}`.
3. P lives in the ungranted namespace `a` and was later corrected to level 8.

So P's existence and id leak across both the namespace and the level boundary. The same applies to source links into other namespaces, including their client-chosen revision strings, and to expired parents or sources.

Required: a link is returned only if its target is in `read.namespace_ids`, has its current level ≤ `read_ceiling`, and is not expired. For a parent, check its current `access_level`; for a source, its level and namespace. Omit any other link without a count or marker. Add a test with a cross-namespace parent, a parent raised above the ceiling and an expired source. Phase 03 timeline, context and search must reuse the same filter.

### F2 major: a replay with a later deadline does not extend the record's retention
`src/adapters/postgres/memory.rs:148-188`, `src/application/memory.rs:115-126`. `expires_at` is excluded from the canonical hash, so "a retry may change only its deadline". But a replay never raises `request_expires_at`. D5 (and 01A.6, "a request retry cannot silently create another observation") relies on retention lasting until every accepted attempt's deadline has passed, so no accepted key executes twice.

Probe 2: the first request had a deadline of now+1h. A replay with a deadline of now+6d returned `replayed: true`, but the record still says `request_expires_at = now+1h`.

The failure sequence:
1. The record is retained 7 days.
2. A retry minted at day 6 carries a deadline of day 13. It is valid, being at most 7 days ahead of its own send time.
3. Delivered again at day 8, after the 06A sweeper removed the record, it executes a second time.

Clients that re-mint `expires_at` per attempt are exactly the case the exclusion was made for.

Required, either:
- (a) on replay, `UPDATE idempotency_records SET request_expires_at = greatest(request_expires_at, $new)` in the same transaction; or
- (b) put `expires_at` back into the canonical request, so a changed deadline conflicts.

In either case, record the choice in D5 and add a test that asserts the stored deadline. Option (a) keeps the declared deviation sound.

### F3 major: source resolution ignores operation grants
`src/application/memory.rs:179-184` (`readable`) and `src/adapters/postgres/memory.rs:266-271`. Every grant with a `read_max` counts as "readable" for service sources, whatever its operations are. `validate_grants` (`service.rs:401`) allows `read_max` without any read operation.

Probe 3: a credential with `a: {save}, read_max 9` and `b: {save, fetch}`:
- gets 403 on fetching from `a`;
- learns that parent P in `a` exists through a save in `b`;
- learns P's current revision: revision 2 gives `revision_conflict`, revision 1 succeeds;
- learns P's level: the save returns `access_level: 5`, and a ceiling of 3 gives `source_above_ceiling`;
- plants a parent link into `b` as well.

01B.2 binds credentials to allowed operations and requires "the same boundary on … selected IDs". A cited id is a selected id.

Required, either:
- count a namespace as readable for citation only when its grant holds a read operation (at least `fetch`); or
- have grant issue refuse `read_max` without a read operation.

Add an authority test for this case.

### F4 minor (decision needed): delete is not bounded by `write_max`
`src/adapters/postgres/memory.rs:825-858` and `src/domain/grant.rs:176`. Probe 4: a credential with `read_max 9, write_max 0, {delete}` deleted a level-9 record. Correction refuses a result above `write_max`, but delete needs only some `write_max`. 01B.2 binds credentials to "permitted read/write access levels". Deletion is a write at the record's level.

Required: either refuse a delete when the record's level is above `write_max` (403, after the 404 visibility check), or record in D3/D4 why deletion is bounded by the read ceiling alone, and test it.

### Minor

- **m1 Correction overwrites the label's evidence.** `memory.rs:777-793`. When `max(existing, computed)` keeps the old level, the row stores the new `producing_ceiling`, `watermark_*` and `source_max_level`, for example level 6 with "exact 2, sources 0". 02B.2 and 01B.3 want the label kept with its provenance. Required: persist that the level was carried from the prior revision, for example a `carried_level` column or keeping the maximum of each evidence column, or document why the CHECKs are enough.
- **m2 Cross-namespace service sources are undecided.** First, `readable` namespaces other than the target are neither locked nor re-checked in the transaction, so a namespace deleted after authentication can still be cited (`memory.rs:261-371`). Second, a source link's FK (`0001_init.sql:271`) has no `ON DELETE`, so B's links will block the 06A sweeper and purge from deleting namespace A's sources, and the source rows would otherwise survive as metadata. Required: either restrict service *sources* to the target namespace (copies of parent observations already cascade), or lock and check each cited namespace `FOR SHARE` in id order and define the sweeper's handling now.
- **m3 Re-recording a source ignores `expires_at`.** `memory.rs:533-541`. A re-record with a different retention silently returns the existing row. Required: include `expires_at` in the identical-revision comparison, or document it.
- **m4 `SourceRef::Source.revision` is unbounded.** `contract.rs:30`, and `memory.rs:128-146` does not check it. Required: apply `MAX_REVISION_BYTES` in the schema and in `context()` (01A.5).
- **m5 Test strength.**
  - `#[tokio::test]` uses a current-thread runtime, and the identical-save and correction races do not show that the loser actually waited on the winner's uncommitted row. Hold the winner open with an advisory lock or barrier, or assert a lock wait. This matters under AGENTS.md's competing-claimant rule.
  - HTTP/MCP parity covers only save and fetch. 01 requires the same denials for correction and copy over both transports.
  - The cross-tenant FK test lacks session, parent-link and part cases.
- **m6 Lock-order notes.** Delete takes `FOR NO KEY UPDATE` and then upgrades to `FOR UPDATE` when it deletes (`memory.rs:467`, `:834`). It should lock `FOR UPDATE` directly, before 03A part inserts start taking `FOR KEY SHARE`. Correction takes source locks before it deletes parts and links. D13 should state the actual order for correct and delete.
- **m7 Decision record text.** D5 still says a replay "reports current state". The record rule is that the phase file wins and the record is corrected, so replace the wording rather than marking it "deferred". Do the same for D4's "422 `missing_trusted_ceiling`".
- **m8 Replay skips current-credential checks.** The scope is `(tenant, kind, key)` with no credential. A different credential replays the stored result, including the level, even when its `write_max` or readable namespaces could not have executed the request. The impact is low, because it needs the key and the exact request. Required: on replay, refuse with 403 when the stored level is above the current `write_max`, or document the choice.
- **nit** A correction can cite its own target as a parent, which creates a self-link. Refuse it. The `idempotency_records.status` column is always 200 and is unused.

## Assumptions challenged (declared deviations)

| Deviation | Verdict |
| --- | --- |
| Source readability via namespace `read_max` instead of `min(read_ceiling, producing_ceiling)` | The reasoning is sound: otherwise `source_above_ceiling` is unreachable. It is acceptable only once F3 is fixed. |
| A parent at another revision gives `revision_conflict` | Acceptable once F3 is fixed: it then reveals revisions only to real readers. |
| 422 `classification_refused` with `details.reason` | Acceptable. Fix the D4 text (m7). |
| Canonical request excludes key and `expires_at` | Acceptable only with F2 fixed. |
| Replay returns the result as at execution | Acceptable: it matches 01A.6 ("return the original result"). Correct D5 (m7). |
| Sessions idempotent by external id; closed sessions accept writes | Acceptable for 02B. Phase 04 must decide whether close fences summaries. |
| `content_sha256` = SHA-256(title NUL body); NUL refused | Acceptable. |
| Sources stored unredacted until 04A | Acceptable as an open item. The README warns about it. |
| `context.validate`/purge later | Acceptable. The README status omits validate (nit). |

Architecture challenge: cross-namespace citation (F1, F3, m2) is the root of three findings. Unless a product need for cross-namespace *sources* is recorded, the simplest sound design is:
- sources only in the target namespace;
- parent observations across namespaces only for copies, with the operation check from F3;
- fetch provenance filtered as in F1.

Speculative scope: none significant. `content_sha256` and `index_state` in fetch are small, and both are used by phase 03.

## Remaining required checks

1. Re-review the F1–F4 fixes, with the new tests run against PostgreSQL: rerun focused `memory_storage_`, `memory_idempotency_`, `memory_access_level_`, `memory_authority_`, `memory_mcp_` and `memory_contract_`, then `verify.sh`.
2. Carried over from P2, CI only: the first green run of `memory-service.yml` (verify job with Linux isolation, extracted copy) and the image job (non-root, `--network none --read-only`).

## Verdict

**Not passed.** There are three majors (F1 fetch provenance leak, F2 idempotency retention hole, F3 operation-grant bypass in source resolution) and a decision-needed minor (F4). All four were confirmed by probes. The rest of 02B is implemented correctly, and verify passed 72/72 in my run.
