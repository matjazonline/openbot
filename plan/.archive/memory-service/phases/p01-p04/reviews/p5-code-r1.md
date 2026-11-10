# Review p5-code-r1: P5 = batch 03B (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen full diff `tmp/memory-service-run/frozen/p5-code-r1.diff`: sha256 recomputed as `0d69202a4805b2f59b58b29c14ece5b3453d1c0a1211cf4dca66eeb86d51b5b1`, which matches.
- Incremental diff from P4 (`6402e15c`), `p5-incremental-from-p4.diff`: sha256 recomputed as `83724147f554bc677d805f142439e31f9dcca609bb6381a0d2b399353a77f68f`, which matches. It covers 29 files.
- Worktree tree rebuilt with a temporary index: `0e841af1546bf7d1cdb09fe72f7d866d3a86eff5`, identical to the frozen tree.
- Originals read:
  - 03B in full;
  - D4 (provenance filter), D6, D8 (rewritten), D10 and the P5 implementation notes;
  - the audit statements in 01–06.

## Checks I ran

- `scripts/verify.sh` on the private cluster, with the shared target only.
- Result: every gate passed, and **120/120** tests passed under macOS isolation. That includes `memory_recall_independent_python_client_…`, which runs over real TCP, and the real-model acceptance check.
- Log: `logs/b2-p5-verify.log`.

## Coverage of 03B

| 03B item / focused check | Status |
| --- | --- |
| 1. Compact index; bounded authorized timeline; selected fetch repeats authority | **Met.** Search items carry id, title, a snippet of at most 320 bytes, timestamps, level and visible sources, and no body. The timeline anchor must pass the D4 predicate, or the answer is a uniform 404. Each side's WHERE carries the full predicate ahead of its `LIMIT` (≤ 20, default 5). Ordering uses the `(occurred_at, id)` tuple, so ties are deterministic. `session_only` keeps the anchor's session; session ids are tenant-unique, so this stays inside the namespace. There are no counts or gap markers. The timeline runs in one REPEATABLE READ snapshot. Fetch and timeline re-authorize every request independently of earlier searches (cross-scope id injection test). |
| 2. Equivalent HTTP/MCP; same grants, bounds, audit; write tools need write authority | **Met.** Everything goes through one registry and one `execute`. The MCP test compares bodies for full and read-only credentials, checks eight identical denials, and checks that `memory_save` is refused for a read-only credential on both transports. "Reads not audited" is consistent: neither transport audits reads, and both audit writes identically, as 03B.2 asks. Nothing in 01–06 requires read audit records. 02B's audit duty is "security/state changes". |
| 3. Token *and* byte budgets; identity, provenance, classification with text; deterministic order, selection and omission; exposed level of everything returned | **Met.** Selection is greedy whole-item, skips and continues, and is pure (`domain/packing.rs`). Boundaries are inclusive and tested. The byte budget is exact over the items' compact serializations. Omission is a count of visible skipped candidates only. `exposed_level` is the maximum of included items and their *visible* provenance (`RecallRow::exposed_level`); it is never the ceiling or a skipped candidate. Snippets are each item's own text. |
| 4. Structured data for any host; no prompt, roles or instructions | **Met.** Packs are JSON data. The test covering stored instructions checks that quotes and control characters round-trip as escaped data. |
| 5. Revision/expiry evidence to freeze and check a pack | **Met, with a semantic gap** (m1). `context.validate` reports per item: `valid`, `changed` (revision), `expired` or `unavailable`. `unavailable` covers deleted, foreign, unnamed, above-ceiling and raised-above-ceiling items. A revoked credential gets 401 and a narrowed grant 403. No content is returned. |
| Focused list: progressive disclosure, cross-scope id injection, restricted timeline neighbours, exact budget boundaries, escaping, snippet/metadata classification, filtered candidate counts, deterministic packing, equivalent transport denials, second independent client | **All present and passing.** |

### The specific questions

