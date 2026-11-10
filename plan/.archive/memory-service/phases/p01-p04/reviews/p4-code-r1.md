# Review p4-code-r1: P4 = P3 minors n1–n3 + batch 03A (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen full diff `tmp/memory-service-run/frozen/p4-code-r1.diff`: sha256 recomputed as `016a7de81130924720b4eeb649bada982774ef90dc0ec0a4ac62808b25e299ba`, which matches.
- Incremental diff from P3 (`08f82f2a`), `p4-incremental-from-p3.diff`: sha256 recomputed as `c230107660deabbe0d8795922b8447464c93d418fcfb66c547ece0894fae6bcb`, which matches. 65 files.
- Worktree tree rebuilt with a temporary index (tmp/ excluded; `assets/model` is gitignored): `82be501c37dead36d8f947bd51c1c9319dae8343`, identical to the frozen tree.
- Originals I read: 03 in full; D7, D9/D13 (lease, fence, order), D1 and the P4 implementation notes in the decision record; AGENTS.md (raised-limit rule, competing claimants).

## Checks I ran (private cluster 127.0.0.1:55432 only; shared `services/memory/target` only)

| Command | Result | Log |
| --- | --- | --- |
| `scripts/verify.sh` (sqlx-cli 0.8.6, Homebrew psql 18, no incremental/debug), including the model-artifacts gate and the real-model acceptance check | **All gates passed** (fmt, whitespace, boundary + probe, fetch, offline check, clippy `-D warnings`, owner migrate, `prepare --check`, manifest, model artifacts by size and SHA-256), then **100/100** under macOS isolation, including `memory_search_real_model_retrieval_acceptance` | `logs/b2-p4-verify.log` |
| Keyword-leg probe in `psql`: the exact D7 CTE run against a 4-row toy table (no project code) | 7 queries; results in F1 | `logs/b2-p4-keyword-probe.log` |

No probe copy was built: copying the service would rebuild against the crate in the shared target. All other conclusions come from reading the code.

## Coverage of 03A

| 03A item / focused check | Status |
| --- | --- |
| 1. PostgreSQL FTS + pgvector + RRF; exact identifiers *and* paraphrases useful | Partly met. RRF (k = 60, tie order by `occurred_at`, then id) and the semantic leg are correct. The keyword leg no longer keeps digit-free identifiers useful in ordinary queries (**F1**). |
| 2. Pinned local model; explicit identity, width, normalization, e5 conventions; artifacts validated independently | Met. The manifest pins the model and runtime by size and SHA-256, and rejects symlinks. Serving validates the files and never downloads; downloads happen only at build/CI. The id ends in `/fp32/mean/384`. Inputs use the `query: ` and `passage: ` prefixes, mean pooling and L2 normalization, with a norm check within 0.001. Startup self-checks both sessions. There is no OpenBot import or runtime configuration; the boundary gate passes. |
| 3. Bounded parts, no silent truncation, rank parts, dedupe, provenance; never compare across models | Met. Parts are counted with the model's own tokenizer (padding and truncation disabled, special tokens included) and capped at 500 tokens, below the 512 at which fastembed would truncate, so it never does. Splitting goes paragraph → sentence → token-counted character cut, with no overlap, at most 128 parts, and fails loudly. Search takes each observation's best part (`DISTINCT ON` / first hit). The provenance filter is shared with fetch. Each part stores `model_id`, both the vector leg and the exact scan filter `p.model_id = $model`, and startup adopts the model or refuses another. |
| 4. Predicates before limits in both legs; ceiling 6 cannot expose 7–9; approximate filtering not silently empty; bounded expansion | Met. Every leg's WHERE checks tenant, the named namespaces, ceiling, active namespace, expiry and model before the LIMIT. Fusion is re-checked in the keyword leg's snapshot. Provenance and `exposed_level` cover link targets; there are no counts. A short HNSW leg falls back to the exact scan (count capped at `cap+1`, then a MATERIALIZED scan), and goes `partial` only above 100,000 *eligible* parts, so ineligible rows cannot affect that state. Expansion is bounded: retries ×4 up to `leg_candidates × 128` parts, `ts_headline` on at most 100 rows, snippets at most 320 bytes. |
| 5. Stored-text success separate from indexing; keyword recall while pending/failed; retries embed only saved content; no false success | Met. The claim returns the row's own title and body at the claimed revision. The commit needs fence *and* revision to match and writes the row's current level. Failures are visible (`index_error`, a bounded code set), with backoff and 8 attempts before a terminal state; `admin reindex-failed` is audited. A query-embedding failure is 503. |
| Focused list: fixed corpus with exact ids, paraphrase, long notes, duplicate parts, non-English, empty; foreign/namespace/classification rows nearest; deleted; invalid vectors; failed indexing; model mismatch; ceiling 6 unchanged by 7/9; real-model check | All present. One gap: exact ids are tested only as a bare, digit-bearing query (`INC-48213`), which is exactly the case F1 does not break. |
| Indexer protocol: lease, heartbeat, local deadline, fence, competing claimants, cancellation | Met. Four claimants on separate connections hold disjoint `SKIP LOCKED` batches, and four indexers index each row once. Fences reject stale commits, and expired leases, heartbeat loss and the local deadline are tested. A delete or correction during indexing makes the commit `Stale`; I checked the lock order (tenant → namespace `FOR SHARE` → observation `FOR NO KEY UPDATE`). Shutdown is not bounded (**F3**). |
| HTTP/MCP search parity | Met: same `execute` path; the corpus test compares both transports and their denials. |
| n1–n3 | **Fixed.** n1: every write's `expires_at` description publishes the retry identity, with a test reading both schemas. n2: every request timestamp is truncated to microseconds before validation, hashing and storage (`trunc_subsecs(6)`, matching sqlx's integer-microsecond encoding). n3: a replay by another credential answers `not_found` for a missing, expired, inactive or unreadable parent, before the `write_max` check, so it is no longer an existence oracle. |

