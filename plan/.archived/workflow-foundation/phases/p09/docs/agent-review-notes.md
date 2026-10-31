# P09 agent review notes

> Review artifact (excluded from the frozen snapshot). Entries are appended; originals are preserved.

## WF-P09-R001 — B1a batch review: CHANGES REQUIRED

### Identity

- Date 2026-10-24; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5, effort xhigh as
  assigned; fresh context, no agents spawned); phase `p09`; worktree `/Users/mac02/dev/openbot`,
  branch `workflows`. Records: task `TASK.md`, review `REVIEW.md`, findings
  `agent-review-findings.md`.
- Stage: batch review of B1a (library, editor, catalogue, versions, publish); first review of the
  phase. Gated decisions: none (no architecture gate held; B agrees one was not needed — see
  "Assumptions challenged").
- Phase start / review base: `da86871` + uncommitted p08 tree, digest `00ec454f…6829`
  (`logs/start-snapshot.txt`). HEAD `da868710d3b235d49856e3e481f8f3735f6b3e56`. Reviewed artifact
  digest `62a6c35e187548f9e6fbd444c9482f4b3e378764cef3dd2dc0a19f561cf84e50`, exclusions exactly
  `plan/workflow-foundation/phases/p09/docs plan/workflow-foundation/phases/p08/docs
  plan/workflow-foundation/COORDINATION.md marketing website`; confirmed at start and rechecked
  before returning (unchanged).
- Coordination: authoritative `plan/workflow-foundation/COORDINATION.md` revision 11, sha256
  `3e8501b4660acb3b8d09530a81b748102448fb01ce92efe5111d91c2178d62f5`, byte-identical to the
  immutable copy `logs/coordination-r11.md`; rechecked before returning (unchanged). Applicable
  notices: WF-C002 r1, WF-C003 r1, WF-C005 r1 (all scheduled for B1b/B2/B3/B4; nothing in B1a
  touches them), WF-C004 r2 (no Postgres pool added in B1a — the new reads use `self.pool()`).
  Plan-wide instructions: none beyond AGENTS/README. No changed applicable requirement.
- Delta integrity: recomputed the p09-only delta independently (31 files, equal to
  `logs/b1a-files.txt`); all 13 `logs/start-base/` copies match `logs/start-filehashes.txt`;
  regenerated the diff myself — identical (+/- line sets) to `logs/b1a.diff`. Preserved p08 work
  reviewed only as callers/contracts.

### Coverage and evidence

- Requirements derived from plan 09 "Authoring and setup" (library, editor, catalogue, graph,
  versions/compare/publish, MCP selection, capability display per 06-D5, optimistic revisions,
  unsaved source, authorization sentence) and Acceptance 4 and 6 as they apply; EXPANSION 09.1;
  README; 06 §Skills with 06-D5; TASK criteria/batches/DV-P09-1..6; all seven AGENTS.md files.
- Read in full: every new file (application `authoring/*` and tests, `reads.rs` port additions,
  `persistence/workflow/authoring_reads.rs`, `routes/workflows/ui/{mod,authoring,authoring_tests}.rs`,
  `pages/workflow_authoring/{mod,editor,preview,graph,versions,catalogue}.rs`) and every modified
  hunk (definitions.rs refactor, bindings `Visibility`, workflows router merge, tests helpers,
  persistence `reads.rs` visibility, icon/mailbox/pages mod, registry help/tests, infra
  accessors, plan 09 note + `NO_BACKWARD_COMPATIBILITY_NOTES`).
- Callers/contracts read: `DefinitionService::{save,validate,publish}` + `check_draft` +
  `DraftContent::check`; `ProductionDefinitions` composition (`infra/workflow.rs` `definitions`,
  shared by test and production engines); MCP/HTTP publication capture, `status_on`,
  `policy_on`; `ToolSnapshot`/`ToolContract`/`AgentSnapshot`; operation/connection table
  constraints; `HttpOrigin::parse` (no userinfo/path); `agent_universe` (06-D5 `/params` enum
  agents); `agent_publication::{derive_on, verify_on}` (slug resolution — DV-P09-5 verified);
  `security::browser_security` (global Origin/Referer CSRF, CSP `script-src 'self'`,
  `style-src 'unsafe-inline'`); `load_managed_company`; `REQUEST_ERROR_SCRIPT`,
  `BUTTON_BUSY_SCRIPT`; `escape_html_text/attr`; `form_error_banner`; `UNHANDLED_STEP_KINDS` and
  the handler-coverage test (DV-P09-2 note accuracy spot-checked against `MAX_HEADERS`,
  `MAX_PATH`, `MAX_TEXT`, `MAX_MESSAGE_DESTINATIONS`).
- Points of scrutiny and conclusions:
  - No parallel business logic: the five `definitions::*_draft` functions are the JSON handlers'
    bodies moved verbatim; only error precedence between `actor()` and body parsing moved
    (`actor` is evaluated first now), with no behavioural effect on valid requests. The keyed
    publish version still hashes the same `{"http","mcp"}` JSON. JSON route tests pass.
  - Tenant scoping: every handler runs `load_managed_company` first; every new read binds
    `company_id`; foreign workflow/version ids end `NotFound` by inspection (save: `Conflict` then
    `NotFound`). Tests cover the victim-scope path and `pane` with own scope only (F006).
  - Escaping: every text/attribute sink checked, including SVG text/title, `aria-label`, select
    labels, frozen-contract JSON, diagnostics, banners with command messages, schedule/channel
    names, field names built from slot/tool (grammar-restricted and escaped); `hx-confirm` strings
    are static. Hostile-content test covers list/editor/preview. No defect found.
  - CSRF: all five POST routes covered by the global middleware; the test drives exactly
    `WRITE_ROUTES` through it. No defect.
  - Optimistic revisions: save sends the opened revision; conflict keeps typed text, shows a diff
    and arms an explicit overwrite (documented in the banner) — acceptable. Failure paths with
    wrong messages: F008. Unsaved text lost through other pane-replacing controls: F003; textarea
    newline: F005.
  - Publish exactness: the preview's form sends the validated revision and the validated
    selections as hidden fields, separate from the editor's selects — correct. Dependencies are
    re-captured at publish (F010, optional).
  - Credentials: labels are MCP slug / HTTP origin; snapshots hold contract and policy only;
    probe confirmed no endpoint text in the editor. No defect.
  - Bounds: diff table ≤ (a+1)(b+1) u32 cells with a·b ≤ 1 M (≈ 4–6 MB worst case), sources
    ≤ 256 KiB; usage ≤ 200 bindings × 21 schedules; graph drawing ≤ 60 nodes; options cut at 200
    company-wide — correctness defect F002.
  - Capability display (06-D5): literal agents correct; parameter-selected agents not shown
    (F001).
  - Progress feedback / theme tokens: forms disable submitters and show spinners; SVG uses
    `var(--color-*)`; class scan 0 missing. Misleading/stale indicators: F004. Browser, both
    themes and keyboard not verified (F012, deferred by plan).
  - Registry notes (DV-P09-2): accurate where checked; memory is excluded from the catalogue.
- Checks actually run (env `SQLX_OFFLINE=true CARGO_INCREMENTAL=0
  DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09`, phase cluster only):
  - `cargo test --lib -- routes::workflows application::workflow::authoring
    application::workflow::registry pages::tests` → 188 passed, 0 failed, 1 ignored
    (`logs/r001-b-focused-1.log`).
  - `cargo test --lib -- adapters::http` (shell/rail and JS bundle are shared by every `/ui`
    page) → 324 passed, 0 failed, 1 ignored (`logs/r001-b-http.log`).
  - `cargo fmt --check` → clean (read-only). `python3 -I logs/css-class-check.py
    src/adapters/http/pages/workflow_authoring/*.rs` → 0 missing; spot-checked dynamic classes.
  - Probes on a scratch copy of the artifact (identical to it except the probe-extended test file;
    own target dir; `own_database` per test on 55490): `logs/r001-b-probes.log` — confirmed F001,
    F002, F005 markup, and F007's refusal; showed the MCP UI happy path works with a real
    connection.
- Inspected, not run: the author's clippy log (`logs/b1a-clippy.log`, clean) and focused logs.
- Unavailable/not run: browser verification in both themes, computed styles, keyboard walk-through,
  `npm run check:css` (no `node_modules`), `sqlx-cli` (not needed: runtime queries only, no schema
  change), full suite and stack budget (not required for a batch review).
