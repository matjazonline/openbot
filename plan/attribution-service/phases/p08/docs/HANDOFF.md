# P08 handoff — frozen batch review

Root/A user-confirmed Sol6.1/xhigh; retained independent B /root/attribution_reviewer
Astra/high. Active isolated worktree remains /private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree,
start/base/HEAD6e55258c (user test fixes preserved). Original main delivery workspace
still acceptedp07 source2712efab, verified immutable baseline snapshot/archive inlogs.

Architecture AS-P08-D1/r1 accepted AS-P08-G1-N1. Main coordination authority revision23
C001–C007/r1 acknowledged; pin inlogs. Implementation complete, not accepted yet.
Frozen source `e31e15080831480b5ad749be84bb980250149b98732741d01b43533caae40e91`, exact9 exclusions inTASK; B wholebatch review next.
TASK final section owns criterion mapping, commands/results, limitations and failure
dispositions. Evidence AS-P08-B1-R1.evidence.json; delta35 paths + predecessor archive.

Owned resources: UUID PostgreSQL DBs/ephemeral ports automatically cleaned, Rust1.95
cache/tmp/as-attribution-target, stock2MiB, contractPython/tmp/as-p02-contract-clean,
Playwright/tmp/as-p03-browser/node_modules/playwright/index.mjs. Use GNUcoreutils path
/opt/homebrew/opt/coreutils/libexec/gnubin for snapshot script on thisMac. No running
servers. No commits/deployments authorized. P09/p10 remain; p10 owns full suites and
unchangedp03 test clock correction. Do not transfer partial/unacceptedp08.


## AS-P08-B1-R2 — all findings corrected; recheck pending

B AS-P08-B1-N1 requested F1 duplicateCSVcolumn and F2 missing delivered CSV basis labels.
Corrected both together: unique currency_ metric columns preserve group numerator,
and each CSV row carries group_by, click_observation_basis, financial_basis and
nonadditive_measures. API/dashboard pass their validated grouping to shared exporter.
Both routes trace to the same serializer (logs/AS-P08-F1-callers.txt). REPORTING and
OpenAPI describe exact headers/labels. No aggregation/authorization/gate change.

Added attribution_analytics_export_parsed_currency_and_product_semantics: actual
HTTP export of2 attributed paid orders (EUR/USD) and2 products is parsed using
Python stdlib csv.DictReader. It asserts unique headers, group total2 vs currency1,
blank later group fields, per-product line2400/3500, repeated wholeorder tax1180 and
shipping500 plus explicit nonadditivity labels. Existing10000-row export remains
within bound with labels and10001 rejects. Focused9 PG tests pass R2-analytics-1;
Chromium1 pass R2-browser-1; offline/fmt/contracts15/diff pass. No tests removed.

New frozen source `99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`, HEAD6e55258c and same9 exclusions. Coordination23
C001–C007/r1 unchanged/checked; R2 evidence validates inherited unaffected checks plus
all correction consumers. B must close F1/F2, root has not accepted p08 yet.

Independent future-gate environment preparation (no source changes): task-owned
Colima as-attribution-integration running (4CPU/6GiB/32GiBdata/8GiBroot/nohostmounts,
no SSH config/default Docker context change). Installed docker-buildx dependency.
Task-local DOCKER_CONFIG=/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-docker-90iaewwo
with cliPluginsExtraDirs; Docker socket /Users/mac02/.colima/as-attribution-integration/docker.sock.
Openbot unchangedimage build as-openbot-integration:local in progress, execsession68642,
log AS-P08-openbot-image-preparation-2.txt. Preserve until p09/p10 topology proof;
clean only this profile/cache at final completion. Initial legacybuilder --progress
failure retained in preparation.txt. These are environment preparations, notp09acceptance.


## P08 accepted — AS-P08-ACCEPT-R1

B AS-P08-B1-N2 explicit PASS 99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e; F1/F2 closed. Root recomputed same artifact, checked coordination23 pin and retained evidence validator. Phase08 accepted/integrated in isolated sequential worktree; final plan verification remains09/10. Guarded main transfer follows; no commit/deployment.


Authority handoff 2026-11-16: the canonical records now live at /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p08/docs. This isolated copy is historical; subsequent root updates belong to main. Source55ecc5ea is accepted/integrated with the p10 verification limitations.
