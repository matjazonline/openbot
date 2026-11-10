# 03 Retrieval and context

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-rust-application-and-storage.md). An independent HTTP or MCP client can search
authorized memories, inspect nearby context, fetch selected details, and request a bounded context
pack. Observation generation is added in [04](04-observation-generation.md).

## Batch 03A Implement keyword and semantic retrieval

1. Use PostgreSQL's native full-text search for keywords and pgvector for meaning. Combine bounded
   ranked candidates with reciprocal rank fusion, keeping exact identifiers and paraphrases useful.
   A separate vector server or an additional BM25 extension is not required for the first release.
2. Use a pinned local embedding model and tokenizer with explicit model identity, vector width,
   normalization and document/query conventions. Adapt the offline artifact-validation pattern in
   [OpenBot's embedding adapter](../../src/adapters/embedding/mod.rs) without importing its private
   code or requiring its runtime/configuration. Package and validate assets independently.
3. Split long observations into bounded meaningful parts so the model does not silently truncate
   the information being indexed. Rank parts, deduplicate observations, and retain provenance.
   Never compare vectors from different models; incompatible model/schema changes reset the
   service database under this repository's policy.
4. Apply tenant, namespace, classification, deletion and expiry predicates in both retrieval legs
   before candidate limits. A ceiling of 6 reads levels 0–6 within authorized namespaces and cannot
   expose levels 7–9 through snippets, source metadata, counts or timeline expansion. Handle
   approximate-index filtering without silently returning no
   results merely because another tenant filled the candidate window. Bound all expansion work.
5. Keep stored-text success distinct from indexing failure. An explicit save can be durably
   accepted with a visible pending/failed indexing state; keyword recall remains possible.
   Retrying indexing uses only already-saved authorized content, never reruns observation
   generation. Do not silently report successful semantic indexing when it failed.

Focused checks: `memory_search_` uses a fixed corpus with exact IDs, paraphrases, long notes,
duplicate parts, non-English text and empty matches. Include foreign-tenant/namespace/classification
rows nearest to the query, deleted records, invalid vectors, failed indexing and model mismatch.
Assert ceiling-6 recall includes eligible level-0/4/6 records and is unchanged by level-7/9 records.
Run a small real-model retrieval acceptance check in addition to deterministic unit fixtures.

## Batch 03B Implement progressive recall and context packing

1. Search returns a compact index with IDs, bounded titles/snippets, timestamps, classification and
   source references. Timeline returns a bounded authorized neighborhood. Selected-detail fetch
   accepts a bounded batch of IDs and repeats all authority checks, regardless of prior search.
2. Expose equivalent HTTP operations and MCP tools for search, timeline and selected details.
   Both transports call the same use cases, with the same current grants, body/result bounds and
   audit behavior. Write tools, if exposed, require separate write authority.
3. Context packing selects useful observation/summary items and enforces both token and byte
   budgets. Include identity, provenance and classification alongside text. Row limits alone do
   not bound prompts. Define deterministic ordering, selection and omission semantics.
   Return the highest level of all text/metadata exposed in the pack, including compact snippets.
   Do not taint it with candidates discarded before exposure or substitute the requested ceiling
   for the actual returned level. The client combines this with its prior consumed watermark.
4. Return structured data suitable for an arbitrary agent host. Keep system-prompt construction,
   provider-specific message roles, workflow checkpoints and recipient selection out of this
   service. Stored instructions remain ordinary untrusted memory content.
5. Define revision/expiry evidence so a client can freeze a context pack and later check whether
   it is still authorized and valid. Do not promise that deleting a service row can retract text
   a client has already sent to a model; clients own their retained snapshots and model requests.

Focused checks: `memory_recall_`, `memory_context_` and `memory_mcp_` cover progressive disclosure,
cross-scope ID injection, restricted timeline neighbors, exact budget boundaries, escaping,
snippet/metadata classification, filtered candidate counts, deterministic packing and equivalent
transport denials. A second non-OpenBot test client must
complete save, search, selected fetch and context assembly using only the service contract.

## Verification

2026-11-08, batch 03A plus the P3 recheck minors n1-n3 (decision record, P4 notes). Environment:
macOS arm64, rustc 1.95.0 via `rust-toolchain.toml`, sqlx-cli 0.8.6, a disposable private
PostgreSQL 18.6 + pgvector 0.8.7 cluster on 127.0.0.1:55432 (`MEMORY_TEST_DATABASE_URL`), the
pinned model prepared by `scripts/prepare-model.py --download` and verified by `--verify`
(sizes and SHA-256 of every manifest file, ONNX Runtime 1.24.4 osx-arm64). Logs are under the
untracked run directory `tmp/memory-service-run/logs/a3-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_search_ --network-isolated` | 16 | 16 passed: corpus (exact ids, paraphrase, long note, duplicate parts, German, Japanese, empty matches, bounds, HTTP = MCP), both-leg predicates with ceiling 6 unchanged by 7/9, > 20,000 nearer foreign/above-ceiling parts, partial above the exact cap, failed indexing and invalid vectors, model mismatch and missing vector capability, the pool's reserved query slot, chunking/fusion/plan fixtures, and the real-model acceptance check |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed, including competing indexers, fences and expired leases, local deadline, heartbeat renewal and heartbeat loss, tenant re-check at commit, n2 |
| `scripts/focused.sh memory_idempotency_ --network-isolated` | 2 | 2 passed |
| `scripts/focused.sh memory_access_level_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed, including n3 |
| `scripts/focused.sh memory_contract_ --network-isolated` | 11 | 11 passed, including n1 and the regenerated `openapi.json` |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 5 | 5 passed |
| `scripts/focused.sh memory_startup_ --network-isolated` | 11 | 11 passed, including a missing model and artifact tampering |
| `scripts/focused.sh memory_boundary_ --network-isolated` | 3 | 3 passed |
| `memory-service model-check` (dev build, no database) | — | passed: 2 sessions × 1 thread, 384-d, load 2.8 s, peak RSS 2.58 GB |
| `scripts/verify.sh` | 100 | all gates passed: fmt, whitespace, boundary with poisoned-parent probe, fetch, offline check, clippy `-D warnings`, owner-role migration, `prepare --check`, manifest, model artifacts, 100/100 tests (48 DB-backed) under macOS network isolation |

Unavailable here and carried to the final gate: the first CI run of `memory-service.yml` (Linux
isolation, model cache and download, the extracted tracked-only copy) and the image job
(non-root, `--network none --read-only`, `model-check` within the 3,072 MiB budget), because
Docker and CI do not run locally. The real-model check ran with the prepared model; it fails, and
does not skip, without it. Batch 03B is not started.

2026-11-08, P4 correction round 1 (review `p4-code-r1`: F1 identifier phrases in the keyword leg,
F2 the token-limit refusal published and tested, F3 bounded and supervised indexer shutdown, plus
read-once model loading and `FOR NO KEY UPDATE` claims). Same environment; logs
`tmp/memory-service-run/logs/a3-p4r1-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_search_ --network-isolated` | 18 | 18 passed, including the reviewer's six identifier sentences, the function-word guard, identifier-token fixtures and the real-model acceptance check |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_startup_ --network-isolated` | 13 | 13 passed, including `run` stopping the indexer within a 1 s grace during a 3 s embedding and the supervisor restarting a panicked worker |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed, including a 1,024-character CJK query refused with 400 on HTTP and MCP and a fresh `openapi.json` |
| `memory-service model-check` (dev build) | — | passed; peak RSS 2.34 GB (2,228 MiB), CI budget lowered to 2,560 MiB |
| `scripts/verify.sh` | 105 | all gates passed; 105/105 under macOS network isolation |

CI and the image job remain unavailable locally, as above.

2026-11-08, P4 correction round 2 (review `p4-code-r2` F1a: bare capitalised words are no
longer identifier phrases; one shared shutdown grace for server drain and indexer join). Logs
`tmp/memory-service-run/logs/a3-p4r2-*.log`.

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_search_ --network-isolated` | 18 | 18 passed, including the selectivity guards "how do I reset IT passwords" and "HOW DO I RESET MY PASSWORD" (no filler match) and the real-model acceptance check |
| `scripts/focused.sh memory_startup_ --network-isolated` | 13 | 13 passed; `run` returns within 1.5 s of shutdown under a 1 s grace |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed (regenerated `openapi.json`) |
| `scripts/verify.sh` | 105 | all gates passed; 105/105 under macOS network isolation |

2026-11-08, batch 03B (P5: progressive recall and context packing; decision record D8 and P5
notes). Same environment (macOS arm64, rustc 1.95.0, sqlx-cli 0.8.6, the private PostgreSQL
18.6 + pgvector 0.8.7 cluster on 127.0.0.1:55432, the prepared and verified model). Logs
`tmp/memory-service-run/logs/a4-*.log`. Each filter was listed first and selected a nonzero
count (`focused.sh` refuses an empty selection).

| Command | Selected | Result |
| --- | --- | --- |
| `scripts/focused.sh memory_recall_ --network-isolated` | 5 | 5 passed: progressive disclosure (compact index fields, bounded timeline, selected detail), cross-scope id injection on fetch, timeline, validate and an operation-narrowed credential, restricted timeline neighbours (above-ceiling, foreign, sibling, deleted and expired rows nearer the anchor take no slot; no counts or gap markers; `session_only`), snippet/provenance metadata in `exposed_level`, and the independent Python client over real TCP |
| `scripts/focused.sh memory_context_ --network-isolated` | 9 | 9 passed: 5 pure packing rules (inclusive token and byte boundaries, greedy skip-and-continue, empty and sub-overhead budgets, determinism, `valid_until`) and 4 DB cases (exact budgets on stored items, omission counted over visible candidates only and unchanged by restricted rows, the 200-candidate window, determinism and validation after correction, level raise, deletion, expiry, a lower ceiling and revocation, escaping of stored instructions, query packs following search with fetch's digest) |
| `scripts/focused.sh memory_mcp_ --network-isolated` | 6 | 6 passed, including timeline, fetch, search, pack and validate bodies equal on HTTP and MCP for a full and a read-only credential, the same eight denials on both, no audit events for reads, equal audit for writes, and `memory_save` refused to the read-only credential on both |
| `scripts/focused.sh memory_search_ --network-isolated` | 18 | 18 passed, including the real-model acceptance check (search was refactored onto the shared recall pipeline) |
| `scripts/focused.sh memory_storage_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_access_level_ --network-isolated` | 14 | 14 passed |
| `scripts/focused.sh memory_authority_ --network-isolated` | 24 | 24 passed |
| `scripts/focused.sh memory_contract_ --network-isolated` | 12 | 12 passed (timeline, pack and validate now implemented; four operations remain `501`; regenerated `openapi.json`) |
| `scripts/focused.sh memory_startup_ --network-isolated` | 13 | 13 passed |
| `scripts/focused.sh memory_boundary_ --network-isolated` | 3 | 3 passed |
| mutation probe: timeline ceiling predicate removed from one side | 1 | the restricted-neighbour test failed as expected (restricted rows took slots); restored |
| `scripts/verify.sh` | 120 | all gates passed (fmt, whitespace, boundary with poisoned-parent probe, fetch, offline check, clippy `-D warnings`, owner-role migration, `prepare --check` over the regenerated `.sqlx`, manifest, model artifacts); 120/120 tests (60 DB-backed) under macOS network isolation |

CI and the image job remain unavailable locally, as above.
