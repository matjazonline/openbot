# Cloudroom harness p01

## Branch publication authorization — 2026-11-11

The user explicitly requested pushing the worked branch without merging to main. This supersedes earlier no-commit/no-push statements for this branch publication only; implementation remains paused and incomplete. Source content matches stopped checkpoint c7f1caa6465907c7b7f01a19bf9e0571a63704f810a2d18f3eac0a3c8545faea from base abac7038a08a355156b7680a01d2f79f3de756fd. Publish as WIP on codex/cloudroom-harness-t1gfnhz1 with retained phase evidence. No new implementation or full verification is claimed. Coordination revision2 updates publication authorization only; historical review notice identities remain revision1.

## Original requirements

User: "implement this missing parts so we can set them on agent in openbot, use $implement-astra-reviewed-2-2". Prior user requirement: "we need full access by default". Missing behavior identified: instructions/skills/MCP/permissions currently left to native provider configuration; Openbot sends only project/provider/model/reasoning/prompt.

## Identity and ownership

Workflow implement-astra-reviewed-2-2. Root/A /root uses prior session confirmation of Sol6.1/xhigh; no model override is claimed. B is /root/cloudroom_harness_reviewer, spawned no-history with requested Astra/high; it has completed the B1 architecture gate and will review the whole implementation. Host context threshold is not exposed; continue with defaults and durable checkpoints.

Worktree /private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree; branch codex/cloudroom-harness-t1gfnhz1; starting/review-base HEAD abac7038a08a355156b7680a01d2f79f3de756fd. Primary checkout was clean; preserve it and all other worktrees. New owned parent /private/tmp/openbot-cloudroom-harness-t1gfnhz1; pinned Cloudroom checkout under that parent at d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815. No shared DB/cache/credentials changed.

Authoritative coordination /private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/plan/cloudroom-harness/COORDINATION.md, revision1, writer/root, no active notices. Exact future snapshot exclusions: plan/cloudroom-harness/COORDINATION.md and plan/cloudroom-harness/phases/p01/docs. README and all implementation/deliverable docs remain included.

## Status

PAUSED at the user's explicit safe-checkpoint request, before the earlier2% threshold (last live3%). Source c7f1caa6465907c7b7f01a19bf9e0571a63704f810a2d18f3eac0a3c8545faea is uncommitted/unpushed and unreviewed partial B2. B1 historical acceptance and M2/r2 architecture acceptance remain retained; neither means current B2 source accepted. Permission/protocol slice and optional hosted-model publication are implemented with focused checks. Final optional-model checks PASS17 domain,37 DB/publication and35 workflow-publication; R2 offline all-target compile PASS. Current optional-model changes have NOT had Clippy, broader hosted runtime/UI integration or whole-batch review. Native agent permissions/selection/capture/delivery/core extension, actual staged script, B3 MCP and final full-suite gate remain unfinished. Owned PG stopped; data/worktrees/targets preserved. No active reviewers. Do not auto-resume.

## Acceptance and verification mapping (to finalize after product placement)

- CH01: Openbot authoring persists every configured harness field through create/update/read, with company-scoped references.
- CH02: Full permissions default is explicit at the native spawn boundary; selected lower modes remain effective.
- CH03: Native threads receive the selected instructions and skills without silent truncation or ambient cross-thread leakage.
- CH04: Selected MCP capabilities are actually callable, with company isolation, current credential authority, bounded IO and truthful failures.
- CH05: Admitted sessions freeze configuration; edits/restarts/reconciliation cannot widen capabilities or duplicate uncertain effects.
- CH06: Focused checks per cohesive batch, independent whole-source/evidence review, and final repository-required full gate before completion.

No commit/push/production deployment requested for this new implementation.


## Clarification and cohesive batches

User clarified: "the local harness like claude-code can have scripts inside the skill directory so this should be taken into account". Treat complete skill trees, binary assets, relative script imports and executable intent as requirements. Use the existing agent as the source for agent-level harness settings; machine/project/provider/model selections remain on local profiles. No explicit UI-placement alternative was selected.

