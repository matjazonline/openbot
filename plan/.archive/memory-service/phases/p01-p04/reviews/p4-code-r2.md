# Review p4-code-r2: P4 recheck (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen diff `tmp/memory-service-run/frozen/p4-code-r2.diff`: sha256 recomputed `0930a4dbd3c65f52c8ad6f1af0d4d2e6ec386c9846b2ffa7921a3a098be2cebe`, matches.
- Incremental diff `p4-r2-incremental-from-r1.diff`: sha256 recomputed `b9c8b2a7dc03dca1fc014286075c0c1f1e228968ad979309601a3887661ceb0f`, matches; 22 files.
- Worktree tree rebuilt with a temporary index: `6efd4f49b0c7c607f500eec8d783e835d289c3dc`, identical to the frozen tree.
- Scope: r1 F1–F3, the two optional notes the author took, and their cross-effects.

## Checks I ran (private cluster only; shared target only)

| Command | Result | Log |
| --- | --- | --- |
| `scripts/verify.sh` | All gates passed, 105/105 under isolation, including the real-model acceptance check | `logs/b2-p4r2-verify.log` |
| EXPLAIN ANALYZE of the exact r2 keyword-leg SQL on a scratch database (`b2probe_p4r2`, since dropped) | 100,001 rows of 40-word synthetic notes, GIN on `search_tsv`; results under F1 | `logs/b2-p4r2-keyword-explain.log` |

The scratch tables used the same column names and the same GIN index as `observations`, and contained one target note (`PgStore … idempotency_key … foo.bar.baz`). The 30-word vocabulary included common words: "it", "is", "the", "us", "ok", "io", "api", "on", "or".

## Recheck

| Item | Status | Evidence |
| --- | --- | --- |
| F1 identifier recall | **Recall fixed; selectivity regressed for all-caps tokens** (F1a) | `where is PgStore used`, `which idempotency_key do I send` and `what is in foo.bar.baz` each return the one target row via a Bitmap Heap Scan on the GIN index in ~0.15 ms. `idempotency_key` becomes the phrase `'idempotency' <-> 'key'`. URLs become `'x.org/a/b' <-> 'x.org' <-> '/a/b'`, which matches the parser's consecutive positions for the same URL. `a.b` and `I/O` become single host/file lexemes (`'a.b'`, `'i/o'`), so they cannot over-match. No tsquery injection is possible: tokens are bound as `text[]` and pass through `phraseto_tsquery`, which never parses operators; the `phrase::text` → `::tsquery` round-trip re-reads PostgreSQL's own quoted output; empty phrases are dropped (`numnode > 0`). Stale "OR" text is fixed. |
| F2 query token bound | **Fixed** | 1,024 characters and the 500-token refusal are documented with the CJK case and tested over HTTP and MCP. |
| F3 shutdown/supervision | **Fixed** | `pass` observes shutdown inside a pass: the running job is cancelled (no commit), the rest are dropped and their leases expire, with the fence guarding later commits. `supervise` restarts after a panic or unexpected return with 1–30 s backoff, and `AbortOnDrop` takes the worker down with an aborted supervisor. The join is bounded by `MEMORY_SHUTDOWN_GRACE_MS`, then aborted. Unit and DB tests cover it. |
| Note: model read once | **Done** | Files are verified in memory and the ORT dylib is still check-then-load, as recorded. The measured peak dropped from 2.58 to 2.34 GB. |
| Note: claim lock | **Done** | The claim and expiry now use `FOR NO KEY UPDATE … SKIP LOCKED`, so they no longer block FK checks from copies. |
| RSS budget 3,072 → 2,560 MiB | **Acceptable** | 2,228 MiB on macOS gives ~15% headroom. `ru_maxrss` units are handled per platform (KiB on Linux, bytes on macOS), and the assertion fails the CI image job early. The first Linux run is calibration: if it fails, investigate the memory before raising the budget, and record the reason either way (AGENTS.md raised-limit rule). |

## Findings

### F1a major: all-caps identifier tokens recreate the OR flood
`services/memory/src/domain/retrieval.rs` (`identifier_tokens`, the `all_capitals` rule: at least 2 letters, all upper case).

The rule treats a capitalised common word as an identifier. Its phrase is just the lowercased word, so the rescue clause matches most of the tenant, and every match is then ranked by `ts_rank_cd`:

| Query | Tokens | Rows matched | Time |
| --- | --- | --- | --- |
| `how do I reset IT passwords` | `IT` → `'it'` | 74,305 of 100,001 | 254 ms for the leg alone |
| `HOW DO I RESET MY PASSWORD` (Caps Lock) | `HOW, DO, RESET, MY, PASSWORD` | 93,808 | 302 ms |

The same happens with `US`, `OR`, `OK`, `IS`, `TO`, `AN`, `AS`, and with any shouted sentence. This is the selectivity failure D7's change was made to remove. The keyword leg alone exceeds the p95 < 250 ms target, and the noise rows reach RRF with keyword ranks 1–80.

The real identifiers in the r1 probe do not need this rule:
- `ERR_CONN_RESET` is already caught by the `_` mark.
- Digit and separator marks cover `HTTP-503`, `INC-48213` and `v1.2.3`.
- `PgStore` is caught by the internal-capital rule.

Required:
- Drop the bare all-capitals rule. A bare acronym query (`API`) still matches through the all-words clause. If acronyms in sentences must be rescued, require at least 3 letters and skip the rule when most of the query's letters are upper case.
- Add the two queries above to the corpus or a unit test as selectivity guards. For example: assert `identifier_tokens` yields nothing for them, and the keyword leg returns no function-word-only rows.

### Minor notes (no action required for P4)

- **Crash loops are visible only in logs.** `supervise` restarts forever with backoff; a persistent panic shows only as error logs and rows staying `pending`. Readiness stays green. When 06A adds metrics, export a restart counter next to `semantic_partial_total`.
- **Shutdown can take two grace periods.** The indexer join gets its own `shutdown_grace` after `serve_app` has already used up to one. Worst case is about 2 × `MEMORY_SHUTDOWN_GRACE_MS`. It is bounded, so this is a note only.

## Remaining required checks

1. Fix F1a, then rerun focused `memory_search_` (with the real model), then `verify.sh`. A recheck of F1a only is enough.
2. CI only, carried over: the first green `memory-service.yml` run (Linux isolation, model cache/download, extracted copy) and the image job (non-root, `--network none --read-only` `model-check`, 2,560 MiB budget, first Linux run is calibration).

## Verdict

**Not passed, on one finding (F1a, major).** F2, F3, both notes and the budget change are correct. verify.sh passed 105/105. After F1a is fixed, only that point needs rechecking.
