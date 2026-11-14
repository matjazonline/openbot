---
title: The transport boundary check passes silently when ripgrep is not a real binary on PATH
author: Mac02
date: 2026-11-14
branch: room/check-last-handoff-md-under-plan-02-work-qc8d
cost: { minutes: 22, tokens: 237535 }
id: l-8ece2c97
scope: repo
status: active
src: agent
touches:
  - src/application/transport/ports.rs
  - src/application/transport/mod.rs
  - src/application/transport/ingress.rs
  - src/application/use_cases/thread/ingest/policy.rs
  - src/application/use_cases/thread/ingest/commit.rs
  - src/application/use_cases/thread/ingest/relay.rs
  - src/adapters/protocols/email/egress.rs
  - src/adapters/persistence/thread/inbound.rs
  - migrations/20260817000000_init_schema.sql
  - src/adapters/persistence/workflow/message_thread_tests.rs
---

## Watch out
The transport boundary gate and its fixture test call `rg` from bash. In a Claude Code session `rg` is often only a shell function wrapping the Claude binary, which bash subprocesses can't see. `rg --files` then lists nothing, every rule scans no files, and the real check reports "passed". Only the fixture test notices, failing in 0 s with "gate accepted rig in application". In this state the gate hid two real violations for several phases.

## Decided
Before trusting the gate, put a real `rg` on PATH. On a host without system ripgrep, a symlink named `rg` that points to the Claude Code binary works, because it runs as ripgrep 14 when invoked under that name. Treat a 0 s pass or failure of this gate as an environment problem, never as clean.

In the code:
- scripts/transport-boundary-check.sh (`rg --files`, `scan_production`)
- scripts/tests/transport-boundary.sh
- check.sh gate "Transport abstraction boundary"
- workaround: `ln -sf "$CLAUDE_CODE_EXECPATH" <dir>/rg && PATH=<dir>:$PATH`