Batch B1 is the stable skill-directory foundation (domain/persistence/frozen-publication/authoring/API/archive tests), gated by CH-P01-M1/r1 SHA e24182f35493e0a275faa79a0f57479d2f7893907728bee3d379d6c2b4f2fc56. Batch B2 completes agent-to-Cloudroom configuration, directory deployment, explicit full-permission default and actual MCP execution, with one gate for its cross-system ownership/authority/recovery decisions. Accept B1 before dependent B2 work. Final full-suite gate occurs once for the entire feature; focused foundation checks do not claim native delivery or script execution.


B1 architecture AG1 returned actionable findings F1(copy collision frontmatter identity) and F2(asset JSON body capacity), retained by B. Root revised both together in CH-P01-M1/r2 SHA aabc37ecdcb5960c066b65c58b001f3020c9852ff4f8ebb189d45194eaa61853. No dependent source edits before recheck. New JSON capacity has an enduring75% maximum-valid payload guard and actual413 boundary tests; copied skill identity rewrites only the copied header and preserves body/assets/modes through direct and indirect library copying. Owned PG now running on63451; resources.json records separate database and paths, no shared state reset.


## B1 implementation checkpoint

Architecture CH-P01-M1/r2 accepted by independent B CH-P01-AG1-N2 and fresh root acceptance (logs/root-CH-P01-M1-r2-acceptance.json). No new runtime/MCP architecture is accepted yet.

Implemented in progress: domain skill_bundle.rs with regular binary files, portable paths, canonical base64, executable intent, byte/count/prefix/case collision guards, bounded Saphyr frontmatter and explicit renamed-copy metadata. Skill.files, SkillWrite optional replace/preserve commands, SkillDb SELECT/INSERT/UPDATE/library copy, frozen SkillSnapshot.description/files, publication capture and validation. Init schema valid_skill_files function and files JSONB constraint; new fresh PG migration passed before latest COLLATE C clarification (reset owned DB before next DB tests). Plan README is registered in startup no-compat notes. ZIP2.4.2 dependency already locked; Cargo.lock adds only the direct root dependency.

Archive module imports/exports root or one wrapper SKILL.md, nested scripts/binary assets and executable modes, rejects unsafe entries/decompression/count overflow, deterministic ZIP output. Canonical generated text-only frontmatter is inverted on import so a maximum16KiB text skill has byte-exact native export/import without increasing its body bound; opaque extra header fields retain exact source under the original source bound. No runtime code executes on import.

HTTP adds owner/operator ZIP import/replace/download and asset metadata, company JSON CRUD and existing operator JSON assets at explicit2MiB capacity with75% maximum-canonical headroom guard. HTML metadata form limit unchanged. UI upload spinner/disabled submit, file metadata loading and download controls use existing theme tokens. Caller extractor was generalized from concrete AppState to the same trusted UserUseCases/AppConfig FromRef sources to exercise production routes under a fixture state; algorithm and verified-session requirement unchanged. Include auth suite and broad offline compilation in focused verification for that shared refactor.

Initial migrate PASS. Exploratory all-target check R1 failed missing Self.files in JSON conversion; R2 failed only new handlers' tuple AuthenticatedUser pattern (actual extractor is named struct). Those were corrected; results retained in logs, not accepted evidence. Newly authored domain/archive/persistence/real HTTP tests have not yet run. HTTP import/guards test needs decomposition if rustfmt pushes it past repository function-size guidance. Source files carry explicit new fields in all discovered literals. Next: finish compile, actual focused checks, indirect library-agent-copy bundle regression, native browser import/progress evidence, SQLx prepare, B1 whole review/finding corrections and acceptance before B2 dependent code.

Owned resources: /private/tmp/openbot-cloudroom-harness-t1gfnhz1; private CoW target clone, PG18 localhost63451, database openbot_cloudroom_harness_t1gfnhz1, max_connections100/stat settings in resources.json. Node24.21.0 reused immutable binary under previous Doop publication parent; plugin npm ci PASS. Pinned Cloudroom d8b3 source + exactly2 existing patches hash-verified, pnpm10.34.9 install PASS (normal missing unbuilt dist bin warnings); log cloudroom-baseline-install.txt. Native service/provider execution not started. No published commits, no production/shared reset, primary main preserved.