Declared deviations:

| Deviation | Verdict |
| --- | --- |
| (1) Keyword leg: all lexemes, then any lexeme with a digit | Not acceptable as is; see F1. |
| (2) Two ORT sessions, ≈2.5 GB, CI budget 3,072 MiB | Acceptable. The reason is sound: `run(&mut self)` means a reserved query slot needs its own session. The budget is an early-failing CI assertion in the image job, as AGENTS.md requires, and the first Linux run is declared as calibration. See the m-note on transient buffers. |
| (3) `real[]` cast to vector, no pgvector crate | Acceptable. Width is checked before binding, written vectors use the shortest round-trip text form, and pgvector rejects NaN. |
| (4) DB tests under `adapters/postgres/*_tests.rs` | Acceptable; it follows the D1 layering rule. |
| (5) Snippet rules | Acceptable. Snippets come from the observation's own text at the part's revision, or from `ts_headline` over its body, bounded. |
| (6) Dev-profile opt-level 3 for five leaf crates | Acceptable; release is unchanged. |

## Findings

### F1 major: the keyword leg drops digit-free exact identifiers from ordinary queries
`services/memory/src/adapters/postgres/search.rs:251-273` (D7 "P4 decision change").

The leg matches rows that contain **all** query lexemes, plus rows that contain any lexeme with a digit. `'simple'` keeps function words, so in any natural-language query that names an identifier without digits, the identifier contributes nothing:
- the AND fails because the note does not contain "where", "is", "do", "I" and so on;
- the rescue never fires because the identifier has no digit.

My psql probe ran the exact CTE over notes containing `PgStore`, `idempotency_key`, `foo.bar.baz`, `ops@example.com`, `ERR_CONN_RESET` and `v1.2.3`:

| Query | Keyword hits |
| --- | --- |
| `where is PgStore used` | none |
| `which idempotency_key do I send` | none |
| `what is in foo.bar.baz` | none |
| `who gets mail at ops@example.com` | none |
| `why ERR_CONN_RESET happens` | none |
| `error on v1.2.3` | the right note |
| bare `PgStore` | the right note |

Snake_case identifiers are split by the parser (`idempotency`, `key`), so even a digit-aware rescue cannot see them. 03A.1 requires the fusion to keep "exact identifiers and paraphrases useful". The semantic leg on multilingual-e5-small is weakest on exactly these rare symbols, so the requirement now depends on the user typing the bare identifier. The author's evidence for the regression fix (the OR form ranking a long filler note first) is real. But the change over-corrects, and the corpus tests only bare, digit-bearing ids.

Required:
- Make the rescue clause identifier-aware on the **raw** query tokens (split on whitespace), not on lowercased lexemes. A token is identifier-like if it contains a digit, any of `_ . @ / : -`, or internal or all capitals. Turn each such token into `phraseto_tsquery('simple', token)` so that split snake_case and dotted tokens match as a phrase, and OR those phrases with the all-lexeme clause, keeping the ranking order. This stays selective and cheap for the 100k / 20 rps / p95 < 250 ms target.
- Add corpus cases: the six natural-language queries above must keyword-match their notes, and a query of only function words must still not match everything.
- Correct the stale text that still describes the OR: the `SearchRequest.query` description ("The keyword leg ORs at most 32 of its words", `contract.rs:127-128`, mirrored in `openapi.json`) and the `MAX_QUERY_LEXEMES` doc comment (`domain/retrieval.rs:19`).

