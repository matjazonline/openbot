# Main integration main-20261112

No backward compatibility: reset owned disposable databases when the persisted schema changes.

User request: "push to main" (2026-11-12). Merge reviewed cloudroom harness with current main and push normally. No deploy or production reset.
Workflow implement-astra-reviewed-2-2. Root/A /root (Sol 6.1/xhigh, user confirmed); independent B /root/cloudroom_b3_reviewer (requested Astra/high, independent context; reusable reviewer, no input authorship).
Worktree /private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree, branch codex/cloudroom-harness-t1gfnhz1. Worktree was clean. Integration owner reuses it exclusively; original feature input remains preserved remotely.

Inputs: feature HEAD 23eb638d609614cafe958bd6a5d17a4e9bef6463 (implementation 2c4bc2a471f71093a52b58f2c78c7168d573a8e1; p01 accepted source b2fc6f2c11e57a2e88e09d5978903b91e1977ef3219c65fe6376bf5428f856ac, CH-P01-B3-N5 PASS). Target origin/main 103004a8ac2fd643cf14bde7e09022ae79f72053 (19 newer commits including accepted departments, installer, transport helper recognition). Input records remain authoritative and unchanged.

Coordination: cloudroom-harness/COORDINATION.md authority in this worktree, root writer, revision 4 (no notices). Department authority /Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md, other root writer, revision 19. I001/r1 acknowledged no compatibility; I002/r4 prior session scope does not override this user's main push; I003/r2 gate applies here freshly. C001/r1 trusted desk relays, C002/r1 one-step grants, C003/r1 frozen bounded roster, C004/r1 dispatch deferred to channel.call: acknowledged and preserve unchanged; existing phase evidence reused, combined department checks required. No cross-phase change requested.

One cohesive integration batch. Text conflicts: generated assets/app.css (regenerated from merged sources); provisioning fixture (keep new files field and unique slug); definitions.rs (retain main shared save_draft_on and move private-broker NOT EXISTS guard into helper, covering public save and lead repoint). Automatic merge left four test fixtures using removed copy API or missing bundle fields. Adapt to CompanyAgents, retain competing-copy snapshot assertions through publication-ready bindings/default model.

Artifact freeze: base HEAD 23eb638d; exclusions exactly this integration docs directory and plan/cloudroom-harness/COORDINATION.md. Snapshot pending. No exclusion for deliverables or input histories.

| Criterion | Production contract / decisive assertion | Verification |
| --- | --- | --- |
| AC1 | Combined Rust interfaces compile; test fixtures preserve permissions, bundles, department context | format, offline all-target check; focused tests |
| AC2 | Private MCP workflows cannot be edited through shared save helper; ordinary department lead repoint still works | native_mcp_ and department lead/desk/provisioning suites; review helper callers |
| AC3 | Bundled skill copies remain coherent during competing source editor and slug retry through new atomic company-agent creation | skill_bundle_competing_editor_and_both_copy_paths_keep_one_revision_during_slug_retries |
| AC4 | Fresh merged init schema valid and SQLx cache current; no backfills | owned fresh database migration and prepare check |
| AC5 | Combined CSS current, installer and plugin contracts preserved | CSS rebuild, plugin typecheck/test/guards; package evidence reused where unchanged |
| AC6 | Whole combined artifact assessed before publication | scripts/check.sh including DB and stock-stack gate; independent B combined review; normal fast-forward remote push |

Contract impact: save_draft_on producers public save_draft and department repoint_on (lead assignment/clearing/placement), preserve private visibility; company copy API now creates/publishes default desk, tests must supply usable model and tool requirements; department fixture adds skill description/files. Native-only publication/resolution still allows unset model; new main CompanyAgents creation explicitly requires/pins hosted model for answering default desk. No expansion of native-only auto-desk creation in this integration; inspect ordinary update/publication/native launch path rather than weaken main desk prerequisite.

Resources: owned PostgreSQL18 ../postgres/data port63451; owned openbot_cloudroom_harness_t1gfnhz1 database may reset. Shared target ../target serialized Cargo/DB. Node24.21.0 pinned at /private/tmp/openbot-doop-main-k49qm1op/toolchains/node-v24.21.0-darwin-arm64/bin; owned plugin dependencies/builds. Chrome explicit CHROME_PATH. Retain input worktrees/artifacts; stop only owned services.

Checks: initial merged offline compile FAIL (five fixture errors retained logs/main-offline-check-R1.txt); correction in progress. Focused checks first, then one final full gate. Earlier phase failures were confirmed baseline CSP scanner and dropped-call MCP cancellation; do not assume combined failures unrelated without matching evidence. Full gate not yet run for this combined artifact.