B2 investigation notes (not gated design): Cloudroom public SDK supports explicit permissionMode full/auto/accept-edits and synchronous bb.agents.configure instructions/tools/manifest skills. Dynamic instructions truncate at4096characters; never silently truncate an agent definition. Skills can include executable directories; native deployment must preserve full files. Existing manifest roots are static but discovered per resolution; an immutable content-addressed private cache selected per thread needs ownership proof and collision policy for multiple revisions. First spawn may resolve configure before plugin journal maps thread ID; a message.dispatch wait/positive native receipt gate may be needed. Metadata alone is untrusted per public SDK. Existing company MCP credentials must retain current authority, and retries must not duplicate unknown effects. These consequential cross-system decisions require one new internal M2 gate before dependent edits. Do not mistake B1 storage for completion of agent/Cloudroom/MCP setup.


## Current product clarification and verification

User explicitly chose existing company MCP connections for native Cloudroom agents. Harness-local MCP server configuration is outside this task. Credentials remain managed by Openbot. B2 still needs its internal ownership/authority/recovery decision gate before source implementation.

B1: fresh owned migration and offline all-target compilation pass in focused R1; Rust focused test binary compiling. CSS regeneration followed by check PASS (B1-css-build-R1.txt, B1-css-R2.txt); initial stale CSS failure retained in B1-css-R1.txt. Full suite/native script delivery are not yet exercised. Documentation now describes directory import/export and actual enforced bounds.


## B1 acceptance mapping and contract impact

| Criterion | Production path and decisive assertion | Focused check | Status |
| --- | --- | --- | --- |
| B1-AC1 complete supporting files | SkillFile path/base64/count/byte checks; real SkillWrite/persistence update preserves omitted assets and clears explicit empty; company scope is enforced | domain skill_bundle tests; skill_bundle_persistence_create_update_preserve_and_scope_round_trip; SQL/domain equivalence matrix | PASS R6; pending B correction recheck |
| B1-AC2 native archive round trip | Stored/deflated ZIP decoder/encoder preserve binary bytes, executable intent, wrapper-relative scripts and canonical source; reject traversal/case/prefix/symlink/special/count/decompression overflow before filesystem extraction | skill_archive tests, including maximum plaintext16KiB, max512KiB/64files, footer preflight | PASS R6, including suffix/root/Unicode/duplicate variants; pending B recheck |
| B1-AC3 copy and immutable publication | Direct and indirect library copies get independent assets and matching renamed frontmatter; editing shared file bytes/mode does not mutate an earlier captured agent definition | skill_bundle_library_copy_renames_only_new_identity_and_keeps_assets_independent; picking_a_library_agent_copies_its_skills_with_the_agent_and_channel; agent_publication::tests::skills | PASS R6, including real competing source editor and both copy paths |
| B1-AC4 equivalent authorized interfaces and body bounds | Real signed-session company/operator JSON and multipart routes; maximum-valid payload fits budget; exact body limit accepted, limit+1 returns413 before write; unauthorized tenant/operator/CSRF paths cannot mutate/read bundles | skill_bundle_http*; skill_bundle_json_capacity_retains_twenty_five_percent_headroom; auth tests | PASS R6; historical source/fixture failures retained |
| B1-AC5 usable import state | Actual Rust markup with shipped JS/CSS/HTMX/CSP; empty required file starts no request; exact ZIP bytes arrive, both submits disable, spinner/progress and success/error restore in light+dark themes | dump_skill_bundle_browser_fixtures plus scripts/tests/skill-bundle-browser.mjs | PASS R6 with fresh Rust fixture; selector failure retained |
| B1-AC6 complete integration, no new execution permission | Frozen SkillSnapshot includes description/files and hash; hosted runtime still reads only instruction text; plan reset note listed at startup; all Rust source modules reachable | application::agent tests, source_modules, offline alltargets, Clippy, migration/SQLx, CSS | PASS focused R6/current freeze; native script use explicitly B2 NOT EXERCISED |

