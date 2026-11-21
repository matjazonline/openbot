# CI and current-source gate audit

Compared .github/workflows/ci.yml with scripts/check.sh and retained final-check.sh.
The phase wrapper changes only owned runner paths/fixtures, exact Doop filters/
deferrals, browser concurrency and reuse of unchanged independent services.
No source assertion or production CI declaration/budget was changed.

- Formatting/whitespace/offline all-target/Clippy/SQLx/reachability: current R2logs PASS.
- Fresh migrations: retained successful fresh schema plus current SHA384identity PASS;
  R2production corrections made no migration change.
- Full database+network+stock2MiB:4706PASS/2FAIL/27ignored/85Doopfiltered,1995.49s. Both failures PASS in isolated unchanged2MiB rerun; related families8PASS/3ignored. Binary6/source-modules8PASS separately because Cargo stopped after the library failure. No single all-green full-run claim.
- Registration/query statistics/frontend/transport/integration boundaries/deployment/
  artifact-preparation/evidence validator/native sibling browsers: original full runner
  PASS; those contracts unchanged by R2. Knowledge current fixture/browser and current
  generated CSS separately PASS after R2UIchanges.
- Linux supervisor/descendant scope all8categories PASS in owned Linux namespace.
- Current lease139tests and original50%workbudget PASS; knowledge production image
  and real retrieval/tamper paths PASS; memory backup/restore1PASS, standalone13steps
  PASS, live OpenBot-memory7tests PASS and memory production image PASS.
- Native memory --download FAIL HTTP404 for unchanged pinned GitHubruntimeURL. The
  script/manifest exactly match main; all cached pinned files pass --verify. Local
  model prerequisite is available and exercised, while fresh native retrieval remains
  a retained unrelated failure. Entire runner exited1, never label it all green.
- Independent plugin/attribution source exactly matches accepted Plan04; memory service
  source exactly matches acceptedp02(180service tests). Retained reuse provenance JSON
  identifies original execution logs and SHA256. Current host/service integration
  effects exercised above; no unrelated service reruns.
- Exactly85root Doopnames skipped and three dedicated gates user-deferred under C001.
  No Doop investigation/declaration/assertion changes; no whole-CIgreen claim.

CI's tracked-manifest retention variant must be run after authorized staging before
commit. Dependency installation/server configuration remain prerequisites, not new
application behavior. Actual native fresh-download failure is recorded separately
from the available verified cached artifacts and successful production executions.
