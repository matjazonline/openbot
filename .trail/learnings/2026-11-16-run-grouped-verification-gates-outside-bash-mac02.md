---
title: Run grouped verification gates outside Bash conditions
author: Mac02
date: 2026-11-16
branch: attribution-service
id: l-07b51ae9
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/ci.yml"
  - ".github/workflows/memory-service.yml"
  - Cargo.lock
  - Cargo.toml
  - README.md
  - migrations/20260817000000_init_schema.sql
  - plan/attribution-service/01-application-boundaries-and-contracts.md
  - plan/attribution-service/03-single-sign-on-and-company-access.md
  - plan/attribution-service/04-merchant-setup-and-api-credentials.md
  - plan/attribution-service/06-website-tracking-and-checkout-handoff.md
---

## Decided
The local full-suite runner invokes each gate as an ordinary command, captures its status, and only then branches to record PASS or FAIL. Grouped gates run in subshells with set -e so a failed prerequisite stops that group while the parent can continue other groups.

## Tried and ruled out
Wrapping a grouped shell function in `if "$@"` disables Bash errexit throughout the function, even when the function explicitly enables set -e. A failing build can then be followed by a successful command and the gate is reported as passing. A focused shell probe and a regression test reproduce that failure without running application suites.

## Watch out
Do not restore a conditional invocation of a grouped gate while simplifying the runner. Keep the fail-fast and continue-after-failure tests, and explicitly register new script suites so the local entrypoint cannot silently drift away from CI.

In the code: scripts/check.sh gate(); scripts/check-support.sh; scripts/tests/check-runner.py. The user identifies scripts/check.sh as the full-suite entrypoint; reviewing or editing its registration does not by itself request a full-suite execution.