Changed contracts and consumers:
- Skill.files is required entity/snapshot data. SkillWrite.files is a command option (None preserves, explicit empty clears), not a legacy decoder. Init schema adds bounded JSON constraint; database reset required. Direct/indirect copies, onboarding/provisioning fixtures, agent publication/capture, resolution tests and HTTP serializers carry the field.
- SKILL.md stays one canonical instruction source. Ordinary plain text exports generated metadata; import inverts only the exact generated separator/header. Opaque YAML metadata/body remains source. Copy collision rewrites the copied name only; original text/assets/modes remain independent. Native filesystem execution is deferred to B2.
- SkillSummary lists/pickers expose metadata and file_count rather than instructions/assets. Persistence list projections, skill page cursors, UI agent pickers and company/library views use it. Detail and publication retain full bundles. JSON max/list tests distinguish this boundary.
- Caller extractor now accepts existing FromRef UserUseCases/AppConfig instead of concrete AppState. Current account/verified session resolution and caller_for logic are unchanged; auth suite R1 passes, owner/operator real-route tests exercise the new generic state.
- ZIP import company/operator sibling routes authorize before parsing upload; JSON asset routes use a dedicated2MiB limit with an enduring75% largest-canonical guard, preserving the original HTML metadata bound. Hosted models receive no filesystem/process tool from storing files.
- Tests were added; existing test names are retained. Only helper extraction occurred, with no removed or renamed test obligations.

R1 focused run retained sixteen commands: format, offline alltargets, owned-only database reset, fresh migration, domain/HTTP/bundle tests, archive tests, skill persistence, publication skill tests, usecases, auth, agent contracts, source_modules, browser fixture, browser, Clippy and SQLx regeneration. Failures: generated-header separator inversion (one source defect causes two failures), company stranger privacy expected403 instead of existing404 (fixture assertion), and Playwright generic button locator also matched input[type=file] (fixture selector). No production authorization semantics were changed. Corrected source/assertions/selectors together; R2 reruns affected behavior plus shared checks. SQLx prepare regenerates successfully, leaving no metadata diff for these dynamic queries.


## CH-P01-B1-R1 frozen implementation review

Starting commit, review base and current HEAD are separately abac7038a08a355156b7680a01d2f79f3de756fd (no commits). Artifact cd38325289a55b05843071e92738e5bf97fb8bfeeb06d5a4e4a7afec1e74ebad; exact exclusions unchanged: plan/cloudroom-harness/COORDINATION.md and plan/cloudroom-harness/phases/p01/docs. Entire implementation worktree now frozen for B. Coordination revision1 checked, no notices; pinned copy logs/CH-P01-B1-R1-coordination.md.

Focused R3 PASS:73 skill-related Rust tests, explicit ignored fixture generator separately PASS, real-browser protocol PASS, offline alltargets, Clippy and SQLx check PASS. Fresh migration, shared auth/agent unit tests, source reachability and actual SQLx regeneration passed earlier with affected production contracts unchanged. Format check, regenerated CSS check, patch whitespace and integration docs PASS. Evidence: logs/CH-P01-B1-R1.evidence.json. R1/R2 failures remain in original logs; the final HTTP guard tests assert the existing404 privacy contract on both company-stranger and nonoperator surfaces, plus a new test proving members cannot read and admins may read but both cannot import/replace owner-only directories. No production authorization behavior changed to obtain these passes.

B1-AC1 through B1-AC6 implementation/verification complete pending independent review. B1 does not establish native delivery/script execution, permissions or MCP. No full-suite claim; final full suite remains required after B2. Pinned Cloudroom baseline server/host-daemon build PASS, independent of B1 acceptance; no native provider session started.


## CH-P01-B1 correction decisions

Independent B CH-P01-B1-N1 returned F1/F2 (P2), with actual reproduction retained. F1 correction captures required_tools alongside the initial full Skill row and binds every content value from that one snapshot during all slug retries. INSERT still rechecks the source ID/global-library predicate, preserving tenant/deletion guards. No additional locks, ownership protocol or snapshot/permission schema is introduced; this fulfills M1/r2 coherence rather than changing its contract. Regression races the actual source editor against both standalone and indirect agent copying, holding a real conflicting slug until a database lock wait is observed. It checks instructions, script bytes and required tool names all come from revision A while source B is saved.

F2 correction selects the actual SKILL.md entrypoint once (root first, otherwise one wrapper) and uses its exact path for the instruction bound. Supporting reference-SKILL.md and references/SKILL.md retain the asset bound, including in wrapped archives. During this correction source inspection also established ZIP2.4.2 silently deduplicates exact filenames into its IndexMap: root now validates raw bounded directory names/count before constructing the reader, with a regression proving the library would otherwise hide the duplicate. This closes the same unique-entrypoint obligation; no new archive variant or raised bound.

R4 focused correction checks in progress: all skill callers/new race+archive variants, source reachability for the new test module, regenerated browser fixture/protocol, offline alltargets, Clippy, actual SQLx regeneration. Historical R1 review digest remains frozen only as history; implementation source reopened after B returned its complete findings. No B2 dependent source work has begun.


