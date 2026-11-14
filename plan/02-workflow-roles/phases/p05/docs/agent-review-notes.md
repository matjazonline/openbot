# 02WR-P05 review notes

## 02WR-P05-R1-N1 — batch review of p05 B1+B2 and final plan02 integration

### Identity

- Date 2026-11-14 (06:39–07:01 CET); workflow `implement-opus-reviewed-2_3`. Stage: batch review of p05 B1 (integration) and B2 (final-gate corrections), plus a final-integration review of combined plan02 effects not covered by the p01–p04 acceptances.
- Reviewer: Claude Code subagent B, Opus 5.5 `claude-opus-5-5`. Root requested xhigh in TASK; B cannot see its running effort. Fresh context, not a fork, no sub-agents.
- Records: [TASK](TASK.md), [REVIEW](REVIEW.md), [findings](agent-review-findings.md), [HANDOFF](HANDOFF.md).
- Phase `02WR-P05`; worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`; branch `room/check-last-handoff-md-under-plan-02-work-qc8d`. Review base/HEAD `2bbe2649`, plan02 base `103004a8`, plan02 checkpoint `f4edd160`; the rest of plan02 is uncommitted on HEAD.
- Artifact digest `905f05c23d9b8b0894467a6601ddf89f81d36b06f8bef29b1973fa6236efb28f`. Recomputed by B at entry (06:39) and before return (07:00), identical both times, with the assigned seven exclusions: `plan/02-workflow-roles/phases/p0{1..5}/docs`, `plan/02-workflow-roles/COORDINATION.md`, `plan/03-principal-addresses/COORDINATION.md`.
- Coordination: `plan/02-workflow-roles/COORDINATION.md`, revision 40 at entry and at return (sha256 `83547945…c026` at return). Applicable: I001–I004/r1; C001/r1, C002/r2, C003/r2, C004/r3, C005/r3, C006/r2, C007/r1, C008/r1, C009/r1, C010/r2; and the revision40 section "p05 final-gate corrections" (shared contract: the agent notice key and receipt guard). No architecture gate (N/A per assignment).
- Retained evidence relied on: manifest `logs/02WR-P05-R1.evidence.json`, which B validated with `check-evidence.py --expect-artifact 905f05c2…` (valid: 6 PASS / 4 FAIL / 3 NOT EXERCISED), and the logs it lists. B's own evidence: `logs/reviewer-R1-focused.txt` (sha256 `3599bb9d…ecef`) and `logs/reviewer-R1-boundaries.txt`.
- Isolation: p05 has no byte copy of the p04-accepted artifact. B identified p05's delta from the TASK/HANDOFF file list, checked it against file mtimes newer than `p04/docs/logs/B1-R2-acceptance-snapshot.txt` (04:17), and read the current text of each. `src/lib.rs`/`src/main.rs` mtimes moved at 06:39 (gate run 2's touch); they have no content change vs HEAD.

### Coverage and evidence

Requirements were derived from plan file 05 (04A docs, 04B gate, SOP reference, completion record), plans 01–04 for the documented contracts, carried-in p04 F8/F9, root/`src`/application/infra/persistence/http/pages AGENTS files, and revision40.

**B1 (inspected, plus executed where noted)**
- SOP reference: batch A item 3 now uses `DepartmentRole` (`lead`, `reviewer`; `src/domain/entities/department.rs:47`). No second key is defined. Supported.
- Docs were traced claim by claim to code:
  - `ESCALATION_ACK_HOURS`: parsed 1–168 with default 24 (`binding/escalation.rs:48-59`), startup panic in `AppConfig::from_env` (`infra/config.rs:609-612`), per-case `ack_hours` that is immutable once set (`init_schema.sql:17062,17221`).
  - Escalation policy shape and `binding.escalation_needs_department` on both activation and detach (`role_binding.rs`).
  - Every case producer: `escalations.rs` admission and run, `escalation_follow.rs` empty-reviewers, and all `run_failed_on` callers (guard, maintenance sweeper, recovery, pending recovery, wait commit, child, channel reply refusal, approvals, tokens, local-agent continuation). Root-only and suppression predicates checked.
  - Ladder `ladder_action`; hop 1 nearest-populated resolution and the 16-recipient refusal (`escalation_recipients.rs`); hop 2 parent lead only (`escalation_hops.rs::select_on`); 32 manual assignments.
  - Routes, the `SubmitBody` and choice wire shape, privileged/assigned authority, the role API (lead replaces, reviewer adds/removes), retirement via the demotion trigger with an audit event (`init_schema.sql:15548+`), follow-role compile and runtime, the 256 follow bound (Conflict; the worker logs and re-polls, so "retries later" holds), the role refusal codes (`department.rs:124-130`), and the 400 mapping (`app_error_impl.rs:34`).
  - `.env.example` and `deploy.md` name a key that is actually read, and startup does refuse invalid values. `check-integration-docs.py` PASS, including its configuration inventory.
  - **Two inaccurate sentences, F1.**
- `scripts/check.sh` Lease budget line is identical to CI's (`ci.yml:262-263`); `bash -n` OK; PASS 286 s in gate run 1.
- F8 tests: the helpers `bound_escalating`/`role_action_with` only thread `escalation` through to `ConfigureBinding`. The tests drive real `submit_decision("reject")` and real `resume_wait` expiry (the deadline is moved by UPDATE, a stand-in for elapsed time only). Each has a non-escalating control (0 cases) and an escalating case (exactly `[run, http.approval_{rejected,expired}, start]`), and the rejection test has a replay. **Executed by B: PASS.** Residual F8 scope (delivery, final response, local agent, token owners not driven end to end on an escalating binding) is honestly stated in TASK. F8 was optional; the disposition is accepted.
- F9 disposition (rejections escalate per D1.3) is consistent with the docs text and was reported to the user per the HANDOFF; no new finding.

**B2 corrections**
- C-AGENT-KEY:
  - `agent_notice_key` (application) calls the `TransportRenderer::application_message_key` port. The email renderer mints `<{nonce}@{company}.{app_domain}>` (`egress.rs:346`), the nonce is `event.source_id`, and that is exactly the receipt's `notice_id` (`agent_notice_message.rs` binds `event.source_id`).
  - The commit is one transaction. It rechecks the projection in the transaction (the company slug is inside the compared `NotificationDisposition::Agent`), refuses a missing key, and settles the event in the same transaction.
  - Each notice has exactly one event (`source_generation` generated as 1, `UNIQUE(company_id,event_id)`), so a slug change cannot duplicate a committed notice. Idempotence is supported.
  - The guard keeps every other provenance predicate (application origin, `in_app_only`, `internal_only`, system author, pending, attempts 0, desk, intent and message identity). `split_part(rfc,'@',1)='<'||notice_id` was checked by B on the cluster: own key t; other notice id f; key without `@` f; old literal f. Only two application-origin producers exist: the agent notice and trusted ingress, which uses fresh v4 nonces. The binding is therefore exact, and security is supported.
  - No other consumer of the old literal exists (`rg` over src/migrations/docs/scripts/.sqlx).
  - No adversarial test of the predicate exists (F2, optional). The worker now needs the email renderer for every projection (F3, optional, inert in production).
- C-TEST-EMAIL: `relayed_part` sits in the `#[cfg(test)]` email `test_support`, and `message_reply_tests::relayed_reply` uses it with an authentic relay row. Executed: 47 `intent_tests::reply_tests` PASS.
- C-CLIPPY: the helpers in `persistence/test_support.rs` moved above `mod tests`; the borrow fixes are mechanical. B ran `git blame` on the 7 remaining lint sites: all commits are ancestors of `103004a8`, with no uncommitted lines. CI uses `dtolnay/rust-toolchain@stable`, so CI would also report them: confirmed unrelated lint drift. `gate-clippy2` lints lib and lib-test completely (clippy reports every lint per crate), so no plan02 lint hides behind the diagnostic allow-list.
- C-STALE-TESTS: both tests are exactly p04's "2 inherited p03" reds (`p04/docs/logs/reviewer-B1-R2-regression.txt`). Production confirms person-principal reviewers (`provisioning_native.rs:192-205`). The fixes change only the expected or passed identifier. Executed: PASS.
- C-SAMPLE-BOUNDARY: `roles::refuse_unresolved_sample_role` is pure (bundle in, typed refusal out) and unchanged since HEAD; the allowlist comment is accurate. B reran all CI extra boundary scripts and the transport gate with a real `rg` on PATH: all PASS.
- Failure dispositions, checked against logs and code:
  - Inline-JS scan: flags a test string in `pages/workflow_authoring/sample.rs`; that file and `pages/tests.rs` are unchanged since `103004a8`.
  - auth-limit pair: cluster `datctype=C`; `lower('Älice')` stays unfolded. Plan02's `user.rs` edits touch only the external-login transactions.
  - `database_snapshot…`: `track_utility=on` on the cluster; PASS in B's run with it off.
  - mcp race: no plan02 change to `src/adapters/mcp`, `services::mcp_client`, `services::test_support` or `Cargo.lock`.
  - cloudroom Node ABI: PASS after `npm ci`.
  - doop timeouts: PASS alone.
  - Stack budget: no overflow ("tests failed for a reason unrelated to the stack budget"; 18 = 13 + 5 doop).
  - All supported as unrelated or environment. The base comparison is unavailable (base lib tests do not compile on 1.99.1), so these rest on direct evidence, as TASK states.
