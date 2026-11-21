# Reviewer notes — 06DA-P03

## 06DA-P03-B1-R1-N1 — p03 whole batch: 03A docs + 03B final gate + combined integration

### Identity

- 2026-11-19; implement-opus-reviewed-2_4; p03/B1 whole-batch review.
- Records: [TASK](TASK.md) sections "03A docs", "03B final gate" and "03B gate record"; [HANDOFF](HANDOFF.md); [REVIEW](REVIEW.md); [findings](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan06`, branch `plan06`, HEAD `a93cd38844c76dd48dbbe42ceba8ebf68864d4df`.
- Artifact: the working tree, consisting of `docs/departments.md`, `docs/authentication-operations.md`, `docs/workflow-operations.md` and the 03 completion record.
- Digest `61b33200230c82b9b1c12e746326ceb6c6d14f44dc9b5823e4bce65e44c5c57d`, exclusions `phases/p03/docs`, `phases/p02/docs`, `phases/p01/docs`, `COORDINATION.md`. Reproduced together with `logs/B1-author-freeze.snapshot.txt`; rechecked at return.
- Coordination r10 (committed in HEAD): decisions (a)–(f), U7, the p02 B2-F3 scope decision and the inherited Doop deferral. No notices.

### 03A coverage

I checked every edited claim against the code:
- the predicate (`unit_management_role`, `any_unit_management_role`);
- `CompanyMembership::administers_company`;
- the department use-case rules (create, update and move, delete, members, roles) and the root rules;
- desk move needing both units;
- agent and definition management;
- knowledge read, write, move and delete over the managed subtree;
- `may_invite_as`, `require_inviter` and `refuse_admin_address`;
- 404 versus F10 403;
- `administration_history` and the history card;
- `channel_reply::authorize_settings` → `authorize_association_on` for desk responses;
- the private-memory note.

Most claims are accurate. Finding F1 lists six edited statements that overstate or understate the code, and three pre-existing stale "admin" statements in `departments.md` (lead change run-as, the "admin cannot see the desk" refusal, the Builder `list_company_agents`). I verified these remain accurate: TypeSafe connections, final-reply reviewers, memory capture, run inspection, escalation, and the view-grant line.

### 03B coverage

I checked every row of the gate record against `final-check.txt` (summary 11728–11773) and the rerun logs:
- **Plugin package:** a Node pin assertion (`v24.22.3` versus `v24.21.0`). Correctly labelled unavailable.
- **Transport boundary:** the `rg` gap. Self-test "gate accepted rig in application" in the run; rerun with ripgrep 15.0.0: PASS.
- **DB suite:** 4883 / 1 / 30. The only failure is `doop_agent_current_preparation_reaches_ready_before_model_io` (8 vs 7). Its test file and `PREPARED_AGENT_VERSION = 8` are identical at `be1b504b` (main) and HEAD, so it is inherited, not caused by Plan06. It is not relabelled.
- **Remaining targets:** cargo stops after the lib. check.sh runs `--all-targets`, and the only other targets are the `src/main.rs` bin and `tests/source_modules.rs`. Both ran separately on a fresh migrated database: 6 and 8 PASS.
- **Browser gate:** a FAIL (hang) in the run; whole-gate rerun 18/18 PASS. `knowledge-browser.mjs`, `assets/app.js` and `pages/knowledge.rs` are unchanged since main, so it is plausibly unrelated. "Pre-existing" is not shown (F2).
- **Stack budget:** zero "has overflowed its stack"; the script prints "tests failed for a reason unrelated to the stack budget"; red only on Doop.
- **Lease budget:** PASS.
- **Images:** no Docker socket; unavailable.
- **Memory model:** HTTP 404. I reproduced it independently with curl at 2026-11-19T21:56Z: all three pinned v1.24.4 archives return 404 (linux-aarch64, linux-x64, osx-arm64), and `api.github.com/…/releases/tags/v1.24.4` returns 404.
- **Memory extracted suite and standalone journey:** the `tomllib` gap; reruns with 3.13 stop at the missing model artifacts. Unavailable.
- **OpenBot and memory integration:** the service exits because the embedding model is unavailable.
- **Attribution extracted:** the `tomllib` gap; rerun PASS.
- **Doop:** deferred, with failures kept visible. Its two failures need Docker.
- **Linux network-isolation scope:** NOT EXERCISED (CI-only). The macOS guard ran for the DB suite, stack, lease and remaining targets.
- **CI comparison:** every ci.yml step maps to a check.sh gate; `network-isolation-scope.py` is Linux-conditional (check.sh:159).
- **SQLx:** a fresh UTF-8 schema `prepare` left `.sqlx` unchanged (no git diff), and check.sh "SQLx metadata" passed.
- **Graph refresh:** `trail build` exit 0.
- **Startup plan paths:** `main.rs` lists the README and documents 01–03 (294–297), and all five plan files carry the no-backward-compatibility note.
- **Thresholds:** Plan06 changed no thresholds or scripts (`git diff 174c83b3 HEAD -- scripts .cargo .github Cargo.*` is empty), so the AGENTS.md raised-limit rule is not triggered.
- **03B.4 evidence list:** fully covered, except that the Linux isolation scope is NOT EXERCISED and the DB suite is red on the deferred Doop assertion. The completion record states both honestly and keeps "Do not declare the implementation complete while a required check is unavailable". It overstates nothing, but omits the gates that passed only on rerun (F2).
- **Manifest:** `logs/06DA-P03-B1-R1.evidence.json` validates (4 PASS / 2 FAIL / 4 NOT EXERCISED).

### Combined integration effects (p01 + p02)

The UTF-8 full suite is the only failure-free whole-tree run of all accepted batches together, apart from Doop.
- 4883 passed and 30 ignored, the same ignored count as the p01 M1 baseline; 4914 tests in total, consistent with B2-R2 discovery.
- The C-locale auth pair passes on UTF-8, confirming p01's environment disposition.
- cloudroom passed.
- No new combined regression is visible.

### Checks

Inspection of logs and code. My only executed check was the read-only onnxruntime availability probe above. No test runs: the code is unchanged since the accepted batches, and the artifact is docs only (AGENTS.md: documentation-only changes need no Rust suites).

I wrote the probe output into a log and then removed it, because this assignment limits my writes to the three record files. The results are reproduced here.

### Outcome

**CHANGES REQUIRED.**
- [06DA-P03-B1-F1](agent-review-findings.md) (low, actionable): docs accuracy.
- [06DA-P03-B1-F2](agent-review-findings.md) (optional): completion-record omissions and wording.
- The 03B gate execution and dispositions are otherwise accepted: no FAIL or unavailable result is mislabelled as passed, and every "unrelated" claim except "pre-existing" for the browser hang is supported by evidence.
- Plan06 must still not be declared complete. Required checks are unavailable (Docker images, onnxruntime-1.24.4 memory gates, plugin Node pin, Linux isolation scope), and the DB suite and stack budget are red on the deferred Doop assertion. The record says this.

### Process feedback

- The gate record is thorough and honest. Its separation into unavailable, tool-gap-rerun and deferred results made verification quick.
- The doc drift found in F1 comes from editing only the paragraphs listed by the plan. Candidate instruction (in F1): grep the whole document for role words when the rule changes.

## 06DA-P03-B1-R2-N1 — recheck of B1-F1/F2

- **Identity.**
  - 2026-11-19; implement-opus-reviewed-2_4; p03/B1 correction recheck.
  - Reviewer: Claude Opus 5.5 (`claude-opus-5-5`); effort not visible to me (assigned xhigh).
  - HEAD `a93cd388`. Artifact `f8bfaccd7dadb06a774f1bde3dffd5001be0b3fb5246f67f9804df5d73f2d160`, same four exclusions. My snapshot reproduces the digest and `logs/B1-R2-author-freeze.snapshot.txt` (the same four files).
  - The worktree shows only the four artifact files and p03 records. Coordination r10.
  - Rechecked at return.
- **Coverage.**
  - Re-read every edited passage: the `departments.md` intro, line 39, the lead-change paragraph, the Builder paragraph, the Administration table and the card text; the authentication-operations membership paragraph; the completion record.
  - Re-verified items 1–9 against the code. In particular, the root name is editable (`update_department_on` guards only the slug and parent), root person versus agent removal, owner-only `owned_company` writes, the `follow_department_lead_on` actor, and Builder AnyUnit plus the `unit_management_role` filter.
  - Integration docs log PASS. Manifest `logs/06DA-P03-B1-R2.evidence.json` validates (6 PASS / 2 FAIL / 4 NOT EXERCISED).
  - No source, script or threshold changed (status shows only the four files). The 03B evidence and conclusions from R1 are reused unchanged.
- **Outcome.**
  - **PASS for p03 B1 at `f8bfaccd7dadb06a774f1bde3dffd5001be0b3fb5246f67f9804df5d73f2d160`.** F1 FIXED, F2 APPLIED, no new findings (one non-blocking wording observation, in the findings file).
  - This accepts the 03A docs and the 03B gate record **as recorded**. It is **not** a claim that Plan06 is complete or that the whole suite or CI is green. Required checks remain unavailable (Docker images, onnxruntime-1.24.4 memory gates, plugin Node pin, Linux isolation scope), and the DB suite and stack budget are red on the deferred Doop assertion. The completion record's closing line therefore still applies.
- **Process feedback.** The corrections were precise and checked against code references. No new instruction candidates.
