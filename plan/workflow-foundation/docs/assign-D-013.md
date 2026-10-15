# Assignment D-013 — abort loaded C2 full run; rerun on a quiet host

Same role and settings (claude-code / claude-opus-5-5 / high).

Root observed at 20:53Z: load average ~20 with `/home/mh/dev/bizi44` running
`cargo clippy`/`cargo check` (foreign_cargo_rustc=1 in your load-C2.log), and
your in-progress C2 full-suite log already shows many timing failures. Per the
user's decision (rerun timing gates on a quieter host), this run is not valid
acceptance evidence.

1. Stop your current C2 full-suite/budget pipeline (only your own processes;
   never touch bizi44). Keep its partial log, renamed with a `-loaded-aborted`
   suffix, and note it in IMPLEMENTATION.md as load-invalidated (not a pass, not
   a regression verdict).
2. Wait for a quiet host: poll `uptime` and foreign cargo/rustc every 10 minutes;
   start only when the 1-min load < 4 and no foreign cargo/rustc is running.
   Give up after ~2 h and report back if never quiet.
3. Rerun the full library suite at stock 2 MiB, bin tests, `stack-budget.sh
   --offline`, `lease-budget.sh --offline` and the live SQLx check; log load and
   foreign processes at each boundary. If foreign load appears mid-run and
   failures occur, treat that run as load-invalidated, wait and retry (at most
   2 retries), never relax a test.
4. Then F-008 `graft build`, manifest C2, and the IMPLEMENTATION.md section as in D-012.
Final response: gate table with load figures, manifest SHA256.
