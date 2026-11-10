# Review p3-code-r2: P3 (batch 02B) recheck, reviewer B gen 2

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen full diff `tmp/memory-service-run/frozen/p3-code-r2.diff`: sha256 recomputed as `ed92156724dcb40065abcc68d25b105bd2276f4edcf613079eabf6fe02428ba7`, which matches. Tree `08f82f2adf626a5c2933187bf2616a84322d5c0c`. I rebuilt the worktree tree with a temporary index (tmp/ excluded) and it is identical.
- Incremental diff from r1, `p3-r2-incremental-from-r1.diff`: sha256 `c62c89e4…` recomputed, matches. 22 files.
- Scope: each r1 finding, its cross-effects, and the four questions the coordinator raised. Areas r1 accepted and r2 did not touch were not re-reviewed.

## Checks I ran (private cluster 127.0.0.1:55432 only)

| Command | Result | Log |
| --- | --- | --- |
| `scripts/verify.sh` (sqlx-cli 0.8.6, Homebrew psql 18, no incremental/debug) | All gates passed: fmt, whitespace, boundary + probe, fetch, offline check, clippy `-D warnings`, owner migrate, `prepare --check`, manifest, then **77/77** under macOS isolation | `logs/b2-p3r2-verify.log` |
| `cargo test --lib concurrent`, 3 runs | 2/2 each run; the lock-wait assertions are stable | `logs/b2-p3r2-race-repeat.log` |
| Reviewer probes r2 (scratch copy at `/private/tmp/claude-501/-Users-mac03--codex-worktrees-60a0-openbot/6fedfb2e-40aa-486c-bd0b-051b5a3add7b/scratchpad/probe/memory`, synced to r2, probes appended; no project edits) | 4 tests, 6 probes; results below | `logs/b2-p3r2-probes.log` |

## Recheck of r1 findings

| r1 | Status | Evidence |
| --- | --- | --- |
| F1 fetch provenance | **Fixed** | `memory.rs` link query: a source must be in a named namespace, at or below the ceiling and unexpired. A parent must be in a named, active namespace, at or below its *current* ceiling and unexpired. Anything else is silently dropped. Probe 1: a reader that names both namespaces sees the parent link. The narrow reader (b only, ceiling 6, parent raised to 8) sees `sources: []`. A new test, `memory_storage_fetch_omits_provenance_the_reader_cannot_see`, covers this. |
| F2 deadline/retention | **Fixed (option b)** | `expires_at` is back in the canonical request. Probe 2: same deadline → `replayed: true`; a later deadline → 409. The D5 sweeper argument is sound: an attempt that can still execute carries the stored deadline, so the record exists until that deadline and later attempts get 410. Cross-check against the originals: 01A.6 holds, since the deadline is part of the canonical request. Phase 05 §6 ("generation-intent delivery keeps its original logical identity/expiry") and D11 ("delivery intents reuse one idempotency key and must finish within the request's `expires_at`") already require an intent to persist and resend its deadline. Residual doc gap: n1. |
| F3 operation grants in citation | **Fixed** | `readable` now keeps only grants that carry `fetch` (`application/memory.rs`). Probe 3: without `fetch` on `a`, both revision 1 and revision 2 give `not_found`. The revision oracle is gone. Cited parent namespaces are locked `FOR SHARE` in id order and re-checked as active. New test: `memory_authority_citation_needs_read_authority_and_deletion_write_authority`. |
| F4 delete level | **Fixed (root decision)** | `lock_target` refuses `found > write_max` with 403 after the 404 visibility check, for both delete and correct. Probe 4: write_max 0 deleting a level-9 record → 403. |
| m1 carried label | **Fixed** | Added `carried_level` with `CHECK access_level = GREATEST(watermark, source_max, carried)` and `carried > GREATEST(...)`. It is set only when the prior level exceeds the correction's own evidence. |
| m2 cross-namespace sources | **Fixed** | Sources resolve only in the target namespace (`namespace_id = $2`), so source links never cross namespaces and their FK cannot block another namespace's purge. Parent links cascade. Same-namespace-only sources break no original requirement: 01A.4 and 01B.3 need immutable source revisions and their levels; cross-namespace evidence can still be attested through `external_sources` with its level; 04 records window sources in the target namespace and forbids importing other scopes' evidence. One deliberate narrowing: a credential without `fetch` on its own target namespace cannot cite the sources it recorded. That is conservative and acceptable. |
| m3 re-record retention | **Fixed, with a residual defect** | `expires_at` is now compared, but see n2. |
| m4 revision bound | **Fixed** | 1–128 bytes, both in the schema and in `context()`. |
| m5 tests | **Fixed** | The race tests run on `multi_thread` and assert a lock wait in `pg_stat_activity` from a separate connection, while the `hold-` key keeps the winner uncommitted. They are stable across 3 runs. HTTP/MCP parity for correction and copy denials is covered by `memory_mcp_correction_and_copy_denials_match_http`. Cross-tenant FK cases now include sessions, parent links and parts. |
| m6 lock order | **Fixed** | Delete locks `FOR UPDATE` directly. The actual order is written in the P3 notes, with the shared-only argument for namespaces locked after the target. I checked that argument against admin namespace delete (exclusive on one namespace, needs no row a writer holds) and the planned purge; I found no cycle. |
| m7 record text | **Fixed** | D4/D5 now describe the implemented behaviour. |
| m8 replay by another credential | **Fixed, with a residual oracle** | See n3. |
| nits | **Fixed** | A correction citing itself → 400. The `status` column was removed. |