- Unavailable-gate reasons verified: no `docker` binary; node 26.13.1 and Homebrew `node@24` 24.22.3 versus CI's 24.21.0 pin; plan02 changes only the two doop sample JSON files under `integrations/` and nothing in `services/`.
- Removed tests: none. Four function names that disappeared since HEAD are p04 production helper renames, all predating p05. The only new `#[ignore]` is the manual escalation browser journey that p04 accepted.

**Executed by B**
- `logs/reviewer-R1-focused.txt`: copy of prebuilt lib binary `open_bots-81c16b063ee66a24` (sha256 `efec2c07…c49e`, built 06:26:47, newer than every src/migrations file); fresh DB `openbot_roles_p05_b1` with `track_utility=off`; `RUST_MIN_STACK=2097152`; `--test-threads=2`; under `scripts/test-network-isolation.sh`. Result: **161 PASS / 0 FAIL.** The run covered:
  - 19 `escalation_agent` (actual worker commit, lock and lease-loss reaching receipt insertion through the guard, role-lock contention, binding and teardown);
  - 15 `escalation_origin`, 9 `escalation_person`, 4 `escalation_control`;
  - 42 `notification`, 47 message-reply tests, 10 message delivery tests;
  - both F8 tests, the decision review test, `reviewed_assistants_park…`, `application_message_key…`, the ack-hours unit test, follow retirement and follow worker/repair tests, the knowledge-view worker test, and the snapshot test.
  - Gate run 2's DB suite ran concurrently; B's run still passed.
