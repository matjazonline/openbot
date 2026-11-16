# Plan03 p04 preflight (preparation only; p03 review remains pending)

- Incoming main d543787f; common103004a8; read-only merge preview reports73 overlap/17 conflicted paths.
- No local Docker/gh; public GitHub Actions can run exact Docker and Linux CI gates on a temporary
  integration branch before main publication. No branch pushed yet.
- Local Node26.13.1/npm11.20.0 differs from CI Node24.21.0; exact locked runtime checked by CI.
- Disk271GiB free; existing serialized target166GiB. No cleanup authorized or performed.
- Current PostgreSQL52471 preloads pg_stat_statements, track=top, track_utility=on, parameter
  logging limits0/0. CI requires track_utility=off. Do not reconfigure a shared cluster for this:
  allocate a disposable phase-owned PostgreSQL18 cluster for final integration/CI-equivalent gates,
  or use an explicitly isolated database/session setting where the test harness propagates it.
- Fresh integration worktree from a scoped reviewed Plan03 commit must preserve accepted Plan02
  ancestry and merge incoming main; original unrelated dirty configuration files stay untouched.
- Re-read coordination before p03 acceptance, publish p03 acceptance/next-phase start only after B PASS.