### F2 minor: a query within the published bound can be refused
`services/memory/src/application/embedding.rs:98-99` and `application/search.rs:44`. The schema allows 1,024 characters, but the query embedding refuses more than 500 tokens with 400. For CJK text or identifier-dense queries, roughly 1 token per character, a valid-length query is refused. The description does say "and 500 embedding-model tokens", so the behaviour is documented and not silent. Required: keep it, and add a contract example or test showing the 400 for a 1,024-character CJK query, so clients know to pre-split. Alternatively, lower `MAX_QUERY_CHARS` to a value that always fits.

### F3 minor: indexer shutdown is unbounded, and its supervision does not restart
`services/memory/src/infra/startup.rs` (`run`: `shutdown.cancel(); let _ = worker.await;`) and `application/indexer.rs:81-150`.
- `run` checks for shutdown only between passes. A pass can run until its local deadline (lease 60 s − margin 15 s ≈ 45 s) plus one embedding batch, so `serve` can exit long after `MEMORY_SHUTDOWN_GRACE_MS`. 02A.2 requires graceful, bounded shutdown, and container runtimes kill after about 10 s. Data stays safe: the lease expires and the fence protects later commits.
- If the spawned task panics (for example on a poisoned mutex path), indexing stops silently while `/readyz` stays green, so rows stay `pending` forever. That contradicts "supervised".
- The startup test (`memory_startup_ready_when_store_reachable_and_cancellation_is_bounded`) exercises only `serve_app`, not `run` with the indexer.

Required:
- Make `run_once` observe the shutdown token: on shutdown, set the embedding cancel flag, drop the pass and commit nothing.
- Bound the worker join by the shutdown grace.
- Restart the worker after a panic with backoff, or fail readiness.
- Test `run` shutdown with a slow fake embedder.

### Minor notes (no action needed for P4 unless cheap)

- **Claim locking.** `search.rs:570`: the claim uses `FOR UPDATE OF o SKIP LOCKED`, but it only updates non-key columns. `FOR NO KEY UPDATE` would not block concurrent FK checks (`FOR KEY SHARE`) from copies citing the row during the claim statement. It is a single statement, so the impact is small.
- **Model file reads.** `adapters/embedding/mod.rs` (`load`): the files are SHA-256 validated, then read again, and `model.onnx` is read twice, once per session. This leaves a TOCTOU window (small in a read-only image) and adds ~1 GB of transient buffers that count toward the measured 2.58 GB peak. Reading once, hashing the bytes in memory and building both sessions from them closes the window and lowers the RSS the CI budget gates. If done, recalibrate the budget downwards rather than leaving the headroom.
- **Performance risk, for 06.** Every query whose namespace holds fewer than `leg_candidates` observations takes the exact scan. That is cheap for small namespaces, but the 2 s statement timeout is the only guard on large ones. The 06 load test should include large single-namespace tenants with low-ceiling queries.

## Architecture challenged

- Two sessions versus one: keep two, as root says. The reserved query slot is what bounds query latency behind indexing, and the budget fails early in CI.
- Keyword-leg semantics are the one architectural change in this batch (D7 rewritten). F1 asks for a correction within D7's own design, the `'simple'` multilingual configuration with no new extension, not a new mechanism.
- Speculative scope: none significant. `semantic_partial_total` is a local counter until 06A, which is reasonable. `admin reindex-failed` is needed for the terminal state.

## Remaining required checks

1. Fix F1–F3 and re-review them, then run focused `memory_search_` (including the real-model acceptance check), `memory_storage_`, `memory_startup_` and `memory_contract_` (openapi freshness), then `verify.sh`.
2. CI only, carried over: the first green `memory-service.yml` run (Linux isolation, model cache and download, extracted copy on a fresh target) and the image job (non-root, `--network none --read-only` `model-check` within 3,072 MiB; the first Linux run is calibration).

## Verdict

**Not passed.** One major, F1: the keyword leg loses digit-free exact identifiers in natural-language queries, against 03A.1; confirmed by the psql probe. Two minors: F2 (the query token bound inside the character bound, needs a documented example) and F3 (indexer shutdown unbounded and unsupervised). Everything else in 03A and n1–n3 is correct, and verify passed 100/100 including the real-model check. After F1–F3 are fixed and focused-checked, a recheck of those points only is sufficient.