- `logs/reviewer-R1-boundaries.txt`: 7 boundary scripts PASS.
- SQL checks on the cluster (read-only): locale, `track_utility`, predicate semantics.

**Lightly reviewed or not exercised**
- Clippy fixes in the two untracked origin test files: trusted to `gate-clippy2` plus the passing tests.
- Doc claims about follow-role refresh callers and authority: traced at the function level, not re-proved behaviorally; p03/p04 accepted them.
- The whole DB suite, clippy, and the rest of the gate were not rerun by B; gate run 2 is outside this round. At 07:00 `logs/gate-check2.txt` had no `==> Summary` and was in "Database-backed test suite", so B makes no comment on its result.
- Combined plan02 effects beyond the corrections were reviewed through the gate-1 failure inventory: every red traced to a disposition, and no other plan02 failure exists. Phase-internal behavior was not re-reviewed.

### Outcome

**CHANGES REQUIRED, minor.**
- [02WR-P05-R1/F1](agent-review-findings.md): P3 actionable docs defect (two departments-doc sentences).
- [F2](agent-review-findings.md): P3 optional (adversarial guard test).
- [F3](agent-review-findings.md): P4 optional (unconditional renderer lookup).

Everything else is supported for the digest above. F1's correction is docs-only, so its recheck can reuse all other evidence unless code changes.

### Process and instruction feedback

- Batch size was reviewable. TASK's per-claim code trace list made the docs check efficient. Without a byte copy of the p04-accepted artifact, p05's delta had to be inferred from file lists and mtimes. Candidate (workflow scope): a phase that edits files already changed by a prior unaccepted-integration phase should retain a byte copy or per-file hash list of the prior acceptance, so B can diff p05's own delta. Evidence: this round; B's p04 R2 had such a copy.
- Revision40's lesson (a gate silently passes when `rg` is absent) was demonstrated and is useful. B confirmed `scripts/transport-boundary-check.sh` scans via `rg --files`.
- The two stale tests had been dispositioned as "inherited p03" across p04 rounds and were in fact plan02 regressions. Candidate (reviewer scope): before accepting an "inherited" red, read the failure assertion against the current production producer. Evidence: p04 R2 regression log lines 78 and the provisioning test; p05 TASK triage.
- Outside the review scope (preserved paths, ignored as assigned): `trail build` and two new learnings under `.trail/`.

## 02WR-P05-R2-N1 — correction recheck of R1/F1–F3

### Identity

