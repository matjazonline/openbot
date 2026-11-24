# AWC-P06 reviewer findings

Common context for every finding below: 2026-11-23; workflow implement-opus-reviewed-2_4; task
[TASK.md](TASK.md), review [REVIEW.md](REVIEW.md); phase p06, Batch A (test-only), stage batch review; review
round note [AWC-P06-A-R1-N1](agent-review-notes.md); reviewer B Fable 5.2 (`claude-fable-5-2`), effort medium;
worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, base/HEAD `fe7791d3`, digest
`d3bee2a425e9d3508223741ca231608e8c9336715dbece7ca497f5d061c8576f`. None originates in a gated decision (the
batch has none).

## AWC-P06-A-F1 — "Parent response suppression remains independent" has no behavioral evidence

- **Classification:** actionable defect; severity medium; category verification/requirements.
- **Evidence:**
  - Required: doc 06 A scenario 3 ("Parent response suppression remains independent"), under "Required
    scenarios" of a behavioral proof; doc 04 A.4 and 01 line 63 state the behavior (a child's
    `channel.no_reply` does not suppress the parent run's own response).
  - Observed: no test exercises it. TASK.md offers an inspection disposition ("suppression is held per run,
    `channel_reply_decisions` keyed by run"), the same inspection p04 recorded (p04 TASK F5, A3). In every call
    fixture that reaches `no_reply` (`channel_call_response_no_reply`,
    `channel_call_response_tests.rs:296`) the caller is a manually admitted run with no response owner, so
    nothing about the parent's response can be observed.
  - The manifest `logs/AWC-P06-A-R1.evidence.json` declares criterion G2 PASS, and G2 (TASK.md gap table)
    includes "parent ingress reply still published while its call child answers `no_reply`". That part was not
    exercised.
  - The stated reason ("a large fixture") does not hold for a response-owning caller: `hand_off_through`
    (`channel_call_response_tests.rs:1423`) and `channel_call_response_agent_destination_prepares` (`:1109`)
    already build a caller that owns a final response (a call child whose agent calls a nested desk). A nested
    desk published with `no_reply_destination()` and an assertion that the owning run still settles
    `("automatic", "done")` with its own body, and that its caller consumes that body, exercises the shared
    final-response path (doc 04 A.1 shares it between ingress and call owners).
  - Impact: a required scenario is reported as passing on inspection only. `verification-contracts.md`:
    inspection does not fulfil an explicit behavioral obligation.
- **Required:** add the test (preferably also for an ingress-owned parent if the e2e fixture in
  `channel_reply_e2e_support.rs` composes), or record this item as NOT EXERCISED in a manifest entry of its own
  with the reason and an explicit agreed disposition from root/the user. Do not leave it inside a PASS.
- **Instruction lesson:** observation: the author's gap table named the case, and the progress record then
  replaced it with a disposition while the manifest kept one PASS for G1–G8. Candidate: "a criterion replaced
  by a disposition gets its own manifest entry with status NOT EXERCISED". Hypothesis only; one occurrence.
- **Resolution history:** 2026-11-23 open.

## AWC-P06-A-F2 — G1's "neither destination is a workflow tool" assertion names the wrong workflow

- **Classification:** actionable defect; severity low; category verification (assertion does not establish
  the claim).
- **Evidence:** `channel_call_integration_tests.rs:118-125` publishes desk B's responder as workflow
  `DESTINATION + 2`; desk C's is `DESTINATION + 1` (`:139-146`). The registration count at `:160-168` binds
  `[DESTINATION, DESTINATION + 1]`: the superseded default responder and C. B's workflow is never checked
  (`publish_with_builtins` writes the passed id, `tests.rs:283`). Doc 06 A scenario 1 requires "Neither B nor
  C has a workflow-tool registration".
- **Required:** check `DESTINATION + 2` and `DESTINATION + 1` (or read the workflow ids off the two selected
  bindings), so the assertion would fail if B were registered.
- **Instruction lesson:** cause unknown (likely a constant left over when B took its own workflow id).
- **Resolution history:** 2026-11-23 open.

## AWC-P06-A-F3 — Dispositions recorded as PASS; one cites a test that does not exercise the claim

- **Classification:** actionable defect; severity low; category verification (mapping accuracy).
- **Evidence:**
  - The manifest has one entry for G1–G8 with status PASS. Several parts of those gaps are dispositions, not
    exercised checks, and the manifest gives them no status of their own.
  - **Poison wakeup (scenario 6, G3).** TASK.md cites
    `workflow_fairness_real_loop_mixes_full_unsupported_poison_wait_hung_and_refill`
    (`fairness_mixed_tests.rs:34`). Its poison is a job with corrupted run input, and its wait is a healthy
    `wait.timer`; no wakeup is poisoned and no call wait is present. The claim it is cited for is not shown by
    it. What does support the disposition, by inspection: `poll_work` pages by job id with a cursor for every
    work kind (`polling.rs:23`), a call wait is an ordinary `'wait'` candidate, and
    `workflow_wait_poison_timer_does_not_block_valid_due_work` (`wait_guard_tests.rs:4`) shows a failing wait
    settling terminally beside valid due work. The poisoned *response* half is exercised
    (`channel_call_resume_outcomes`, `response_invalid`).
  - **Depth limit (scenario 6, G3).** The gap table lists "depth/count limits". Count is exercised
    (`channel_call_launch_continuation_limit`), deadline and size are, the nested budget root is
    (`channel_call_integration_cycles_and_nested_budget`). The ancestry depth bound (`MAX_ANCESTRY`,
    `channel_call_target.rs:45,528`) has no test, and the progress record drops it without a disposition.
  - **Skills/resources capture at a call scope (scenario 2, G6)** and **resource revocation (scenario 7, G7)**
    rest on the generic manifest tests and p02 V6's inspection. No call test holds a skill or a resource.
  - I accept these four in substance (shared, kind-independent mechanisms; agent, version, binding and
    settings capture are exercised at the call scope). The defect is the record: a PASS that includes them, and
    one citation that does not show what it is cited for.
- **Required:** correct the poison citation; give each unexercised item its own manifest entry (NOT EXERCISED
  with reason, naming the generic test that the final gate will run) or add a small test: for example a call
  wait whose wakeup cannot continue beside a second due call wait, paged with `limit` 1; and a raw
  budget-link chain at the ancestry bound answering before any row.
- **Instruction lesson:** observation: `review-notes.md` already asks for mismatches between promised
  behavior and actual fixture paths to be findings. No new instruction supported.
- **Resolution history:** 2026-11-23 open.

## AWC-P06-A-F4 — Optional strengthening (not acceptance-blocking)

- **Classification:** optional suggestions; severity low; category verification.
- **Evidence and suggestions:**
  1. `channel_call_integration_launch_races_revocation` (`:516`) and
     `channel_call_integration_competing_consumptions` (`:476`) accept either outcome, and nothing shows that
     both orders occur; under `tokio::join!` on the test runtime the launch may always win. The invariant
     asserted holds in either arm, so this is not a defect. Recording which arm each round took (or forcing one
     order with a held row lock, as `channel_call_launch_lease_expired_at_commit` does) would make the race
     evidence explicit. `competing_consumptions` also discards an `Err` answer without asserting its kind.
  2. G1's desk B holds two active `MessageTrigger::Any` bindings (the default responder and the selected one).
     Asserting `desk_rows` intents/deliveries/external are 0 and that the desk has exactly one run would show
     "no ordinary ingress fan-out" on the one fixture where a second binding could be fanned out to.
  3. `logs/a-clippy.txt` retains clippy's output only; the manifest's `cargo fmt --check (clean)` has no
     retained output.
- **Instruction lesson:** none.
- **Resolution history:** 2026-11-23 open (optional).

## Resolution updates — recheck AWC-P06-A-R2 (2026-11-23)

Recheck note [AWC-P06-A-R2-N1](agent-review-notes.md); reviewer B Fable 5.2 (`claude-fable-5-2`), effort medium;
HEAD `fe7791d3`, digest `c7fba3400cd27117204379ea11848105f5d96846018aa35d3b27a3bef9d97431`.

- **AWC-P06-A-F1: fixed.** `channel_call_integration_nested_no_reply_keeps_parent_response`
  (`channel_call_integration_tests.rs:948`): nested desk C's decision is `suppressed` and B's agent receives
  `{"closure":"no_reply"}` through the port; B's run then ends `succeeded` with its own decision
  `automatic`/`done`, and the caller consumes `done(B)` with B's contract body. The assertion would fail if a
  child's suppression reached the owning run. Passed in my run (`logs/AWC-P06-A-R2-reviewer-focused.txt`).
  Stand-in limit, stated so the pass is not over-read: the response-owning parent is a call owner, not an
  ingress owner; the two share the final-response path (doc 04 A.1), and an ingress-owned parent remains
  unexercised. B's agent step is completed through `complete_io` with a literal output rather than a model turn.
- **AWC-P06-A-F2: fixed.** The query at `:160-168` now binds `DESTINATION + 1` and `DESTINATION + 2`, and a
  control at `:169-177` shows the same table counts the agent's registered tool (`CHILD`) as 1.
- **AWC-P06-A-F3: fixed (record).** Manifest `logs/AWC-P06-A-R2.evidence.json` validates with 3 PASS and 4
  NOT EXERCISED entries (G3 poison wakeup, G3 depth bound, G6 skill/resource capture, G7 resource revocation),
  each with a reason; the PASS entry's criteria no longer include them; the poison citation is withdrawn in
  TASK.md. These four remain unexercised at the call scope by agreed disposition, not passed.
- **AWC-P06-A-F4 (optional): closed.** Item 3 taken (`logs/a2-fmt-check.txt`). Items 1 and 2 declined with
  reasons in TASK.md; I accept both reasons (the asserted invariant holds in either order; `DeskRows` counts
  company-wide).

---

Common context for AWC-P06-B-F5 and F6: 2026-11-24; workflow implement-opus-reviewed-2_4; task
[TASK.md](TASK.md) "Batch B — final gate", review [REVIEW.md](REVIEW.md); phase p06, Batch B, stage final
integration review; note [AWC-P06-B-R1-N1](agent-review-notes.md); reviewer B Fable 5.2 (`claude-fable-5-2`),
effort medium; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, base/HEAD `fe7791d3`, digest
`bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f`. Neither originates in a gated decision.

## AWC-P06-B-F5 — Source-module reachability never ran in the full gate, yet the manifest declares it PASS

- **Classification:** actionable defect; severity medium; category verification (required evidence missing and
  mis-declared).
- **Evidence:**
  - Required: doc 06 B.4 lists "source-module reachability" among the evidence the final gate must explicitly
    include. `scripts/check.sh:186` relies on the all-targets suite to run `tests/source_modules.rs` once.
  - Observed: both full runs stopped after the failing lib target (`logs/b-check-sh.txt:6085` and the stack
    budget run: "error: test failed, to rerun pass `--lib`"); cargo does not go on to later targets without
    `--no-fail-fast`. The log has no `Running tests/source_modules.rs` and no `Running unittests src/main.rs`
    line anywhere. The only mention is the `--no-run` compile listing (`:699-700`). So the reachability
    target and the binary's unit tests were compiled but never executed, in either run.
  - The manifest's first PASS entry lists `check.sh gates reported PASS: source-modules/conformance`. Only the
    conformance-inventory gate ran; TASK.md B1–B3 does not mention the unexecuted targets.
  - Reviewer run at this digest: `cargo test --locked --offline --test source_modules --bin open_bots` → main.rs
    6 passed, source_modules 8 passed, exit 0 (`logs/AWC-P06-B-R1-reviewer-source-modules.txt`). So no
    reachability defect exists; the defect is the record.
- **Required:** remove the source-modules claim from the check.sh PASS entry; record that the all-targets
  runs executed the lib target only; add an entry for the reachability and binary targets citing an actual
  run (the author's own, or the reviewer log above), and say so in TASK.md and in doc 06's completion record.
- **Instruction lesson:** observation: a failing first target hides later targets in `cargo test
  --all-targets`. Candidate (scope: final-gate guidance in this repository or the workflow's verification
  reference): "when an all-targets run fails, list which targets executed before mapping gates to it".
  One occurrence; hypothesis that it recurs whenever a full suite is red.
- **Resolution history:** 2026-11-24 open.

## AWC-P06-B-F6 — Final-gate record omits or mis-states three items

- **Classification:** actionable defect; severity low; category verification (record accuracy; AGENTS.md
  "explicitly report unavailable checks").
- **Evidence:**
  1. **A CI gate neither run nor recorded.** `ci.yml:122-123` "Network isolation preserves the runner control
     plane" (`scripts/tests/network-isolation-scope.py`) is Linux-only; `check.sh:159-163` printed "Linux
     network-isolation scope: NOT EXERCISED locally; required in CI" (`logs/b-check-sh.txt:321`). Neither
     TASK.md B1/B3 nor the manifest records it. (The suite itself did run under the macOS sandbox guard:
     "network guard: loopback allowed; … external destinations rejected".)
  2. **Clippy status.** CI's gate is `cargo clippy --locked --all-targets -- -D warnings` on
     `dtolnay/rust-toolchain@stable` (`ci.yml:81,163`). On the local stable (1.99.1) it fails with 9
     `redundant_field_names` errors. I confirmed all nine files are unmodified against `fe7791d3`, so
     "pre-existing, unrelated" holds. The manifest's `clippy` entry is PASS and describes runs with the lint
     allowed; the failure of the gate as CI defines it appears only inside that entry's command text. It needs
     its own FAIL entry (pre-existing, unrelated), separate from the PASS for the scoped runs.
  3. **Crash-test description.** TASK.md B2.4 says "no panic text captured". The log has it:
     `logs/b-check-sh.txt:9861`, `agent_history_process_driver.rs:117`, "the actual worker must reach the
     required crash barrier". So the child's `worker.process` returned `Ok` after `AH_PROCESS_PREPARED`
     without reaching the barrier (it did not crash or overflow). That is the observable to carry with the
     gap; it narrows where a later investigation should look.
- **Required:** add the three corrections to TASK.md and the manifest (a NOT EXERCISED entry for 1, a FAIL
  entry for 2, the panic text for 3), and carry them into doc 06's completion record.
- **Instruction lesson:** none new; AGENTS.md already requires unavailable checks to be reported.
- **Resolution history:** 2026-11-24 open.

## Resolution updates — recheck AWC-P06-B-R2 (2026-11-24)

Recheck note [AWC-P06-B-R2-N1](agent-review-notes.md); reviewer B Fable 5.2 (`claude-fable-5-2`), effort medium;
HEAD `fe7791d3`, digest `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f` (unchanged; records only).

- **AWC-P06-B-F5: fixed.** Manifest `logs/AWC-P06-B-R2.evidence.json` drops the source-modules claim from the
  check.sh PASS entry and adds a PASS entry `source-module-reachability`, `binary-unit-tests` citing the
  author's own run `logs/b3-source-modules-bins.txt` (`tests/source_modules.rs` 8 passed; `main.rs` 6 passed;
  both exit 0), stating that check.sh's all-targets runs did not execute them. This matches my R1 run.
- **AWC-P06-B-F6: fixed.** (1) NOT EXERCISED entry `linux-network-isolation-control-plane` with the reason;
  (2) clippy split into `clippy-ci-command` FAIL (pre-existing, 9 errors in unchanged files) and
  `clippy-scoped` PASS; (3) TASK.md B2.4 now quotes the child's panic and states the child returned without
  reaching the barrier.