- **Does the token rule bound prompts?** It is honest about what it bounds. The token count is 64 plus 2 × UTF-8 bytes of the items' compact JSON. Byte-level BPE tokenizers (GPT/Claude families) and SentencePiece with byte fallback emit at most about one token per byte, so 2× is a safe, provider-independent ceiling *for the service's serialization*. The contract (`contract.rs:311-316`) says exactly that: items' compact JSON. Two caveats: it does not cover the pack envelope fields, nor a client re-encoding that inflates bytes (m3). 03B.3 ("row limits alone do not bound prompts") is satisfied.
- **Do the omission, truncation or semantic fields leak?** No.
  - `omitted.budget` counts only candidates that passed the predicate.
  - `candidates_truncated` is derived from filtered windows: legs whose WHERE already holds the predicate, the visible fused list, or the 201st *visible* recent row.
  - `semantic: partial` depends only on the eligible-part count (P4).
- **Does the Python client use only the published contract?** Yes. Paths and request fields come from `openapi.json` (`operationId`), tools come from `tools/list`, the transport is real TCP, and it uses the standard library only. It reads its own grants from `credential.get`. Provisioning is done by the harness through the admin API, which is outside the client contract and appropriate.
- **Deviations:**
  - `ts_headline` on ≤ 200 rows for compact query packs is acceptable: it is bounded by the candidate cap and the 2 s timeout.
  - The `pack_digest` is compare-only, which is acceptable.
  - Placing the DB tests in `adapters/postgres` follows the D1 layering rule.
  - Four operations returning 501 (generation ×3, purge) is correct for their phases.
  - `valid_until` derived from own expiries is covered under m1.

## Findings (all minor; no blocker or major)

- **m1, provenance outside validate and `valid_until`.** Locations: `services/memory/src/adapters/postgres/recall.rs:246-302` (`validate_once`) and `domain/packing.rs:74-77`. 03B.3 puts provenance in the pack, and 03B.5 asks whether a frozen pack is "still authorized and valid". `validate` checks only each item's revision, expiry and visibility, and `valid_until` uses only the items' own expiries. So a frozen pack whose source link has since expired, or whose parent was raised above the ceiling or moved to an inactive namespace, is reported `valid`. It is also replayed with that provenance, which includes the source's client-chosen revision string, past the source's retention expiry. Impact is limited: these are ids and revision strings that were authorized when packed, and 03B.5 leaves retained snapshots to clients. Required, either:
  - report `changed` when an item's visible provenance set differs from what was packed, and include provenance-target expiries in `valid_until`; or
  - state in D8 and in the `context.validate`/`valid_until` descriptions that validation covers item text, revision, expiry and visibility only, not provenance, and that a pack being reused should be rebuilt to refresh its provenance.
- **m2, full-detail packs load every candidate body before budgeting.** Location: `services/memory/src/adapters/postgres/recall.rs:56` and the `body` column in `search.rs`'s recall query. A `detail: full` pack can load up to 200 × 64 KiB (12.8 MB) per request to fill a budget of at most 256 KiB. With 64 requests in flight that is about 820 MB. Required for 06A's load work, or now if cheap: select first on sizes (`octet_length` plus escaping slack), or load bodies in rank-order chunks and stop once no remaining candidate can fit.
- **m3, byte and token accounting caveats.** Locations: D8 and `contract.rs:311-316`.
  - `byte_count` is the sum of the item serializations. The `items` array itself adds n + 1 bytes (brackets and commas), which a client checking the array against `max_bytes` would see.
  - The token rule holds for the service's encoding. A client that re-encodes the items with non-ASCII escaped (for example Python's default `ensure_ascii=True` turns a 4-byte emoji into a 12-byte escape) can exceed it.
  - Required: one sentence in D8 and the field docs saying the bound applies to the items as served, in compact JSON, excluding envelope fields.

## Speculative scope

None. `content_sha256` in packs, `checked_at` and `generated_at` are small and serve 03B.5 and 05.

## Remaining required checks

1. Optional recheck of m1–m3 after they are fixed or documented. Focused `memory_context_` and `memory_contract_` cover openapi freshness.
2. CI only, carried over: the first green `memory-service.yml` run (Linux isolation, model cache and download, extracted copy) and the image job (non-root, `--network none --read-only` `model-check` within 2,560 MiB; the first Linux run is calibration).

## Verdict

**Pass for P5 (03B)**, with three minor follow-ups (m1–m3). Every 03B item and focused check is implemented and passing: verify.sh 120/120 in my run, including the independent Python client over TCP. The access-level properties hold: predicates come before limits, there are no restricted counts, `exposed_level` covers snippets and visible provenance, and transport denials are identical. Fixing m1 now (code or documentation) is the most useful of the three, because phase 05 will freeze packs.