Frozen combined source R1: 84f0e56a7169710052dfb5ea0470d8c3f8dae644c9826f842b718d5145a58220, base/HEAD23eb638d. Focused Rust test run is executing (source frozen), final repository gate follows. Current completed checks: offlinealltargets PASS; fresh migration PASS; plugin types PASS; plugin133/133 PASS; guards14/14 PASS; CSS rebuilt. See CH-MAIN-B1-R1.evidence.json.

Focused merged Rust run PASS315/0FAIL/2explicit opt-in ignores in69.58s, covering exact private save/archive/publication refusal, lead repoint, permissions/library copy and competing skill revision capture (logs/main-focused-R1.txt). Final gate waits for complete B findings/correction batch; root preserves source freeze.

R1 independent CH-MAIN-B1-N1 CHANGES REQUIRED (F1 create blank choice promises native although new creation pins hosted model; F2 inherited docs update missing mandatory native_permissions). Complete findings received, source unfrozen; both corrected cohesively. Company model helper derives create/edit from existing AgentFields.agent_id; create says Company default and key prerequisite, edit retains native blank selection. Existing create/edit page assertions now discriminate both surfaces. API example includes native_permissions full plus supported choices; one exact documentation-to-production-decoder regression verifies actual full request, rather than field-text presence. No execution/auth/schema changes. Focused R2 includes page assertions, decoder, native-only resolution and actual default-desk creation/refusal.

Frozen correction source R2: e9d327b92aedd692b4fb7b42d38366ed3413d27eb9caf80c8f32f792b46f99dd, HEAD/base unchanged23eb638d; exclusions unchanged. R1 checks reused for unchanged contracts; R2 focused and final full gate pending.

Correction R2 compile FAIL: one newly added decoder test type was not reexported by its parent UI module. Retained main-corrections-R2.txt; qualified the actual route production type. Source frozen R3 a0aa542552ba808a17447d5c219eb6777ec00c9c0801550da8dc8329219b2554, identical to R2 except that type qualification. Focused rerun executing. Imported immutable input-phase evidence contains whitespace diagnostics; preserve original bytes, distinguish source/target checks from log formatting.

R3 correction focused PASS27/0FAIL/0ignored5.81s: actual decoder, distinct create/edit rendered choices, native-only resolution, model-less default-desk refusal and atomic default-desk creation/competing builders. Product/source/target whitespace PASS excluding only retained workflow log files; raw archive diagnostics retained. Final scripts/check.sh now executes on this frozen R3 source.

Normal full Rust suite complete4216PASS/2FAIL/26ignored1365.08s; retained normal-prefix log. Both failures reproduce exact accepted-feature failure payloads: sample.rs:153 inline-script hostile fixture scanner; MCP completed3of4 instead of4. Both triggering source files byte-identical to both immutable inputs (main-baseline-source-identity-R3.json); compare exact failure bodies main-baseline-failure-comparison-R3.json. These confirmed unrelated failures remain unchanged; whole suite is not green. Full stock2MiB gate running, no final acceptance yet.

Cargo all-target execution stopped at the two unrelated library failures, so the already-compiled src/main.rs test binary was listed and exercised separately behind the network guard:5PASS/0FAIL. No Cargo/database concurrency; these five argument/shutdown tests use no database. Full source-reachability gate8PASS. Stock-stack suite continues; no acceptance yet.

Final full gate COMPLETE (main-final-full-gate-R3.txt):15PASS/3FAIL. Both normal and unchanged2048KiB stock library runs4216PASS/2FAIL/26ignored,1365.08s/1383.61s, no overflow. Failures exactly match accepted input failure payloads (main-baseline-failure-final-R3.json); triggering files identical to both inputs. Raw patch whitespace fails only immutable input evidence, all331 offending files byte-identical to an input; product/target whitespace passes. Leave these unrelated failures unchanged. All format/offline/Clippy/migration/SQLx/CSS/SSE/deploy/transport/evidence/reachability gates PASS; binary5PASS separately. Full suite is not green; no unavailable required integration check. Final manifest records actual FAIL statuses, not waived PASS. No source changes after freeze. Final independent recheck/acceptance and authorized normal push remain.

Root ACCEPTS combined R3 artifact a0aa542552ba808a17447d5c219eb6777ec00c9c0801550da8dc8329219b2554 after independent CH-MAIN-B1-N3 / CH-MAIN-B1-R3-final PASS, both findings resolved, focused/final verification completed with confirmed unrelated failures retained. Freeze and both coordination authorities rechecked unchanged before acceptance. No missing required integration check. This accepts combined behavior; it does not claim whole-suite green or deployment. Authorized merge commit and normal main publication follow.

Final precommit checks:68 retained evidence manifests validated with --tracked-manifests --require-tracked; all changed-file identities match, accepted source digest unchanged after staging, no unmerged or unstaged paths, staged non-log whitespace clean. Main still103004a8 after fetch.
