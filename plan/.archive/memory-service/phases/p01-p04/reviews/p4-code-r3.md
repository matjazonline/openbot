# Review p4-code-r3: P4 recheck of F1a and the shutdown-grace change (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen diff `tmp/memory-service-run/frozen/p4-code-r3.diff`: sha256 recomputed as `2211d003a1bb3add872ba53b5880e541cc510fd47446ed7171734f8194f33a03`, which matches.
- Incremental diff `p4-r3-incremental-from-r2.diff`: sha256 recomputed as `31a992851621afacff40c016ec922cc08abcd3a38ad126c3d95c519925d69640`, which matches. It touches 10 files.
- I rebuilt the worktree tree with a temporary index. It is `6402e15c3e6cd8b41227cea4788f1038a3e90026`, identical to the frozen tree.

## Checks I ran

- `scripts/verify.sh` on the private cluster, reusing the shared target only.
- Result: every gate passed, and 105/105 tests passed under isolation. These include the new `memory_search_keyword_leg_keeps_identifiers_inside_sentences` (with "it/do/my" in the filler) and `memory_search_identifier_tokens_come_from_the_raw_query`, plus the real-model acceptance check.
- Log: `logs/b2-p4r3-verify.log`.

I did not rerun the 100k EXPLAIN probe, because it would show nothing new. Neither guard query now yields an identifier phrase: `IT`, `NASA`, `API` and the all-caps sentence carry no digit, no inner separator and no lower-to-upper transition. So the rescue clause is NULL, and the leg reduces to the all-words clause, which r2's probe already showed is selective.

## Recheck

| Item | Status | Evidence |
| --- | --- | --- |
| F1a, all-caps flood | **Fixed** | `identifier_tokens` (`domain/retrieval.rs`) now keeps a token only if it has a digit, one of `_ . @ / : -` inside, or a lower-case letter followed by a capital (`windows(2)`). The bare all-capitals rule is gone. The r1 identifiers still qualify: `ERR_CONN_RESET` and `foo.bar.baz` by their separators, `PgStore` by the internal capital, `v1.2.3` and `HTTP-503` by digits and separators. `IT`, `NASA API` and the Caps-Lock sentence yield none, and both a unit test and a DB-level test check this. The remaining mixed-case matches (`iPhone`, `macOS`, `eBay`) produce rare single-lexeme phrases, so the clause stays selective. The D7, README, contract and openapi wording is updated to match. |
| Shutdown grace | **Correct** | The `requested` task is spawned before `serve_app` (`infra/startup.rs:118`) and records when shutdown is cancelled. The indexer join gets `shutdown_grace − elapsed since cancel`, then the worker is aborted. Total shutdown is therefore bounded by one grace period. If the server stops for another reason, `shutdown.cancel()` runs first and the join gets the full grace. The test asserts `run` returns within 1.5 s under a 1 s grace. |

No new findings.

## Remaining required checks (CI only, carried over)

- The first green `memory-service.yml` run: Linux network isolation, model cache and download, and the extracted tracked-only copy on a fresh target.
- The image job: non-root, `--network none --read-only` `model-check` within 2,560 MiB. Treat the first Linux run as calibration.

## Verdict

**Pass for P4** (P3 minors n1–n3 plus batch 03A).

- All r1 and r2 findings are fixed: F1, F1a, F2, F3 and both optional notes.
- My own runs back this:
  - verify.sh passed 105/105 here, after 100/100 in r1 and 105/105 in r2.
  - The r2 EXPLAIN probe confirmed that identifier phrases are GIN-selective, at about 0.15 ms each.
  - The flood case is now ruled out by the token rules and their tests.
- Acceptance still depends on the CI-only checks above.