- Date 2026-11-14 (07:11–07:17 CET); workflow `implement-opus-reviewed-2_3`; stage: correction recheck by finding ID plus affected behavior. Same reviewer and model as R1 (Opus 5.5 `claude-opus-5-5`; xhigh requested, not visible). Records: [TASK](TASK.md) "R1 result and corrections", [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Phase `02WR-P05`; same worktree and branch; base/HEAD `2bbe2649`. Digest `c3ababd836dafa6d285e16e7d4f884254439f50cdc6b30e7efc1ba39b9a8035e` (`logs/R2-snapshot1.txt`), recomputed by B at entry and before return, same seven exclusions; the snapshot file list is identical to R1's.
- Coordination revision 40 unchanged; the applicable notices are as in R1. Manifest `logs/02WR-P05-R2.evidence.json`, validated by B for this digest (2 PASS / 0 FAIL / 2 NOT EXERCISED).
- Correction delta, found by mtime newer than `logs/R1-snapshot3.txt` and matching root's list: `docs/departments.md`, `src/application/notification.rs`, `src/application/services/notification_worker.rs`, `src/adapters/persistence/test_support.rs`, `src/adapters/persistence/workflow/escalation_agent_guard_tests.rs`. The gate touched `src/lib.rs`/`src/main.rs` but neither has a content change vs HEAD; `.sqlx` is unchanged vs HEAD. The guard test file is untracked, so B compared it with the p04-era copy at `scratchpad/diffs/per/…escalation_agent_guard_tests.rs.diff` (00:49): lines 1-91 are byte-identical and only the new test was appended.

### Coverage and evidence

- F1: both corrected passages re-traced to code (roles and escalation recipient resolvers, `department.rs:124`, `escalation_hops.rs::create_on`, `agent_notice_message.rs::commit_on`). Accurate. Inspection only, as is appropriate for docs.
- F2: read the new test. The control replay with the minted key fails only as 23505, which shows all other guard predicates hold. Three forged keys each fail with 23514 on exactly `workflow_escalation_agent_receipt_guard`, inside rolled-back transactions on the fixture's own database. Executed: PASS.
- F3: read the closure signature and both callers. Non-agent dispositions return before the lookup; agent behavior and its failure class are unchanged. Executed through the agent, person, control and notification worker families: PASS.
- B executed:
  - `logs/reviewer-R2-focused.txt`: copy of rebuilt binary `open_bots-81c16b063ee66a24` (sha256 `6526fd0b…daf8`, built 07:07:28, newer than every changed source). Fresh DB `openbot_roles_p05_b2` with `track_utility=off`, 2 MiB stack, 2 threads, network isolation. **84 PASS / 0 FAIL**: 20 escalation_agent including the new test, 9 escalation_person, 4 escalation_control, 42 notification, 9 follow, the knowledge-view worker test, 2 dead-delivery tests. The same log holds the boundary scripts.
  - `logs/reviewer-R2-origin.txt`: 15 escalation_origin PASS (these use the changed `minted_agent_notice_key`).
  - `logs/reviewer-R2-transport-boundary.txt`: **FAIL**, the synthetic `@openbot.invalid` literal at `escalation_agent_guard_tests.rs:145` → new finding F4. The other six boundary scripts PASS.
- Inspected, not rerun: `logs/continue-run-f4fixes.txt` (179 PASS; covers reply, F8 and origin families) and `logs/gate-clippy4-diagnostic.txt` (EXIT 0, allows only the pre-existing lint names). The R2 delta has no construct matching those names.
- Gate run 3 (gate of record, `logs/gate-check3.txt`) had no `==> Summary` at return. Its early `Transport abstraction boundary` step already shows the same failure (lines 38-42). Its remaining gates were still running. B ran no cargo command.
- Reused unchanged R1 evidence for everything outside the five-file delta.

### Outcome

**CHANGES REQUIRED.**
- [02WR-P05-R1/F1](agent-review-findings.md) RESOLVED.
- [F3](agent-review-findings.md) RESOLVED.
- [F2](agent-review-findings.md) behavior resolved, but its implementation introduced **[02WR-P05-R2/F4](agent-review-findings.md) P2 actionable**: the transport boundary gate fails in check.sh and CI on the new test's `.invalid` literal.
- Recheck for F4 needs: the edited guard test, `scripts/tests/transport-boundary.sh` + `scripts/transport-boundary-check.sh` with a real `rg`, and the gate's Transport abstraction boundary. Gate 3's result for that gate is stale once fixed. Everything else may be reused if the fix touches only that test.

### Process and instruction feedback

- Recurrence of the revision40 lesson inside the same phase: a correction to `src` was verified by tests and clippy but not by the boundary scripts that this phase had just shown to be load-bearing. Candidate (repo correction rounds): every correction touching `src`, tests included, reruns `scripts/transport-boundary-check.sh` with a real `rg` before refreeze. Evidence: F4, `logs/gate-check3.txt:38-42`.
- The batch-size and isolation notes from R1 stand. The p04-era per-file diff copy in the scratchpad was what allowed B to prove the untracked test file's head was unchanged.

## 02WR-P05-R3-N1 — correction recheck of R2/F4 (and R1/F2's case)

### Identity

- Date 2026-11-14 (08:32–08:37 CET); workflow `implement-opus-reviewed-2_3`; stage: correction recheck by finding ID plus affected behavior. Same reviewer and model (Opus 5.5 `claude-opus-5-5`; xhigh requested, not visible). Records: [TASK](TASK.md) "Gate run 3 result" and "F4 correction and R3 inputs", [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Same worktree and branch; base/HEAD `2bbe2649`. Digest `bd5279a566cdffd7b50e30277160ecb512e6cc9f3871b431422b695391a01da7`, recomputed by B at entry and before return; same seven exclusions; the file list is identical to R2's.
- Coordination revision 40. Its only change is the trimmed EOF blank line flagged by gate 3's Patch whitespace step; `git diff HEAD --check` is now clean. Manifest `logs/02WR-P05-R3.evidence.json` validated by B for this digest (4 PASS / 1 FAIL, the FAIL being gate 3, triaged).
- Source delta vs `c3ababd8`, by mtime newer than B's R2 snapshot: only `src/adapters/persistence/workflow/escalation_agent_guard_tests.rs`. Lines 1-91 are byte-identical to the p04-era copy, and against B's R2 read only lines 144-145 differ. `src/lib.rs`/`src/main.rs` mtimes are gate touches with no content change; `.sqlx` is unchanged vs HEAD.

### Coverage and evidence

- F4 / F2: the forged case is the old local part under the minted key's own domain. It still discriminates, has no synthetic identity and no regex evasion.
- B executed `logs/reviewer-R3-checks.txt`:
  - `git diff HEAD --check` EXIT 0; `rustfmt --check` on the changed file EXIT 0.
  - The gate's `.invalid` pattern over `src` (excluding the email adapter) finds no match.
  - All 7 boundary scripts PASS with a real `rg`.
  - Focused tests on a copy of the rebuilt binary (sha256 `3d55886c…5b5d`, built 08:29:08, newer than every source) on fresh `openbot_roles_p05_b3` (`track_utility=off`, 2 MiB stack, network isolation): **24 PASS / 0 FAIL**. These are the 20 escalation_agent tests including the guard test, plus the 4 doop tests that timed out under load in gate 3's stack-budget run.
- Gate 3 (`logs/gate-check3.txt`, digest `c3ababd8`, Summary present). I read its failure lists:
  - DB suite: 4450 PASS / 5 FAIL. Those 5 are exactly R1's confirmed-unrelated set (inline-JS scan, mcp race, 2 C-locale auth tests, `track_utility` snapshot). The 6 cloudroom tests and both formerly stale tests now pass.
  - Stack budget: the same 5 plus 4 doop document timeouts; no overflow, and the script reports "unrelated to the stack budget". The 4 doop tests pass alone at 2 MiB in the author's run and in B's.
  - Clippy: the known 7 pre-plan02 lints.
  - Patch whitespace and transport boundary: the two plan02 items, now corrected and rechecked above.
  - Everything else PASS, including migrations, SQLx metadata, module reachability and lease budget.
  - Gate 3's results remain valid for this digest outside the corrected test file; the corrected file is covered by the delta checks above.
- Inspected, not rerun: `logs/gate-clippy5-diagnostic.txt` (EXIT 0), `logs/gate-whitespace-fmt4.txt`, `logs/continue-run-f4delta.txt` (39 PASS).

### Outcome

**PASS for digest `bd5279a566cdffd7b50e30277160ecb512e6cc9f3871b431422b695391a01da7`.** [02WR-P05-R2/F4](agent-review-findings.md) RESOLVED; [02WR-P05-R1/F2](agent-review-findings.md) RESOLVED; F1 and F3 remain RESOLVED from R2. No open findings.

Reporting caveats for root (dispositions, not findings):
- The gate-of-record steps Clippy, Database-backed suite and Stack budget are red in this environment for the confirmed-unrelated reasons above and must be reported as FAIL with those dispositions, not as PASS.
- The doop load timeouts under the 2 MiB rerun are environment load, supported by isolated passes. Their relation to stack size is not otherwise proved.
- The Docker, memory-integration, doop-integration and Node-pinned package gates remain unavailable, with the reasons recorded in TASK.

### Process and instruction feedback

The R2 candidate (rerun the transport boundary script with a real `rg` after any `src` edit, tests included) was applied in R3 and caught nothing further. No new instruction is supported.