## CH-P01-B1-R2 correction recheck freeze

Artifact 4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13, phase start/review-base/HEAD separately abac7038a08a355156b7680a01d2f79f3de756fd. Exact exclusions unchanged. Coordination revision1/no notices rechecked and pinned logs/CH-P01-B1-R2-coordination.md. Evidence manifest logs/CH-P01-B1-R2.evidence.json.

F1/F2 implemented and verified pending independent B recheck. R6 all skill callers77PASS +1 opt-in generator separately PASS; actual browser PASS, offlinealltargets PASS, Clippy PASS. Source reachability with the new concurrency module, actual SQLx regeneration and SQLx check passed in R4/R5 with schema/queries/module paths unchanged afterward. Current freeze format/CSS/whitespace/integration docs PASS; Graft rebuilt. Native and full-suite obligations remain unexercised.

F2 affected-variant follow-up: actual ZIP2.4.2 Unicode Path metadata can alias distinct raw names and silently collapse the reader index. Root now compares bounded raw directory count to reader count as well as validating raw names/count first. Regression constructs actual Unicode Path metadata and proves reader length1 versus raw count2, then production decode rejects it. R5's new test initially failed because ZipWriter validates Unicode metadata before a filename exists; R6 corrects only the input fixture construction by tagging the opaque field after finalization. Production guard remained unchanged; all77 tests pass. Retained R5 failure output is not discarded.

User requests handoff at a safe checkpoint and stopping at2% weekly usage remaining. Read-only direct Codex app-server account/rateLimits/read worked (two proxy attempts timed out); at2026-11-10T20:50:48Z it reported5% remaining. Helper /private/tmp/openbot-cloudroom-harness-t1gfnhz1/weekly_remaining_direct.py performs read-only checks, identifies the10080-minute window, retains sanitized numeric metadata and exits2 at/below threshold. It starts/joins only its own stdio server, creates no task/model turn, and never prints credentials. Check before further batches and while review is active; stop/interrupt B if threshold is reached.


## Root B1 acceptance / safe handoff checkpoint

B1 accepted in phase after fresh source digest, coordination and evidence validation: artifact 4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13, B note CH-P01-B1-N2, F1/F2 fixed. Root record logs/root-B1-R2-acceptance.json. No source commits/pushes or integration.77 focused skill tests plus separately exercised generator/browser; static/DB/schema checks as mapped. Native B2 and final full verification still outstanding. Handoff is being saved durably outside temporary storage at /Users/mac02/dev/openbot-review-records/cloudroom-harness-20261110/HANDOFF.md. User stop condition remains2% weekly remaining; fresh direct read at21:01:07UTC reported5%. Continue authorized work while above threshold; do not call the whole task complete.


## CH-P01-M2/r1 native delivery architecture gate

Author proposed stable split: B2 native settings/capture/delivery/explicit permissions and real native script execution; B3 actual existing-company MCP with reviewed approvals/receipts. Full task requires both. Decision and criterion mapping: logs/CH-P01-M2-r1.md. B2 temporarily refuses nonempty MCP tool bindings instead of silently omitting them. No dependent source edits before B architecture PASS. Source remains accepted B1 artifact4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13. Coordination revision1/no notices rechecked. User stop threshold2% remaining; live read21:42:43UTC remains5%.

B2 independent preparation: existing plugin npm test PASS90/90 under real Node24.21.0, logs/B2-plugin-baseline.{json,txt}. This is baseline evidence only, no native delivery claim. Actual pinned originKind nonnull is currently assumed to be a fork requiring sourceThreadId in thread-create.ts762–766,957ff; author sent this contradiction to B for the complete M2 finding set. No dependent source edits.

Native test preparation PASS: actual installed Claude Code CLI, isolated home/config and scripted local Messages endpoint with dummy key, full explicit bypassPermissions, no paid inference. It received the long instruction tail, executed a0755 Bash script and returned the SHA256 of an adjacent256-byte binary asset. logs/native-fixture-baseline/{probe.py,result.json,native-stdout.jsonl}. This is standalone fixture feasibility only: no Cloudroom host-daemon staging/Openbot delivery or model reasoning is proved.