- Reviewed lightly: SVG geometry/aesthetics (`centre`, curve paths) beyond correctness and
  escaping; diff folding presentation; template copy beyond keyed-identity and refusal paths
  (the template command itself is p08's, already reviewed).

### Assumptions challenged

- "No architecture gate needed": agreed — no schema, transaction, concurrency or authorization
  model change; `AuthoringReads` is a new cohesive read port with no default methods, implemented
  in persistence; UI routes call the existing commands.
- "Publish sends exactly what was validated": holds for revision and selections; frozen
  dependencies can still differ (F010).
- "A capture NotFound is an actionable refusal" (DV-P09-6): holds; the `Conflict` case was missed
  (F008).
- The author's MCP test was read as proving the happy path; it proves a refusal (F007).
- Out-of-order responses: read controls use `hx-sync` replace and writes disable their
  submitters; a write's response landing after a later navigation could still repaint the pane —
  noted, not filed (not a live protocol; same pattern as existing workspaces).

### Outcome

- **Changes required.** Actionable: WF-P09-F001 (Medium), F002 (Medium), F003 (Medium), F004
  (Low), F005 (Low), F006 (Low, verification), F007 (Low, verification), F008 (Low), F009 (Low).
  Optional: F010, F011. Tracked verification gap: F012 (deferred by plan to after B1b; must be
  closed before phase acceptance).
- Unresolved questions: none requiring a user decision.
- A correction recheck should cover each finding's delta, rerun the focused set above, and reuse
  the rest of this review where the code is unchanged.

### Process and instruction feedback

- Batch size: large but reviewable in one pass; the pure application pieces and pages separate
  cleanly. Not too large.
- Author records were useful: DV-P09-1..6 and the exact diff/base tooling made the delta
  verifiable; the B1a log honestly lists what was not run.
- Misleading evidence: the MCP test's comment ("whatever capture decides is shown") hid that the
  fixture never reaches a valid preview. Candidate instruction (scope: implement-opus-reviewed
  author checks): "a test that tolerates several outcomes must assert which one its fixture
  produces; a fixture for a provider-backed path reuses the subsystem's test support rather than
  inserting rows directly".
- Candidate for `pages/AGENTS.md`: "a pane holding an editable form treats every control that
  replaces it as navigation away from unsaved input" (F003) and "a form with two submit buttons
  needs submitter-scoped progress, not `[.htmx-request_&]`" (F004).
- Candidate for `src/adapters/http/AGENTS.md` clarification: "another tenant's id" includes the
  attacker's own valid scope combined with the victim's object id (F006).
- Process note: the scratch probe build ran from the scratch directory (not from the repository
  root as the assignment's shell rule says) so that it could not touch the project tree or its
  `target/`; recorded here for transparency.

## WF-P09-R002 — B1a correction recheck: CHANGES REQUIRED (Low only)

### Identity

- Date 2026-10-24; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session as R001); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`.
  Stage: correction recheck of B1a by finding ID (F001–F012). Gated decisions: none.
- HEAD `da868710d3b235d49856e3e481f8f3735f6b3e56`; reviewed digest
  `b4dc745de5c69e461189ae0c64ad0d38ec3de67e30eeacc7550c9675e9e2de3c` (same exact exclusions as
  R001; manifest `logs/b1a-snapshot-r2.txt`). Confirmed at start and before returning.
- Coordination: COORDINATION.md r11, sha256 `3e8501b4…62f5`, unchanged (start and end). No
  applicable notice changed.
- Delta integrity: the files changed since R001 are exactly the 7 in `logs/b1a-r2-files.txt`.
  B checked them three ways: `delta.py` against `logs/b1a-filehashes.txt` (whose entries B
  spot-verified against its own R001 copy), a `diff -rq` of `src`/`migrations`/`assets`/
  Cargo files against B's independent R001 scratch copy, and identical manifest path sets. B's
  own diff equals `logs/b1a-r2.diff` as +/- line sets.

### Coverage and evidence

- Read the whole correction diff (reads port, persistence options query and row structs, route
  handlers, editor/preview/catalogue pages, `WORKFLOW_EDITOR_SCRIPT`, nine new or changed
  tests) against each finding and its callers. Additional contracts read: htmx 2.0.4's
  `beforeRequest` cancel path, `BUTTON_BUSY_SCRIPT` cleanup, script concatenation order,
  `parse_workflow` (no step bound), `validate` (`MAX_GRAPH_NODES` 1024),
  `DependencySnapshots` / `children()` types for digest determinism, serde_json features.
- Checks run: `cargo test --lib -- adapters::http application::workflow::authoring
  application::workflow::registry` → 373 passed, 0 failed, 1 ignored
  (`logs/r002-b-focused.log`; the author's count is reproduced). `cargo fmt --check` clean.
  Class scan: 0 missing. Probes on a scratch copy equal to the R002 `src` plus probes
  (`logs/r002-b-probes.log`): `probe_b2_many_literal_tools_and_the_editor` (F013) and
  `probe_b2_conflict_echo_is_not_marked_dirty` (F003 residual).
- Reused from R001: everything outside the 7 files (unchanged), including the R001 HTTP-suite
  run, escaping/CSRF/tenancy conclusions for unchanged routes, and the registry review.
- Not verifiable here: the client-side behaviour of the dirty guard, status text, reference
  failure state and F014 (JavaScript; established by reading code and the htmx source). These
  belong to F012's browser pass.

### Outcome

- Fixed: F001, F002, F004, F005, F006, F007, F008, F009; adopted optional F010, F011 verified.
- Remaining: **WF-P09-F003** (open, lowered to Low): server-echoed unsaved text is not marked
  dirty.
- New, from the corrections: **WF-P09-F013** (Low), more than 256 tool names makes the editor
  unrenderable; a save commits but answers 400. **WF-P09-F014** (Low), cancelling the discard
  prompt from New leaves it disabled and spinning. **WF-P09-F016** (Low), a parameter step lists
  every frozen agent instead of its enum.
- Optional: **WF-P09-F015** (split `ui/authoring.rs` 987 lines, `editor_body` ~95 lines, tests
  1573 lines, before B1b).
- Tracked: F012 open by plan.
- Not acceptance-ready until the Low actionable findings are corrected (one round) and rechecked.

### Process and instruction feedback

- The corrections were thorough and well-tested on the server side. Each remaining defect sits
  where a fix crossed into another owner's contract: the shared busy script (F014), the read
  bound vs page rendering (F013), the server-rendered state vs the client guard (F003), and the
  union vs per-step agent set (F016).
- Candidate instruction for `pages/AGENTS.md` (from F014): "a handler that cancels
  `htmx:beforeRequest` must run before the shared busy-button handler, which cleans up only on
  `htmx:afterRequest`". Candidate author check (from F003/F013): "when a fix adds client state
  or a refusal, test the server-rendered states that must carry it and the render paths the
  refusal reaches".
- Process note: as in R001, the scratch probe build ran from the scratch directory.

## WF-P09-R003 — B1a correction recheck: PASS

### Identity

- Date 2026-10-24; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session as R001/R002); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`.
  Stage: correction recheck by ID (F003 residual, F013, F014, F016; adopted optional F015;
  tracked F012). Gated decisions: none.
- HEAD `da868710d3b235d49856e3e481f8f3735f6b3e56`; digest
  `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db` (exclusions unchanged;
  manifest `logs/b1a-snapshot-r3.txt`). Confirmed at start and before returning.
- Coordination: COORDINATION.md r11, sha256 `3e8501b4…62f5`, unchanged at start and end;
  notices WF-C002 r1, WF-C003 r1, WF-C004 r2, WF-C005 r1 acknowledged; none affected by B1a.
- Delta integrity: B confirmed the 12 changed paths three ways: `delta.py` against
  `logs/b1a-r2-filehashes.txt` (entries spot-verified against B's own R002 copy), a `diff -rq`
  against that copy, and the manifest path sets. Real changes are `pages/workflow_authoring/
  {editor,preview}.rs`; the rest is the F015 move. `tree-b1a-r2` bases equal B's copy.

### Coverage and evidence

- Read both page diffs in full. Read `editor_options`, the `unsaved` computation and the new
  module wiring (`authoring/mod.rs` `#[cfg(test)] mod tests;`, area modules under `tests.rs`).
- F015 move: reran `logs/move-map.py` (identical to the author's maps apart from path labels).
  An independent line-multiset comparison of the old route and test files against the split
  files showed only the listed changes and no lost test assertion.
- F014: read htmx 2.0.4's request function: `htmx:confirm` (with `target`) is fired first and,
  if cancelled, returns before `hx-sync`, the `hx-confirm` prompt and `htmx:beforeRequest`.
- Checks run: `cargo test --lib -- adapters::http application::workflow::authoring
  application::workflow::registry` → 375 passed, 0 failed, 1 ignored, including all 25
  `routes::workflows::ui::authoring` tests and the new and changed ones
  (`logs/r003-b-focused.log`). `cargo fmt --check` clean. Class scan 0 missing. Clippy:
  the author's log (`logs/b1a-r3-clippy.log`) was inspected, not rerun.
- The author's tests for F003 and F013 assert the same conditions as B's R002 probes, so the
  probes were not rerun.
- Reused: all R001/R002 conclusions for code unchanged since (everything but the two page files
  and the moved route code, which the move check shows unchanged apart from F003/F013).
- Not verifiable here: the client-side behaviour (dirty guard clicks, status text, reference
  failure state, prompt order in a real browser). It stays in F012.

### Outcome

- **PASS** for digest `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db`.
  F003, F013, F014 and F016 are fixed; F015 is adopted and verified. All B1a findings
  (F001–F011, F013–F016) are resolved, and no acceptance-blocking defect remains.
- **F012** remains open by plan. B considers it **not blocking B1a's acceptance** for the
  server-verified scope, but **blocking for phase acceptance** (criterion 6). The B1a acceptance
  record must state that the B1a client behaviour is verified only by inspection until the
  browser pass after B1b.

### Process and instruction feedback

- Three rounds were needed. R002's new defects all came from corrections crossing a shared
  contract; R003's corrections were narrow, and the author's move-map tool plus a
  line-multiset check made a large file split cheap to verify. Candidate practice (workflow
  scope): "when a correction round also moves code, supply a per-item move map; the reviewer
  cross-checks it with an independent multiset comparison".

## WF-P09-R004 — Gate P09-B1b-D1 rev 1 (frozen agent ids per version): PASS

### Identity

- Date 2026-10-24; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`, HEAD `da86871`.
- Stage: architecture gate, B1b. Decision reviewed: **P09-B1b-D1 rev 1** (TASK.md "B1b plan
  (revised 2026-10-24 after B1a acceptance)" → "Gate P09-B1b-D1 — frozen agent ids stored with
  each version (rev 1, 2026-10-24)"). This is a decision review, not an artifact review. The
  tree is changing under B1b, so no digest is claimed. B1a remains accepted at `1484a5bc…57db`.
- Coordination: COORDINATION.md r11, sha256 `3e8501b4…62f5`, unchanged. WF-C003 r1 applies to
  the batch (see DV-P09-7 below).

### Coverage and evidence

- Requirement: plan 09 "Agents" bullet; p08 WF-P08-F098 item 2 ("Used by" became binding usage).
- Claims checked against code:
  - **Schema.** `workflow_versions` DDL (init schema :5564; bundle ≤ 4 MiB check; PK
    `(company_id, id)`, no trigger). `workflow_bindings` / `workflow_binding_revisions` DDL and
    their composite company FKs (:10115–10151): the join path is company-scoped.
  - **Writes.** The only production insert is `definitions.rs:264-285`; the only other insert is
    the test `tests.rs:407` (plus an `UPDATE` of `source` in the same test). No `.sql` fixture
    and no `SELECT *` reader touch `workflow_versions`, and the insert uses runtime
    `sqlx::query`, so `.sqlx` is unaffected.
  - **Bundle.** `PublishedBundle.children` is exactly the pinned set:
    `dependencies::capture` + `pinned_versions` (step controls ∪ workflow-tool pins), with
    duplicates and unused children refused. Each child is the full child bundle (`verify_children`
    hash-checks it). `AgentSnapshot.agent_id` is a trusted identity, and agents are resolved by
    slug inside the company (`agent_publication::derive_on`).
  - **Storage.** The closure is deduped by version id (`store_bundle`).
    `supported_publication` walks the closure without a visited set but refuses more than 256
    visits, and it runs before `insert_version`.
  - **Rejected alternative.** No bundle cache and no per-company binding cap exist, so the claim
    that decoding per view is unbounded holds: up to 4 MiB per bound version.
- Answers to the coordinator's challenges:
  - **Stored column vs decoding:** justified. Decoding only the bound versions would still be
    up to 4 MiB × bindings per view. The column is small (at most the company's agents), derived
    once, and immutable with the row.
  - **Recursion set:** correct. Step children plus workflow-tool pins (including those reached
    through an agent's workflow-tool grants) is the run-reachable closure, and nothing else
    freezes agents. Counting twice is harmless with a distinct result, but the walk should
    dedupe by version id (F017; bounded on the publish path by `supported_publication`).
  - **No execution reader:** acceptable for a display index. Promote it to an invariant (F018.3).
  - **NOT NULL, no default:** right. There are no other inserts; the test copy must include the
    column because NOT NULL is checked before the FK trigger.
  - **Read scope and bound:** sound (company-scoped joins through the current configuration
    revision, `Visibility` in the route, 50 + `more`). Order, agent lookup and copy: F018.1–2.
- **DV-P09-7 (not part of the gate):** I read the code the same way. `ToolCheck::Run`
  (`workflow_tools.rs:75`) holds when the current registration of the name points to the same
  workflow, at any version. That is exactly P07-D2 rev 4 §D2.2 ("Re-registering the name for
  the **same** workflow re-enables those runs. Re-pointing the name to a different workflow, or
  reusing a revoked name for one, does not"; B PASS WF-P07-R010). WF-C003 r1's "old pins stay
  refused" is the imprecise statement, not the code. The UI copy in the B1b plan matches the
  code and D2 rev 4. The coordination response should cite D2 rev 4 §D2.2; no requirement
  changes.

### Outcome

- **PASS for P09-B1b-D1 rev 1.** Optional suggestions: WF-P09-F017 (name a deduplicating
  closure walk), WF-P09-F018 (read ordering and copy, agent-scope source, display-only
  invariant, element check, extra tests). No actionable defect. Adopting a suggestion that
  changes the decision text makes it rev 2, which needs only an affected recheck. The
  implementation is reviewed in the B1b batch review.

### Process and instruction feedback

- The gate text was complete and verifiable. Naming the affected inserts and the test's FK
  expectation saved a search, and every claim checked out.
- Candidate guidance for gate templates (scope: implement-opus-reviewed author): "a derived value
  computed over a bundle closure states its traversal (deduplicated by version) and its bound".

## WF-P09-R005 — B1b batch review + F012 browser pass: CHANGES REQUIRED

### Identity

- Date 2026-10-24; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`, HEAD `da86871`.
- Stage: batch review of B1b plus the F012 browser-pass evidence over B1a and B1b. Gate
  P09-B1b-D1 rev 1 (R004 PASS) implementation checked here.
- Digest `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555` (exclusions
  unchanged; manifest `logs/b1b-snapshot.txt`); confirmed at start and before returning. Base:
  accepted B1a `1484a5bc…57db`.
- Coordination: COORDINATION.md r11, sha256 `3e8501b4…62f5`, unchanged at start and end.
  WF-C003 r1 applies (tools admin), read against P07-D2 rev 4. WF-C004 r2: no new pool (reads
  use `self.pool()`).
- Delta integrity:
  - The R002→R003 hash lists differ exactly in the 12 R3 files, so the chain to B's own R002
    copy holds.
  - `delta.py` against `b1a-r3-filehashes.txt` gives exactly the 30 files of `b1b-files.txt`.
  - The five new `start-base` copies match `start-filehashes.txt`.
  - B regenerated the diff from the bases; it equals `logs/b1b.diff`.

### Coverage and evidence

- **Read in full:**
  - Migration hunk; `frozen_agent_ids`; the publish insert; the test insert; the reads port and
    all six new persistence reads, plus the shared `binding_uses`.
  - The `bindings.rs` JSON refactor (`BindingRequest`, `configure_binding`,
    `set_binding_activity`); `infra::selectable_bundle`.
  - `params.rs`; the UI `bindings.rs` and `tools.rs` routes; the bindings and tools pages;
    every small B1a page diff (DV-P09-9); router registration and `WRITE_ROUTES`; the tests'
    names and key assertions.
- **Callers and contracts read:**
  - `BindingService::configure` / `activate`; `save_binding_on`, `validate_change`,
    `check_run_as` (DV-P09-10 owner rule confirmed), `check_message_binding_cap`.
  - Application `resources::validate`: `Unavailable` → `Conflict`, `Revoked` → `NotFound`.
  - HTTP `connection_on` / `status_on` (`secret_set`); `admission.rs` → `verify_selection` →
    `supported_publication(Run)`; P07-D2 rev 4 §D2.2/§D2.3/§D2.7.
  - The `company_members` roles (the owner row exists); the htmx confirm path (R003).
- **Probe** (scratch copy of the R005 tree, `logs/r005-b-probes.log`):
  `probe_b5_unready_resource_on_create_configure_and_activate` confirmed F019 on all three paths.
- **CSS:** isolated Tailwind builds of the current and the reconstructed pre-p09 tree (scratch;
  17 reconstructed p08-changed files hash-verified) confirm DV-P09-8. The class scan over all ten
  p09-touched page files finds 0 missing.
- **Checks run** (55490 only):
  - `cargo test --lib -- adapters::http application::workflow::authoring
    application::workflow::registry application::workflow::publication` → 420 passed, 0
    failed, 1 ignored (`logs/r005-b-focused.log`).
  - `cargo test --lib -- adapters::persistence::workflow` → 948 passed, 0 failed
    (`logs/r005-b-persistence.log`), covering the new schema column, the insert and the
    cross-company copy on the final tree.
- **Browser evidence** (`logs/f012/`): inspected, not rerun. I read `pass.js` and checked its
  methods (theme toggle, computed-colour contrast via canvas, focus style, routed failures)
  against `pass-run.log` and the screenshots list. Assessment in F012's R005 disposition.
- **Conclusions on the coordinator's challenges:**
  - **Authorization and visibility:** every new route gates on `load_managed_company`.
    - `visible_binding` = `binding_use` + workflow match + `Visibility` (NotFound otherwise),
      and it runs before save-edit, activate, deactivate and form-open.
    - The channel card and the form's channel list apply `Visibility`; the agent cards look the
      agent up company-scoped.
    - Tool commands authorize again. Tests cover strangers using their own company and plain
      members on bindings, forms, activation and agent grants/usage, plus hidden channels in
      the tab, form and card.
  - **Keyed create and double submit:** sound for a real double submit, but every `Conflict`
    is taken for one — F019.
  - **Configure conflict keeping choices:** sound for a real revision conflict (same F019
    caveat). Association is fixed on edit (taken from the stored binding, threads included).
  - **Params:** expressibility fallbacks are sound; round-trip fidelity gaps — F021.
  - **Gate:** matches rev 1 plus F017/F018 (see their R005 dispositions).
  - **Tools admin copy:** the same-workflow re-registration semantics are right (DV-P09-7);
    the revoke and re-point consequences are wrong — F020. The two-step replace of a taken name
    is sound (warning first, then the revision; a stale revision shows the current state).
  - **Lazy cards:** load on `load`, swap `outerHTML`; failure → fallback text, busy cleared
    (browser-verified for the agent card).
  - **DV-P09-8:** agree; leaving `app.css` unrebuilt is acceptable for p09. The `check:css`
    failure is pre-existing and unrelated; report it as owed.
  - **DV-P09-9:** fine; classes present; contrast improved, as measured in the pass.
  - **F012:** largely closed; keyboard residual (see F012).
- **Reviewed lightly:**
  - SVG and screenshots: listed, not inspected one by one.
  - `binding_options` cost: up to 200 channel visibility checks per form render. Bounded and
    memoized, not measured.
  - The non-transactional `binding_uses` reads (resources read after the rows): display-only,
    not filed.

### Outcome

- **Changes required.**
  - Actionable: **WF-P09-F019** (Medium), **WF-P09-F020** (Medium), **WF-P09-F021** (Low).
  - F012: residual keyboard coverage, Low, not blocking B1b; carry into the final pass.
  - F017 and F018: adopted and verified.
- Notes, not filed:
  - The version-change GET carries the whole form in the query string, so a very large JSON
    params field could exceed proxy URL limits.
  - The "Used by" copy says "runs this agent" where "froze this agent" is exact.

### Process and instruction feedback

- The batch was large but reviewable.
- The probe-first approach found F019 quickly: a command's error class reused for a
  non-revision cause. The browser-pass evidence was well structured: scripts, raw log and JSON
  results with numeric details.
- F020 began in the plan's paraphrase of D2 rev 4. Candidate instruction (author scope): "copy
  that states a gated decision's consequences quotes or cites the decision's verification
  items, and a test pins it".

## WF-P09-R006 — B1b correction recheck (F019, F020, F021): CHANGES REQUIRED (Low only)

- Date 2026-10-25; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`, HEAD `da86871`.
  Stage: correction recheck by ID. Gated decisions: none changed (P09-B1b-D1 rev 1 stands).
- Digest `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e` (exclusions
  unchanged; `logs/b1b-snapshot-r2.txt`); confirmed at start and before returning. COORDINATION
  r11 `3e8501b4…62f5` unchanged.
- Delta integrity:
  - B's R005 scratch copy (the R005 tree plus B's probe in one test file) differs from the
    current `src` in exactly the 9 files of `logs/b1b-r2-files.txt`; migrations are unchanged.
  - `delta.py` against `logs/b1b-filehashes.txt` (spot-checked against that copy) gives the
    same 9.
  - B's own diff equals `logs/b1b-r2.diff`.
- Coverage:
  - Read the whole correction diff: `binding_moved` and its three call sites, the
    `params_mode` / `fields_for` path, `json_number`, the new copy constants and banners, and
    the four new or changed tests.
  - Rechecked the copy against P07-D2 rev 4 §D2.2/§D2.3/§D2.7 and the code (`admission.rs`,
    `save_binding_on`).
- Checks:
  - `cargo test --lib -- adapters::http application::workflow::authoring
    application::workflow::registry application::workflow::publication` → 424 passed, 0
    failed, 1 ignored, including the four new tests (`logs/r006-b-focused.log`).
  - `cargo fmt --check` clean; class scan 0 missing.
  - Chrome 155 headless value-sanitization check (`logs/r006-b-newline-check.log`).
  - Reused: the R005 persistence run (no persistence file changed); every R005 conclusion
    outside the 9 files.
- Outcome:
  - F019 fixed; F020 fixed.
  - F021 mostly fixed, with one Low residual left open: an `<input>` strips newlines, so a
    stored multi-line text parameter still changes on an unchanged save.
  - F012 residual (keyboard) unchanged: Low, not blocking B1b.
  - The "Used by" copy note is adopted ("froze this agent"). The query-string note is
    recorded as not adopted, which B accepts.
- Process feedback: the fixes were narrow and well tested. The one miss is a fidelity check
  done on server-side values while the browser transforms the control's value. Candidate rule
  for `pages/AGENTS.md`: "a round-trip guarantee for a form field must account for the
  control's own value sanitization (`<input>` strips newlines, `<textarea>` normalises line
  breaks to CRLF, `type=number` drops invalid strings)".

## WF-P09-R007 — B1b correction recheck (F021 residual): PASS

- Date 2026-10-25; workflow `implement-opus-reviewed-2_2`; reviewer B (Opus 5.5 / xhigh, same
  session); phase `p09`; worktree `/Users/mac02/dev/openbot`, branch `workflows`, HEAD `da86871`.
  Stage: correction recheck by ID (F021). Gated decisions: P09-B1b-D1 rev 1, unchanged.
- Digest `89d23e56e74c70e6fcea1d60a9b9b87d4cc5c829af3afd34e8d8ab8147760eb9` (exclusions
  unchanged; `logs/b1b-snapshot-r3.txt`); confirmed at start and before returning. COORDINATION
  r11 `3e8501b4…62f5` unchanged.
- Delta integrity:
  - The R005→R006 hash lists differ exactly in the 9 R006 files (a chain to B's R005 copy).
  - `delta.py` against `logs/b1b-r2-filehashes.txt` gives exactly the 2 files of
    `logs/b1b-r3-files.txt` (`params.rs`, `tests.rs`).
  - B diffed them against `logs/tree-b1b-r2/`, whose entries match the R006 hash list.
- Checks:
  - `cargo test --lib -- adapters::http application::workflow::authoring
    application::workflow::registry application::workflow::publication` → 424 passed, 0
    failed, 1 ignored (`logs/r007-b-focused.log`).
  - `cargo fmt --check` clean.
  - Chrome 155 headless JSON-textarea round trip (`logs/r007-b-json-textarea-check.log`).
  - Reused: R005/R006 for everything outside the 2 files.
- Outcome: **PASS for B1b at `89d23e56…0eb9`.** F019, F020 and F021 are fixed, and F017/F018
  are adopted and verified. No actionable B1b finding remains.
  - Open: F012's residual (keyboard walk beyond the tab strip, focus in light). It is Low, not
    blocking B1b, but it blocks phase acceptance (criterion 6) until the final browser pass.
  - Reported, not p09's: the pre-existing `check:css` failure (DV-P09-8) is owed to the user or
    CSS owner.
- Process feedback: a one-line, well-targeted fix with matching unit cases. No instruction
  change beyond R006's candidate (control value sanitization in round-trip guarantees).

## WF-P09-R008 — B2a batch review (runs workspace, detail, controls, live updates): CHANGES REQUIRED

- 2026-10-25; reviewer B (Opus 5.5 / xhigh). Artifact digest
  `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2`, HEAD `da86871`. I
  confirmed it at the start and again before returning, with the snapshot exclusions
  unchanged. COORDINATION r12 `05eb8e6e…2c69` matches the copy `logs/coordination-r12.md`.
  r11→r12 only adds p09's WF-C003 response, with no requirement change. Applicable notices:
  WF-C002 r1, WF-C003 r1 (tool-call children in inspection) and WF-C005 r1.
- Scope: B2a only, covering criteria 4, 5, 6 and part of 3. B2b is out of scope.
- Delta: `delta.py` against `b1b-r3-filehashes.txt` gives the 22 files of `logs/b2a-files.txt`.
  My regenerated diff equals `logs/b2a.diff`.
  - **Base gap → F028:** `routes/workflows/reconciliations.rs` is a p08 file (untracked; it is
    in `start-filehashes.txt` with hash `c6412655…d309`). There is no pre-B2a copy of it, so the
    diff shows it as created. I reviewed the whole file.
- Read in full:
  - `application/` and `persistence/workflow/run_views.rs`;
  - `routes/workflows/ui/runs/{mod,controls,events}.rs` and the four test files;
  - `pages/workflow_runs/{mod,detail,step}.rs`;
  - the `bindings.rs` extraction diff and `reconciliations.rs`;
  - the small wiring diffs: pages `mod.rs`, the authoring "Runs" links, `NodeMark::Stopped`,
    `ui/mod.rs`, the persistence and application `mod.rs`, and `infra/workflow.rs`;
  - TASK's B2 plan, the B2a design and log, and DV-P09-12;
  - the browser evidence (`b2a-pass.js`, `pass-run.log`; 32/32).
- Checks, run in the scratch copy `scratchpad/probe` (its own `target/`, synced from the
  worktree at this digest), on cluster 127.0.0.1:55490 with each test on its own database:
  - `cargo test --lib -- adapters::http`: **358 passed, 0 failed, 1 ignored**
    (`logs/r008-b-focused.log`). This equals the author's count. It includes the ten
    `ui::runs` tests and the six p08 JSON reconciliation tests over the extracted command.
  - `cargo fmt --check` in the worktree: clean.
  - Clippy: I relied on the author's `logs/b2a/clippy.log` (clean) and did not rerun it,
    because my scratch copy now holds probes.
  - `adapters::persistence::workflow` was not rerun. B2a changes no persistence behaviour: it
    adds one module of reads, and the UI tests exercise those reads.
  - Probes (`logs/r008-b-probes.log`, sources included; scratch copy only):
    `probe_r008_tick_reload_replaces_the_reconcile_note`,
    `probe_r008_reload_replaces_the_command_keys`,
    `probe_r008_a_reloaded_rerun_form_starts_a_second_run`,
    `probe_r008_a_same_key_edited_note_reads_as_nothing_recorded`,
    `probe_r008_run_view_reads_have_no_run_index`. All five ran and confirmed what they test.
- What holds (the challenge list):
  - **Reads:**
    - every query is scoped by company, and each step list also by run and execution;
    - lists fetch limit+1 for their "cut" flags;
    - the attempts join proves the run through `workflow_executions.run_id`; the
      `workflow_action_job_scope_key (company_id, workflow_execution_id, id)` serves
      `background_tasks`, and `task_attempts (task_id, attempt_number)` serves the attempts;
    - children join their wait through `workflow_wait_child_run_key`, which is unique per child,
      so there are no duplicate rows;
    - children inherit the parent's association (`persistence/workflow/children.rs`), so the
      parent pane reveals no hidden channel.
    - Not bounded: the work several of these reads do → F029.
  - **Authorization:**
    - `Ui::visible_run` applies the JSON routes' rule (managed company plus channel
      `Visibility`), and anything else is NotFound;
    - every handler calls `ui.company` first;
    - the list counts hidden rows per page and pages by `next` (WF-C005), as the B2a design asks
      and as B1 does for bindings;
    - the stream subscribes before it authorizes, checks channel visibility per event (a thread
      maps to its channel, the same as `Visibility`), and ignores other companies'
      (`run_scope`);
    - the 30 s re-check ends the stream when company access is lost, which its test shows; the
      reconnect then fails at `ui.company`.
  - **Shared commands:**
    - `start_command`, `cancel_command` and `retry_command` are verbatim extractions, with the
      status mapping left in the JSON handlers;
    - `reconcile_command` is the old handler body, as far as the file and the passing p08 tests
      show (F028);
    - `controllable_run` refuses a child with 409 naming the parent (WF-C005).
  - **htmx sync:**
    - the leaf design fixes DV-P09-12's three defects: writes are never in a replace group; a
      control's answer sits above `#run-body`, which outlasts wake-ups; `tick` reaches only an
      unfinished run's body;
    - `hx-preserve` on finished steps is keyed by execution id;
    - the body reload and pane opening are in separate sync groups, and a late body reload into
      a replaced pane is dropped by htmx (a detached target).
    - Gaps: the pane's own links → F026; what a reload destroys → F022, F023; the list → F025.
  - **Refusals:**
    - `Conflict`, `BadRequest` and `Diagnostic` re-render the pane with a fresh key and
      revision (WF-C005's 409 rule);
    - everything else (503, NotFound) is an error response, so htmx keeps the page and a toast
      says what failed;
    - the reconcile outcome mapping is right, except `IdempotencyConflict` → F024.
    - The 503 rule holds only until the next reload → F023.
- Findings: **F022 Medium, F023 Medium**; F024 L, F025 L, F026 L, F027 L (verification), F028 L
  (records), F029 L.
  - F012 keyboard residual (Low, phase-blocking) is unchanged. The B2a pass walks Tab and Enter
    over runs and steps, which is a start on criterion 6 for the new pages.
- Observations (not findings):
  - `visible_run` reads the whole view before the visibility check. A summary-first check would
    save the work for hidden runs.
  - Each pane render decodes the run's bundle (≤4 MiB) and runs `binding_options` (four queries)
    for `people`, including every 30 s tick.
  - `trigger_message` uses `LIMIT 1` without `ORDER BY`. A run has at most one admitted intent
    outcome in practice.
  - The annotation is hidden on a child run (`detail.rs:544,558`, the log's "never on a child"),
    but WF-C005 restricts only cancel and retry for children. If a child can wait on
    reconciliation, its pane gives no next step (the early return skips the recovery note).
    Worth one line in the log either way.
  - Truncation flags at 200 and 50 are rendered but no test drives a list past its bound.
- Outcome: **CHANGES REQUIRED** (two Medium). Recheck scope: F022–F029 at a new digest, the
  probes as regression cases, and the browser checks named in F022 and F023.

## WF-P09-R009 — B2a correction recheck: CHANGES REQUIRED

- 2026-10-25; workflow `implement-astra-reviewed-2.2`; replacement independent Reviewer B,
  requested `gpt-6-astra` / medium (actual runtime settings not separately exposed). Phase p09;
  worktree `/Users/mac02/dev/openbot`, branch workflows; start/base/HEAD
  `da868710d3b235d49856e3e481f8f3735f6b3e56`; preserved p08 artifact `00ec454f…6829`.
  Reviewed artifact `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e`, verified before review and at return.
  Exact excludes: `plan/workflow-foundation/phases/p09/docs`,
  `plan/workflow-foundation/phases/p08/docs`, `plan/workflow-foundation/COORDINATION.md`,
  `marketing`, `website`. Unchanged installed snapshot script used with the documented macOS
  realpath PATH adapter. Retained manifest/delta/tree: `logs/b2a-resume-snapshot.txt`,
  `logs/b2a-resume.diff`, `logs/b2a-resume-filehashes.txt`, `logs/tree-b2a-resume/`.
- Requirements read: original plan 09, root/source/HTTP/pages/application/persistence AGENTS,
  original R008 findings F022–F029 and note, relevant F012 history. Graph unavailable as documented;
  scoped rg and source reads used. Author summary was not treated as proof.
- Coordination: authoritative `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`
  r13 matches pinned `logs/coordination-r13.md`; WF-C002 r1, WF-C003 r1, WF-C004 r2 and WF-C005 r1
  remain applicable, acknowledged, and unchanged at return. B2a does not claim outstanding B2b,
  B3/B4 or final-phase notice obligations are complete.
- Coverage: correction delta and actual run page/detail/step scripts and callers; command-key
  replay and conflict feedback; list selection, row count, scrolling, caps and replace groups;
  note/step preservation and detached-target HTMX handling; route authorization tests and stream
  lag path; SQL query/index/cardinality changes and representative plans; current child-wait and
  notification paths. Reused R008's unchanged whole-batch evidence (including reconciliations.rs
  extraction evidence with its acknowledged unavailable base). No architecture gate change.
- Checks **run by B** (phase cluster 55490 only; isolated migrated test databases):
  - `DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09
    TEST_DATABASE_URL=postgres://workflow09@127.0.0.1:55490/workflow09_test SQLX_OFFLINE=true
    CARGO_INCREMENTAL=0 cargo test --lib adapters::http::routes::workflows::ui::runs`:
    **13 passed**, `logs/r009-runs.log`.
  - `CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    node --test scripts/tests/workflow-runs.mjs`: **2 passed**, `logs/r009-protocol.log`.
  - Inspected author evidence: broader HTTP **361 passed / 1 ignored** before final list-count
    correction; completed offline build/clippy; full application browser **43/43**, scripts and
    `logs/b2a-r2/browser-resume/`; representative before/after EXPLAIN ANALYZE BUFFERS.
    Did not rerun the application journey or mutate its fixtures. No full-suite claim.
- Outcome: **changes required**. F022, F024, F025, F026, F028 and F029 fixed. F023 Medium and
  F027 Low remain partially corrected; new F030 Low. Detailed evidence/resolution history is in
  `agent-review-findings.md`. F012 remains a separate final-phase keyboard verification gap,
  unchanged by this focused recheck. No pass for this digest.
- Process feedback: cohesive correction batch is reviewable; prior evidence use avoided redundant
  full review. The recorded untracked-file baseline limitation was corrected candidly. Candidate
  instructions supported by this round: when testing an idempotent retry UI, include a repeated
  operation whose before/after button label is already identical (F023); when removing periodic
  refresh, trace related-object invalidation as well as the root row (F030). No instruction edits.
- Follow-up to R008: its assumption that HTMX simply drops a detached target omitted the preserve
  pantry's earlier global DOM mutation. The author's new guard and B's 2/2 shipped-HTMX protocol
  checks close that integration defect; retained as observable review feedback, not a rewritten
  historical conclusion.

## WF-P09-R010 — B2a R009 correction recheck: PASS

- 2026-10-25; workflow `implement-astra-reviewed-2.2`; independent Reviewer B, requested
  `gpt-6-astra` / medium (actual runtime settings not separately exposed), same reviewer as R009.
  Phase p09; `/Users/mac02/dev/openbot`, branch workflows; start/base/HEAD
  `da868710d3b235d49856e3e481f8f3735f6b3e56`; preserved p08 artifact `00ec454f…6829`.
  **PASS** for exact artifact `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6`.
  Snapshot checked at start (`logs/r010-start-snapshot.txt`) and return
  (`logs/r010-return-snapshot.txt`) using unchanged installed script and documented PATH adapter.
  Exact exclusions unchanged: `plan/workflow-foundation/phases/p09/docs`,
  `plan/workflow-foundation/phases/p08/docs`, `plan/workflow-foundation/COORDINATION.md`,
  `marketing`, `website`.
- Scope: original plan 09 and R009 findings F023/F027/F030 plus changed integration. Read the
  complete `logs/b2a-r009.diff`, actual affected render/route/query/fixture callers, new tests and
  application-browser lost-response scenario. Evidence identity: `logs/b2a-r009-snapshot.txt`,
  diff/hash/tree `logs/b2a-r009*`; newly touched fixture baselines retained under
  `logs/tree-b2a-resume/`. The deleted-file sentinels are an evidence-manifest correction, not
  an implementation change. Original R008 and R009 evidence for unchanged code remains applicable.
- Coordination authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md` r13
  matches pinned `logs/coordination-r13.md` before return; WF-C002 r1, WF-C003 r1, WF-C004 r2,
  WF-C005 r1 unchanged and acknowledged. B2a inspection obligations are covered; later phase work
  and final integration obligations remain outside this pass. No changed architecture gate.
- B independently ran (phase PostgreSQL 55490 only, same env as R009, isolated migrated DBs):
  - `cargo test --lib adapters::http::routes::workflows::ui::runs`: **14 passed**, `logs/r010-runs.log`.
  - `CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    node --test scripts/tests/workflow-runs.mjs`: **3 passed**, `logs/r010-protocol.log`.
- Inspected author evidence: affected shared HTTP/agent fixture callers **54/54**
  (`logs/b2a-r2/r009-fixture-callers.log`); actual application browser **46/46** including the
  already-rerun lost-response/reconnect/retry with no third run (`logs/b2a-r2/browser-r009/`);
  offline build, formatting/whitespace and completed clippy. Did not repeat the broader HTTP
  suite or full application browser fixture mutation. No full-suite claim.
- Findings: F023, F027, F030 **fixed**; no new actionable finding. See appended dispositions in
  `agent-review-findings.md`. F022/F024–F026/F028–F029 stay closed using R009 evidence. F012
  remains a separate final-phase keyboard verification gap, neither closed nor waived here.
- Integration challenge: rerun history no longer silently renews intent; new intent uses an
  explicit read action; child event subscriptions match the children actually rendered, remain
  scoped, and retain unsent-note preservation; private-channel fixtures use consistent binding
  and admission association, while prior helper callers preserve Company semantics. No production
  persistence change in this correction.
- Process feedback: focused recheck size was appropriate; targeted tests now distinguish the
  repeated-operation retry case and use valid inaccessible identities with a positive control.
  No additional instruction change supported beyond the candidates recorded in R009.

## WF-P09-R011 — B2b conversation links whole-batch review: CHANGES REQUIRED

- 2026-10-25; workflow `implement-astra-reviewed-2.2`, phase p09; independent Reviewer B,
  requested gpt-6-astra / medium (actual runtime settings not separately exposed). Worktree
  `/Users/mac02/dev/openbot`, branch workflows; start/base/HEAD
  `da868710d3b235d49856e3e481f8f3735f6b3e56`; preserved p08/p09 work identified in TASK.
  Prerequisite B2a R010 `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6`.
  Current artifact `0d695075870e4287db513ffcb96c9cb1b5d54ab66f0332e441f7759e281e83a4` verified at start and return
  (`logs/r011-start-snapshot.txt`, `logs/r011-return-snapshot.txt`). Same exact five exclusions
  and macOS realpath adapter as R010: p09/docs, p08/docs, plan COORDINATION.md, marketing, website
  (full paths in R010). Current manifest/delta/hash/tree `logs/b2b*`, delta against accepted B2a.
- Requirements: original plan 09 conversation/run/SSE/authorization criteria; applicable root,
  src, application, persistence, HTTP and pages guides retained from previous rounds. Graph is
  unavailable as recorded; scoped rg and source reads used. Read the complete B2b diff, actual
  changed render/read/stream/helper implementations and relevant unchanged callers, rather than
  relying on TASK's author summary.
- Coordination authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md` r13
  matches `logs/coordination-r13.md`, notices WF-C002 r1, WF-C003 r1, WF-C004 r2, WF-C005 r1 remain
  applicable/acknowledged and unchanged at return. No new architecture gate: established read,
  authorization and SSE policies; defects below are implementation/integration issues.
- Coverage/challenges:
  - Initial mailbox messages and streaming messages share exact canonical-message attribution
    spans; run chips use full run IDs and escaped URLs, and titles are escaped. Existing batched
    run lookup reaches producing destinations and admitted binding outcomes with company/thread
    predicates. Current message builders write workflow-produced entries outbound, consistent
    with the renderer's label choice; no new producer/trigger conflation established.
  - Multiple active/waiting/failed runs render individually with explicit 50-row truncation.
    Scope and order use company/thread/version and deterministic timestamp/ID ordering. Existing
    thread-index EXPLAIN is retained; per-source-channel eligibility has a separate bounds issue.
  - Link eligibility applies manager membership and actual producing-channel viewer policy;
    source-channel hidden IDs remain plain labels. Destination routes reauthorize. Schedule reads
    already require managed schedule plus channel access; attention query visibility and manager
    checks support the new run links. Binding resolver scopes the binding and invokes
    visible_binding before redirecting to its workflow tab. No mutation/CSRF surface added.
  - SSE sends reconciled strip/attribution without replacing the mounted stream owner or composer;
    new message wakeups also refresh attribution. Durable message cursor and multiple batches
    exposed the retained-window mismatch F031. Existing event-before-subscribe/reconnect tests
    and the new browser harness cover the small-window path, not this boundary.
- B independently ran with phase PostgreSQL 55490 and standard phase environment:
  - `cargo test --lib -- adapters::http::pages::tests adapters::http::routes::ui::stream_tests
    adapters::http::routes::ui_schedules::tests binding_destinations thread_runs_are_bounded
    thread_attribution_links message_outcomes_lookup attention`: **148 passed, 1 ignored**,
    `logs/r011-focused.log`. No unavailable affected check; ignored test is retained, not passed.
  - `CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    node --test scripts/tests/workflow-conversations.mjs scripts/tests/sse-navigation.mjs`:
    **6 passed**, `logs/r011-browser.log`.
  - `git diff --check`: clean. Inspected author all-target offline clippy, 136/1-ignored focused
    result, 11/11 lifecycle tests and rollback-only EXPLAIN evidence under `logs/b2b/`.
    Server 3001 is still B2a; did not claim an actual B2b application-browser or full-suite pass.
- Outcome: **CHANGES REQUIRED**, F031 Low (retained-message reconciliation), F032 Low
  (unbounded channel read), F033 Low (explicit SQL alias rule). Saved evidence and remediation
  in `agent-review-findings.md`; no other actionable finding established. B2a acceptance remains
  unaffected. F012 remains separately open for final-phase keyboard verification.
- Process feedback: batch is cohesive and reviewable; whole-delta plus actual stream caller review
  surfaced a limit mismatch missed by mock/small-page tests. Candidate scoped guidance is recorded
  with F031. F032/F033 already have explicit applicable guidance; no instruction edits made.

## WF-P09-R012 — B2b R011 correction recheck: PASS

- 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09; same independent Reviewer B,
  requested gpt-6-astra / medium (actual runtime settings not separately exposed). Worktree
  `/Users/mac02/dev/openbot`, branch workflows; start/base/HEAD
  `da868710d3b235d49856e3e481f8f3735f6b3e56`, preserved pre-existing p08/p09 work as in TASK.
  **PASS** for exact artifact `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d`.
  Start/return manifests: `logs/r012-start-snapshot.txt`, `logs/r012-return-snapshot.txt`;
  complete correction input `logs/b2b-r011.diff` and b2b-r011 snapshot/hash/tree, against R011.
  Exact excludes unchanged: `plan/workflow-foundation/phases/p09/docs`,
  `plan/workflow-foundation/phases/p08/docs`, `plan/workflow-foundation/COORDINATION.md`,
  `marketing`, `website`; installed snapshot script unchanged, documented realpath PATH adapter.
- Reviewed F031/F032/F033 against the original plan 09 and applicable guides; read the complete
  correction delta, actual stream batch/read/render paths, existing live-edge scroll handling,
  both ThreadActivityReader implementations, channel lookup/policy integration and tests.
  Reused unchanged R011 whole-batch evidence. No new architecture gate or source edit by B.
- Coordination authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`
  remains r13, matching pinned `logs/coordination-r13.md` before return. Applicable notices
  WF-C002 r1, WF-C003 r1, WF-C004 r2, WF-C005 r1 unchanged and acknowledged. This is B2b acceptance
  evidence, not acceptance of later decision/sample or final-phase obligations.
- B independently ran (standard phase environment, PostgreSQL 55490 only):
  - `cargo test --lib -- adapters::http::pages::tests adapters::http::routes::ui::stream_tests
    reconnect_batch_links link_visibility_loads thread_attribution_links thread_runs_are_bounded`:
    **127 passed, 1 ignored**, `logs/r012-focused.log`. Ignored check remains ignored.
  - `CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    node --test scripts/tests/workflow-conversations.mjs scripts/tests/sse-navigation.mjs`:
    **7 passed**, `logs/r012-browser.log`.
  - `git diff --check`: clean. Inspected author focused 129/1-ignored, lifecycle 12/12,
    retained-anchor protocol 2/2, all-target offline clippy and production channel-query EXPLAIN.
    SQLx CLI is unavailable; inspected the documented successful direct SQLX_OFFLINE_DIR metadata
    generation/check with byte-identical existing cache. No SQL macro cache delta is claimed.
- Outcome: F031/F032/F033 **fixed**, no new actionable findings. Detailed dispositions appended
  to `agent-review-findings.md`. Explicit live-history truncation and per-batch eligibility close
  the two-window mismatch without removing bounds. Channel reads are scoped to the requested run
  set and preserve domain authorization. Reused B2a/R011 unaffected evidence remains applicable.
  F012 remains a separate final-phase keyboard verification gap. No full-suite or actual updated
  application-server browser claim; server 3001 remained the older binary.
- Process feedback: focused correction batch was reviewable; database tests now distinguish the
  old backlog batch from current-window IDs, and real HTMX tests establish trimming/anchor behavior.
  No additional instruction changes supported beyond R011's recorded window-alignment lesson.

## WF-P09-R013 — Architecture gate P09-D1 r1 (B4 isolated samples): PASS

- 2026-10-25; workflow `implement-astra-reviewed-2.2`; phase p09; independent Reviewer B,
  requested gpt-6-astra / medium (actual runtime settings not separately exposed). Worktree
  `/Users/mac02/dev/openbot`, branch workflows, HEAD/base
  `da868710d3b235d49856e3e481f8f3735f6b3e56`. **Decision-only gate**: immutable
  `logs/p09-d1-r1.md`, SHA-256 `56dcd3ff0290aa3d04f093c963f57964324a69efaa0c148cc9049387a1bfd07f`. Hash verified at start/return.
  Source tree was not frozen: author continued B3 verification. No B3/B4 implementation digest
  reviewed or accepted here, and no dependent B4 implementation is authorized before B3 acceptance.
- Original criteria read directly from plan09 Sample execution and Acceptance: same compiler/engine,
  isolation below UI, scripted model/human responses, fixtures for external reads, intercepted
  writes/messages/memory, zero effects through direct tools and children, no production resume or
  promotion. Applicable repository/source/application/HTTP/page guides retained from prior rounds.
- Authority `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md` r13 matches pinned
  `logs/coordination-r13.md`. WF-C002 r1, WF-C003 r1, WF-C004 r2 and WF-C005 r1 unchanged and
  acknowledged; C003's workflow-tool-child sample obligation is explicitly included in this gate.
- Actual contract trace (read-only, scoped fallback because graph unavailable):
  - `lifecycle/definitions.rs:47-66` validates a company-authorized expected draft revision and
    compiles a preview bundle with a nil, nonselectable version without publishing. This supplies
    the proposed sample input without admitting a production run.
  - `publication/mod.rs:28-55`, `publication/types.rs:120-159` and
    `publication/capabilities.rs:46-84` establish immutable finite bundles, exhaustive ToolKind
    dispatch classification, and frozen tool/skill selection. `child_publication.rs:17-103`
    captures both static children and workflow-tool pins with tenant/size bounds. MCP publication
    captures approved database snapshots and has no provider discovery call in that path.
  - `activation.rs:122-135` shares frozen input resolution; persistence activation `:169-177`
    owns the presently separate pure rule choice preparation, which the decision permits extracting
    for joint use. `batch.rs:86-155`, `completion.rs:53-85`, `decisions.rs:316-329` and
    `children.rs:26-84` provide the proposed shared pure execution, output/choice/schema/route,
    human submission and repeat/child completion semantics. These helpers do not grant durable
    effect ownership. Therefore the proposed isolated mode has a credible shared engine seam;
    a second independently implemented language interpreter is not required or approved.
  - `agent.rs:104-134` distinguishes tool-bearing agent.run from tool-free classify/decision
    preparation. Implementation review must preserve this existing distinction when consuming
    scripted direct tool calls; frozen agent grants alone do not add tools to tool-free steps.
  - Domain root budgets (`domain/workflow/budget.rs`) and child continuation (`children.rs`,
    persistence `child_waits.rs`) support shared descendant counters and bounded repeat rounds.
    The proposed conservative sample ceilings supplement rather than replenish compiled limits.
- Safety/architecture assessment: the sample evaluator receives only inert frozen content and
  fixture data, never production effect services, admission/claim/fence capabilities or bound
  resources. Every external leaf fails on missing fixtures; exhaustive direct-tool dispatch and
  pinned-child recursion keep the same isolation through agents and descendants. Reports have
  ephemeral identities and no control/resume bridge. Read-only authorization/snapshot assembly
  belongs before that evaluator; the decision does not grant a production service to the sample.
  Explicitly excluding durable lease/retry/receipt simulation is acceptable because reports must
  not assert those properties; language semantics and no-effect requirements still apply.
- Verification assessment: the planned compiled fixture tests, human schema/feedback checks,
  repeat and workflow-tool child cases, shared budgets/limits, missing-script failures, count
  comparisons and zero-call loopback trap cover the material risks. Import/capability checks alone
  are not proof; the decision itself requires behavioral tests and independent cohesive B4 review.
  Expected revision/tenant/CSRF/hostile output and browser progress/themes remain implementation
  acceptance obligations. No tests run for this text gate and no zero-effect behavior claimed yet.
- Outcome: **PASS for P09-D1 r1** at the named hash; no actionable or optional finding added.
  No existing implementation finding is closed by this gate, and F012 remains separately open.
- Process feedback: a pre-implementation gate is justified by the effect-capability boundary and
  shared-engine contract. The explicit non-capabilities and named existing pure seams make the
  design reviewable without authoring an alternative. No new instruction change supported.


## WF-P09-R014 — B3 human work queue whole-batch review

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime model not independently exposed); phase p09; worktree /Users/mac02/dev/openbot, workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact 8cb2171b82b9e72e6d5a69813b81066dc7012e0fc9e3674815221f338e794b5b.
- Scope: [TASK.md](TASK.md) B3, [REVIEW.md](REVIEW.md), original plan09 Human work queue and Acceptance. Complete `logs/b3.diff` against accepted R012 reviewed, including application DTOs, subject labels in both attention query branches, read/context/history extraction, history index, page/forms/routes, response guard, tests and CI browser fixture wiring. B4 remains unimplemented/unreviewed; P09-D1 r1 architecture acceptance remains separate.
- Authority: COORDINATION.md r13 compared byte-for-byte with logs/coordination-r13.md before return. WF-C002 r1, C003 r1, C004 r2, C005 r1 unchanged/acknowledged. Exact five snapshot exclusions: `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Start/end snapshots `logs/r014-start-snapshot.txt` and `logs/r014-end-snapshot.txt` match the named digest. GNU-realpath PATH adapter used; script unchanged. Graft unavailable, so scoped symbol/caller rg and exact file reads used.
- Contract inspection: existing lock_decision tenant/association visibility and reviewer assignment boundaries retained; sibling/parent history constrained by company and channel/thread scope. Per-run lateral newest41 heads, capped20 rounds and <=22 run candidates keep history bounded; full feedback loads follow bounded identity selection. Reviewed new history index and author 50k-decision EXPLAIN evidence. Protected requests retain immutable displayed proposal and hidden empty data; command settlement/revision/artifact/choice validation remains in existing authoritative commands. Read-only closed identities and hostile output escaping inspected. Forms preserve raw schema/JSON/feedback values and leading newlines, serialize comment/submission via stable shell, and guard detached responses; ordinary comment preservation works but terminal response transition fails (F034). Exact source-context destination window fails (F035).
- Independent checks: `cargo test --lib -- decision attention approval` with phase55490 test environment: **109 passed**, logs/r014-focused.log. Matching rendered browser fixture generated by those tests. Shipped decision/runs/conversation protocol scripts: **6 passed**, logs/r014-browser.log. Additional closed-comment probe: **1 passed**, asserting the defect, logs/r014-comment-close-probe.log. No project source edits or dev fixture resets. Inspected author focused/build/clippy/fmt/SQLx direct-cache and full app browser46/46 evidence under logs/b3; author results are distinct from independently executed checks. SQLx CLI unavailable, supported direct macro cache check used by author. Full phase suite intentionally deferred to final gate.
- Outcome: **Changes required**, all actionable findings recorded together: [F034](agent-review-findings.md#wf-p09-f034--comment-refresh-loses-an-unfinished-submission-when-the-decision-closes) Medium and [F035](agent-review-findings.md#wf-p09-f035--source-message-link-cannot-reach-a-message-outside-the-newest-history-window) Low. F012 remains separate final-phase residual. No other actionable finding from reviewed B3 delta. Source-link window evidence is inspection-only; no successful deep-message browser navigation claimed.
- Process feedback: browser tests need terminal-state refresh and destination-window cases; ordinary happy-path preservation and href string assertions miss both. Candidate improvements recorded with findings; no instructions changed.


## WF-P09-R015 — B3 R014 correction recheck

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p09; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Original plan09 Human work queue/Acceptance and applicable guides retained. P09-D1 r1 remains separate B4 architecture acceptance.
- Reviewed entire logs/b3-r014.diff and affected callers: latest-draft beforeSwap recovery/escaping, submission-versus-comment paths and detached guard; exact source link, company/channel/thread authorization, use-case wrapper, existing exact canonical-message persistence query, bounded run authorization, standalone render focus/links/actions, new tests and CI wiring. Reused unaffected R014 B3 history/settlement/queue evidence. Source tree frozen; no source edits, agents, or dev-server mutations.
- F034 fixed: closed comment swaps retain latest raw input read-only and safely; own submit stays clean. F035 fixed: one authorized source message loads outside newest200 and focuses without live-window scrolling. New standalone component dependencies expose F036 Low (broken ids control and inspected note-control target), recorded with browser proof. No further actionable findings in the correction delta.
- Independent checks: phase55490 database env, `cargo test --lib -- decision_browser_markup_and_closed_draft_recovery source_message_outside_window_is_exact_and_authorized`: 2/2 pass, logs/r015-focused.log. This regenerates real fixtures and exercises old/retained source plus scope refusals. Shipped decision+runs browser: 4/4 pass, logs/r015-browser.log. Added reviewer-only probe:1/1 passes defect assertions for both sources, logs/r015-source-controls-probe.log. Author decision40/UIstreams7/clippy/fmt/CSS evidence retained, not claimed independently rerun. Full phase suite deferred to final gate. Dev binary is older and was not used as correction proof.
- Start/end snapshots logs/r015-{start,end}-snapshot.txt both match the exact digest. Exact exclusions unchanged: `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Unmodified snapshot script with PATH realpath adapter. Coordination authoritative COORDINATION.md matches immutable logs/coordination-r13.md before/after; WF-C002r1/C003r1/C004r2/C005r1 unchanged and acknowledged. Graft unavailable; scoped read/rg fallback retained.
- Outcome: **Changes required (Low only)**: F034/F035 fixed; F036 open. F012 remains separate final-phase residual. Process lesson: standalone reuse needs checks of embedded control targets as well as rendered content and focus; recorded with F036, no instructions edited.


## WF-P09-R016 — B3 F036 correction recheck

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium; runtime model not independently exposed); p09; worktree /Users/mac02/dev/openbot, workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; exact artifact `44077e91138695e2b1d48eda0b50ae1617bf451078c4c6f79303b7f5bd9da575`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Original plan09 and applicable guides retained. P09-D1 r1 is the separate accepted B4 architecture gate; B4 implementation not reviewed here.
- Coverage: reviewed complete four-file logs/b3-r015.diff versus R015 and affected shared-renderer callers. Explicit Mailbox/ReadOnly selection removes both diagnostic dialog actions and active-note mutation controls only from standalone source rendering. Inspected tombstone/superseded status preservation, body escaping/markdown behavior, remaining attachment/download links, authorized full-navigation run chips, and latest-conversation link. Existing mailbox and live-stream callers retain their original public wrapper/behavior. No production query, authorization or schema change. Reused R014/R015 unaffected B3 and F034/F035 evidence.
- Independent verification: `cargo test --lib -- adapters::http::pages:: source_message_outside_window_is_exact_and_authorized`, phase55490 env with rendered fixture output: **175 passed, 1 ignored**, logs/r016-focused.log. Covers actual old/retained/active-note source reads, scope refusals, original mailbox controls, shared page rendering and regenerated fixtures. Shipped workflow-decisions browser: **1 passed**, logs/r016-browser.log, including input recovery, old/retained/note focus, absence of unsupported actions, and latest-conversation navigation. Remaining attachment/run destinations were inspected, not newly end-to-end exercised. Author clippy/fmt/CSS evidence retained; final full suite remains a phase gate. No source edits, agents, or dev-server mutations.
- Start/end logs/r016-{start,end}-snapshot.txt match the named digest. Exact exclusions: `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Snapshot script unchanged; PATH realpath adapter. Coordination COORDINATION.md r13 compared byte-for-byte to logs/coordination-r13.md before/after; WF-C002r1/C003r1/C004r2/C005r1 unchanged and acknowledged. Graft unavailable; scoped reads/rg fallback retained.
- Outcome: **PASS** for exact artifact above. F036 fixed; no open actionable B3 findings. F012 remains separately open for final-phase verification. This is B3 acceptance, not phase completion or B4 implementation approval.
- Process feedback: explicit renderer action context plus checks for both standalone omissions and original-container controls resolves the component-dependency gap. No additional instruction change supported beyond F036's recorded lesson.


## WF-P09-R017 — B4 isolated sample whole-batch review

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p09; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Full22-file logs/b4.diff against accepted R016 reviewed. Original plan09 Sample execution/Acceptance and repository/source/application/HTTP/page guides applied. P09-D1 r1 hash rechecked:56dcd3ff0290aa3d04f093c963f57964324a69efaa0c148cc9049387a1bfd07f.
- Isolation/semantics coverage: evaluator receives frozen bundle and inert request only; Session/Frame carry no persistence/provider/action/network/resource-binding handles. Shared prepare_activation extraction leaves production preparation behavior intact; sample shares prepare_inputs, rule decision, execute_pure, prepare_io/validate_completion, human prepare_submission, child_completion/provisional_output/decide_round. Inspected every step dispatcher branch and exhaustive direct ToolKind matching, agent-grant resolution, pinned workflow child execution, static calls/repeat paths, ordered scripts across repeated calls, explicit missing-fixture/unsupported failure, input/output schema checking, finite depth/activations/models/tokens/repetitions/tools/request/report bounds and root counters across descendants. Reports expose no admission/resume link or durable identity. Durable scheduling/retry semantics remain explicitly excluded in UI/gate. No production-effect leak found. Capability regression guard promised by gate is absent (F038).
- UI/read boundary: authorized company/draft expected revision and existing tenant-scoped validation/snapshot assembly precede sample execution. HTTP/MCP selection resolution reused; no resource binding handle enters sample. Sample route included in WRITE_ROUTES CSRF coverage. Hostile fields/report escaping, stale revision, invalid JSON retention, progress/failed-request recovery and detached response handling examined. Raw leading newline is lost by new textarea template (F037). Whole new tests/CI and browser journey inspected; full application journey author evidence retained rather than rerun against stopped server.
- Independent checks: phase55490 DB env, `cargo test --lib -- workflow::sample ui::authoring activation`: **91 passed**, logs/r017-focused.log. Covers actual compiled samples, shared production activation and HTTP authorization/count/loopback cases. Sample browser protocol **1 passed**, logs/r017-browser.log. Extracted-template browser newline probe confirms F037, logs/r017-newline-probe.log. No source edits, agents, or dev-server mutations. Author build/clippy/SQLx/CSS scan/full journey evidence retained under logs/b4; not claimed independently rerun. Final full suite/integration review still required.
- F012 assessed separately: author actual reviewed-support copy/edit/validate/sample/publish/activate journey with keyboard diagnostics/disclosures/confirm/binding and both-theme computed focus results, plus retained B2/B3 evidence, closes original residual. See its appended resolution. Existing CSS drift DV-P09-8 remains separate, not newly waived.
- Authority/freshness: start/end logs/r017-{start,end}-snapshot.txt match exact digest. Exact exclusions `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Unchanged snapshot script with PATH realpath adapter. COORDINATION.md r13 matches logs/coordination-r13.md before/after; WF-C002r1/C003r1/C004r2/C005r1 unchanged/acknowledged. Graft absent; scoped reads/rg fallback retained.
- Outcome: **Changes required (Low only)**: F037/F038 open, all findings saved together. F012 fixed. No other actionable B4 finding. Process: repeated textarea parsing issue needs reusable rendering invariant; gate verification promises need explicit completion evidence. No instructions changed.


## WF-P09-R018 — B4 F037/F038 correction recheck

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime model not independently exposed); p09; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; artifact `450480d565582b0b2e03a867ee624a2e5b5c3e017644951b558807482fd5a00b`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Original plan09 and accepted P09-D1 r1 unchanged. Full seven-file logs/b4-r017.diff inspected; reused unaffected R017 engine/isolation/UI evidence and F012 closure.
- Coverage: corrected sample textarea LF convention and browser actual-template cases; strengthened application journey LF assertion; explicit pure activation imports; recursive boundary guard allowlist, use-tree expansion, relative resolution, test-only exclusions, inclusion escape restrictions and CI wiring. Guard positive/negative tests inspected and independently run. Current code passes guard, but absolute qualified paths escape its regex; F038 residual documented with three inert temp-source probes. F037 fixed. No other actionable correction finding.
- Independent checks: `cargo test --lib -- workflow::sample sample_tests`, phase55490 environment:12/12 passed, logs/r018-focused.log. Browser workflow-samples2/2 passed, logs/r018-browser.log. Guard unit tests2/2 and real-tree gate pass, logs/r018-boundary-tests.log and r018-boundary.log. Reviewer probe confirms guard accepts forbidden absolute paths, logs/r018-boundary-probe.log. No source edits or agents; did not touch root's concurrent dev browser journey or its fixtures. Root completed the strengthened full app journey before return; inspected logs/b4-r017/app-browser.log PASS, including leading-LF error restoration. Root also reports operations46/46 on the rebuilt binary; this is author evidence, not an independent rerun.
- Start/end logs/r018-{start,end}-snapshot.txt match exact digest. Unchanged exclusions: `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Unmodified snapshot script with PATH realpath adapter. COORDINATION.md r13 matches logs/coordination-r13.md before/after, notices WF-C002r1/C003r1/C004r2/C005r1 unchanged/acknowledged. Graft absent; scoped read/rg fallback retained.
- Outcome: **Changes required (Low only)**: F037 fixed; F038 residual absolute-path guard coverage. F012 remains closed. Final full-suite and combined integration review still pending.
- Process feedback: boundary assertions should test ordinary syntactic variants of the forbidden capability, not only one qualified-path spelling. No instruction changes made.


## WF-P09-R019 — B4 F038 residual correction recheck

- Identity: 2026-10-25; implement-astra-reviewed-2.2; Reviewer B (requested Astra/medium, runtime identity not independently exposed); p09; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; exact artifact `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Original plan09 and P09-D1 r1 remain authoritative.
- Coverage: complete two-file logs/b4-r018.diff inspected. Tokenized path scanning now includes leading ::, arbitrary token spacing and stripped comments, preserves allowlisted pure imports and local associated-type handling, and normalizes roots. Negative fixtures cover filesystem/process/network, grouped rooted imports and new module paths. Missing root fails closed; raw-string handling tightened. Existing CI registration remains. No Rust/UI changes; reused R017 whole-B4 review and R018 focused12/12, browser2/2, final admin-app and operations46/46 evidence, including F037/F012 closures.
- Independent verification: guard fixture tests3/3 passed (logs/r019-boundary-tests.log); production sample tree passes (logs/r019-boundary.log); original rooted process/fs/network probes and a commented/spaced path all now reject (logs/r019-boundary-probe.log). Probe only passes inert source strings to scanner; no effects executed. No need to repeat unchanged Rust/DB/browser suites. No source edits or agents.
- Start/end logs/r019-{start,end}-snapshot.txt match exact digest. Exact exclusions: `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Unchanged snapshot script/PATH realpath adapter. COORDINATION.md r13 matches pinned logs/coordination-r13.md before/after; WF-C002r1/C003r1/C004r2/C005r1 unchanged/acknowledged. Scoped reads retained because graph unavailable.
- Outcome: **PASS** for exact artifact above. F038 fixed; no open actionable B4 findings. F037/F012 stay fixed. Final full verification and combined integration review remain required before phase completion.
- Process feedback: negative fixtures now test path syntax rather than only one spelling; the guard's role remains supplemental regression protection alongside behavioral tests. No new instruction changes supported.


## WF-P09-R020 — Final combined p09 integration review

- Identity: 2026-10-25; implement-astra-reviewed-2.2; independent Reviewer B (requested Astra/medium, runtime model not independently exposed); root/A user-confirmed Astra/high; p09; /Users/mac02/dev/openbot workflows; base/HEAD da868710d3b235d49856e3e481f8f3735f6b3e56; exact frozen artifact `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea`. [TASK.md](TASK.md), [REVIEW.md](REVIEW.md). Original plan09 all Acceptance criteria and repository full-suite mandate applied. Historical TASK text proposing permission before full tests is superseded by the user's repository instruction; full gates have now actually run.
- Review scope: combined authoring/setup, run operations, conversation attribution, human review and isolated sample integration. Reused accepted B1 R003/R007, B2a R010, B2b R012, B3 R016, B4 R019 and intervening correction evidence. No source changed after R019. Rechecked shared pure activation preparation, exact identity links/read-only source composition, common HTMX script/target ownership and form preservation, and corresponding acceptance test implementations/evidence. No new actionable finding. F012/F037/F038 remain fixed; no implementation finding reopened.
- Acceptance mapping:
  - Admin copy/edit/validate/sample/publish/bind/activate: actual rebuilt-application journey logs/b4-r017/app-browser.log and artifacts, including raw leading-LF errors, progress, keyboard and both themes. Existing capability-profile deviation DV-P09-1 follows authoritative 06-D5, as accepted in B1; no invented profile boundary added.
  - Human comment/revise/next draft/accept: actual database-backed route journey `decision_review_tests::reviewer_comments_revises_reads_next_draft_and_accepts_exact_artifact` passed in full suite; inspected exact artifact/feedback history and late-link checks. Final rendered decision fixtures2/2 and browser1/1 confirm form preservation, queued requests, terminal-state recovery and old/retained/note source navigation.
  - Concurrent runs/separate replies: accepted B2b attribution/read tests plus final suite and operations46/46 evidence; connect/wakeup/reconnect tests retain independent IDs, including event-before-subscribe and bounded retained-message windows.
  - Security: full suite includes tenant/foreign-ID/private-channel/hostile-output/CSRF tests across authoring, runs, decisions and source reads; sample shares authorized draft snapshot selection. No operator-template company-content bypass introduced. Existing route commands remain authoritative.
  - SSE and UI: accepted B2a/B2b browser protocols and final browser protocol suite cover reconnect, delayed stale answers, owner mounting, independent run state and pending form input. F012 named keyboard residual is closed by actual admin journey plus B2/B3 evidence.
  - Sample safety: R017 inspected entire inert evaluator/child/direct-tool paths; final suite includes zero-record delta, loopback trap, schema/choice/repeat/budget cases. R019 guard and negative fixtures pass in CI wiring. Pure production activation helper is shared; no sample admission, resume or production binding capability. Source guard supplements behavioral tests; it is not a formal Rust capability system.
- Final verification inspected (author-executed on unchanged artifact; independent batch checks remain separately recorded): logs/final/rust-results.json has six exit0 gates: locked offline all-target compilation, Clippy-Dwarnings, test compilation, network-isolated all-target suite, stock2048KiB stack suite, lease-budget. All-target library **2306 passed/3 ignored**, main **5 passed**; stack **2306 passed/3 ignored**. Lease **130 passed**, all required operations measured; largest measured work share budget.reserve355/1000ms=35.5%, below50% threshold. Unique owned PG55490 workflow09_final_157ec64124 database isolated final suite. Fresh-init migration test passed; ledger has one successful20260817000000 migration and source SHA-384 checksum match (migration-ledger.log).
- Additional retained final evidence: admin actual app journey PASS; operations actual app46/46; protocol suite15/15 at original B4 freeze plus corrected sample2/2 subsequently; final decision-fixtures2/2 and decision-browser1/1 on current regenerated markup; capability guard3/3 and tree scan; formatting/whitespace, deployment/credential-rotation/transport-boundary scripts and external-network isolation checks PASS. Reviewer started no DB workloads during root's final budget gates and made no source edits or spawned agents.
- Verification limitations, explicitly retained: the three full-suite ignores are one JavaScript dump developer aid and two optional live-DNS smoke tests intentionally excluded under external-network denial; no live-DNS coverage claimed. SQLx CLI is absent: actual SQLx migrator/fresh-schema tests, checksum ledger and recorded direct macro-supported cache refresh substitute for CLI commands; no claim that cargo sqlx ran. Full CSS regeneration still differs from tracked assets under independently established pre-existing DV-P09-8; new phase classes were checked present. No whole-repository CSS reproducibility pass claimed and no unrelated CSS rewritten. Server browser results are inspected author evidence; reviewer did not rerun all journeys. These limits do not introduce an unresolved p09 acceptance defect under the accepted scope/deviations.
- Freshness: logs/r020-start-snapshot.txt and r020-end-snapshot.txt match exact artifact. Exact exclusions `plan/workflow-foundation/phases/p09/docs`; `plan/workflow-foundation/phases/p08/docs`; `plan/workflow-foundation/COORDINATION.md`; `marketing`; `website`. Snapshot script unchanged with PATH realpath adapter. Authoritative COORDINATION.md r13 compared byte-for-byte to logs/coordination-r13.md before/after; WF-C002r1/C003r1/C004r2/C005r1 unchanged and acknowledged. Graph unavailable; scoped read/rg fallback retained.
- Outcome: **PASS — final combined p09 integration** for the exact artifact above, with the stated verification limitations. No open actionable implementation findings. No publication/deployment/merge action performed or authorized by this review.
- Process feedback: cohesive batch rechecks plus one final full-suite/stack/lease pass provided complementary coverage without rerunning broad suites per correction. Exact frozen snapshots and retained failure evidence made reuse auditable. Prior finding-specific lessons remain; no new instruction change supported.