The `#[cfg(test)]` hold hook does not leak. Both the definition (`memory.rs:259`) and the two call sites (`:195`, `:867`) are under `#[cfg(test)]`, which is set only for the lib's own unit-test target. The `serve` binary, release build and image link the lib without it, and clippy `--all-targets` passes. Only keys starting with `hold-` are affected, and only in test builds.

## New findings (all minor)

- **n1, `services/memory/src/application/contract.rs:85` (and `:220`, `:243`, and the other write requests' `expires_at` docs, mirrored in `openapi.json`).** The published description says only "At most 7 days ahead; a new execution after it is refused." Since F2, the deadline is part of the request identity, and a retry with a re-minted deadline is a 409. An independent client (01A) cannot learn that from the schema. Required: state in each write request's `expires_at` description that a retry must resend the original value unchanged, and that a changed value is `idempotency_conflict`.
- **n2, `services/memory/src/adapters/postgres/memory.rs` re-record comparison (`existing.occurred_at != source.occurred_at || existing.expires_at != source.expires_at`).** PostgreSQL stores microseconds; the request keeps chrono's nanoseconds. Probe 6: an identical re-record with `occurred_at …00.123456789Z` and `expires_at …00.000000001Z` under a second key → 409 `conflict` instead of the same source id. `occurred_at` already had this in r1 (I missed it), and the m3 fix extends it to `expires_at`. Required: normalize request timestamps to microseconds at validation, which also normalizes the canonical hash, or compare in SQL against the bound parameters. Add a sub-microsecond case to the source test.
- **n3, `services/memory/src/adapters/postgres/memory.rs` `replay_authority`.** Probe 5: credential B (namespace b only) replays credential A's copy request, whose parent P is in namespace a.
  - A fresh citation of P by B → 404.
  - A replay while P exists → 403.
  - A replay after P is deleted → success (`replayed: true`).
  So a replay tells B whether P still exists, which a fresh request never does. Two things limit the impact: B must already know the key and the exact canonical request, and P's id is in that request. Required: answer `not_found` for a replay by another credential when any cited parent is missing or unreadable now, matching a fresh execution; keep 403 for a level above `write_max`. Alternatively, document the residual. Also note that the check skips cited `Source` refs. They are same-namespace now and sit at or below the result level, which is already checked against `write_max`, so this is acceptable as is.

## Verdict

**Pass for P3 (02B), conditional on n1–n3 as minor follow-ups.** F1–F4 and m1–m8 are fixed, and each fix was confirmed by a probe or a new test. verify.sh passed 77/77, including `prepare --check`, and the race tests are stable over 3 runs. No blocker or major remains.

n1–n3 are small. They can be fixed in this point with a focused recheck (`memory_storage_`, `memory_idempotency_`, `memory_contract_` for the regenerated openapi), or carried as tracked items into 03A. Root decides.

## Remaining required checks

1. If n1–n3 are fixed now: focused `memory_storage_`, `memory_idempotency_`, `memory_authority_` and `memory_contract_` (openapi freshness), then `verify.sh`.
2. Carried over from P2, CI only: the first green `memory-service.yml` run (verify job with Linux isolation, extracted tracked-only copy on a fresh target) and the image job (non-root, `--network none --read-only`).
3. Phase 03 must reuse the F1 provenance filter for timeline, search and context, and re-prove that deleted content is unreachable through those endpoints (02B.3).