## CH-P01-M2/r2 correction recheck

B returned CH-P01-AG2-N1: F1/F2 P2. Both corrected together in logs/CH-P01-M2-r2.md, SHA08357154aecaa1b044a42688a0f25230eff0e7ca2f9ca56d7bb7989889be9403. F1 separates immutable required owner from fork lineage; F2 adds explicit bounded required-tree registry/source/transfer preserving .bb-registry-skill.json and all B1 files. No dependent source edits. Standalone fixture portability rerun PASS from a fresh owned directory, logs/native-fixture-baseline/portable-recheck.json. Snapshot R1 failed on macOS realpath -m; R2 with GNU coreutils PATH reproduces unchanged4cd source (logs/M2-preparation-R2.snapshot.txt). Coordination revision1/no notices.

Root accepted architecture CH-P01-M2/r2 after actual B completion/PASS CH-P01-AG2-N2, fresh source/decision/coordination checks; logs/root-M2-r2-acceptance.json. B2 source implementation now authorized under that decision. First portion: explicit permission enum/catalog/UI/native spawn/send with focused Rust/Node/HTTP checks; agent ownership/ref/required core delivery follows within the same B2 batch. No partial B2 acceptance or completion claim.

B2 permission portion: explicit full/auto/accept_edits modes in domain/catalog/frozen schema/profile form and native spawn/send. Catalog/preflight intersect actual public SDK executionOptions permissionCeiling, host maxPermissionMode and provider-supported modes; changed native mode refuses follow-up before SDK intent. R1 offline all-target check PASS; first Node typecheck caught incorrect SDK config field, corrected to actual public executionOptions. R2 typecheck and102 Node tests PASS. Daemon restart interrupted R2 during Rust test compilation; no test result was produced and old driver/owned target processes are gone. Resume only unfinished Rust/DB/HTTP checks as R3. No B2 implementation acceptance; M2/r2 architecture is accepted.

R3 focused results:18 application/domain/wire tests PASS;56 persistence tests PASS,1 pre-existing ignored. HTTP filename-based filter ui_local_agents_tests discovered zero tests and is NOT HTTP verification; correct live function prefix is cloudroom_management_ui. Add/check explicit UI permission roundtrip and rerun that actual filter. R2 Node102/102 remains PASS. Daemon-recovery repetition was limited to unfinished checks.


## B2 permission/protocol checkpoint

Source artifact6c782bc816a757c5790ebc6a639529d1fdae0bd9df63f21a51fe5b43a5ec736f; unreviewed partial B2, no source acceptance/push/integration. M2/r2 architecture only is accepted. Focused evidence: R5 Node typecheck102 tests; R3 domain/wire18; R4 real HTTP4; R5 actual connection19 and persistence56(one ignored); R6 explicit protocol/chunk test1 and all-target offline Clippy; R7 actual workflow tests from discovered module path, plus whitespace and graph build. R5's guessed workflow filter selected zero tests and is NOT coverage; R6 discovery established the real workflow::tests::local_agent_tests path, exercised in R7. All results retained, including failures/empty filters/daemon interruption; final full verification still outstanding. Native test feasibility uses real CLI + scripted local API only, not Cloudroom delivery.


## User-requested stop / optional-model work in progress

User now explicitly requests stopping at a safe checkpoint and writing a handoff; this supersedes continuing until the earlier2% threshold. No further feature steps. Optional published models/nullable summaries and explicit hosted-use guards implemented under M2/r2; R1 compile exposed typed consumers, corrected; R2 offline all-target compilation PASS. R3 agent-domain16 PASS/1 FAIL was an old expectation that None must fail; configured hosted models remain required to be valid, while the new separately named native-only roundtrip test covers None. Correct that old assertion to reject an empty configured model, then finish only previously planned focused checks as R4. DB test publishes with no company hosted connections and rejects hosted selection; not exercised yet. Preserve unreviewed source and exact results, archive a fresh checkpoint and stop.

Final stop record: logs/stop-status.json. All queued checks finished; optional-model R4 PASS17/37/35, including real no-company-hosted-connection publication and explicit hosted-selection refusal. The old None-is-invalid assertion was replaced by invalid-configured-model rejection; the new named native-only test covers None. No removed test names. Source/records will be retained in durable checkpoint-03 with restoration proof. Restart only owned PG with explicit data directory when resuming.
