# 03PA-P01 agent review notes

## 03PA-P01-D1-R1

### Identity

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md) (section "D1 — architecture gate (revision r1)"), [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Batch and stage: p01 B1 (all of p01, per D1.1), architecture gate, before any implementation.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to me; the author requested xhigh. I am a fresh subagent, not a fork, and I spawned no agents.
- Phase `03PA-P01`. Worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`, branch `room/check-last-handoff-md-under-plan-02-work-qc8d`.
- Commits: phase start, review base and HEAD are all `cb46c6fdfc728634f4943e5e7b47a30fd89ce5c1`.
- Artifact digest: `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`. Exclusions: `plan/03-principal-addresses/phases/p01/docs plan/03-principal-addresses/COORDINATION.md plan/02-workflow-roles/COORDINATION.md plan/02-workflow-roles/phases/p05/docs`. I checked the digest at start and again before returning; it was identical both times.
- Gated decision: **D1/r1**, immutable copy `logs/D1-r1.md`, sha256 `2105b82bcaf4b06df49758152e2d68a9087f2b9312a6fe5f60105ba5a47c773c`. Rechecked unchanged.
- Coordination: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/plan/03-principal-addresses/COORDINATION.md`, revision 7, sha256 `05d8c4f666afaf6a4e3cc622e0215368bb58d6bd77dec16b9203b8254d6a6abd`.
  - Applicable: I001/r1, I003/r1 and 03PA-C001/r1 through C005/r1. I002/r1 is superseded by revision 7's adoption section.
  - Freshness: rechecked before returning; still revision 7 with the same hash. The plan02 producer, `plan/02-workflow-roles/COORDINATION.md`, is revision 41 (complete).
- Evidence relied on:
  - `logs/D1-inventory.md`, sha256 `0a193e99940b869ee707dced05b92dd04ba09a275a640b72cdbf6222cfaba8b1`. I verified the parts I relied on against the code; see the coverage below.
  - Source at `cb46c6fd`.

### Coverage and evidence (inspection only; no tests run, since a gate needs none)

**Requirements read in the originals.**
- `README.md` decisions 1–7.
- `01-unified-addresses.md`, batches 01A and 01B and the focused checks.
- `02`, `03` and `04`, read for D1.1's moves.
- Root `AGENTS.md` and the AGENTS.md files under `src/`, `src/application/`, `src/adapters/persistence/`, `src/adapters/http/`, `src/adapters/http/pages/` and `src/adapters/smtp/`.
- The skill's `review-notes`, `findings`, `verification-contracts` and `concurrent-phases` (notice protocol) references.

**D1.1 (resequencing), verified.**
- `workflow_escalation_agent_route` joins the `agent:{id}` provisioning command, then the desk, binding and email endpoint, so removing the automatic desk in p01 would break plan02's notice routing. Keeping the end state and moving agent addresses together with desk removal is sound.
- Gaps: p02's agent-target checks and the missing notice (F3).

**D1.2 (schema), verified.**
- `channel_slugs` shape, format, primary key, primary index and foreign key (init_schema `:3677`, `:3683`, `:6111`, `:6119`, `:7926`, `:9537`).
- The only SQL reader is `:13275`.
- The `principals` shape and kind checks, and the unique key `(company_id,id,kind)` at `:6638`.
- The precedents `guard_department_principal_kind` (`FOR KEY SHARE`) and `department_principal_demoted` (AFTER UPDATE OF kind).
- The three demotion UPDATEs: `company_invite.rs:594`, `user_lifecycle.rs:99`, `agent.rs:488`.
- Both deviations are accepted on substance:
  - The kind-typed FK columns are strictly stronger tenancy proof.
  - Without `principals.slug`, `external`/`system` principals carry no slug. The cost is the existence guarantee (F7) and a p03 equality invariant (F8).
- Gaps: locking (F4) and the lost `channel_id` index (F6).

**D1.3 (person slugs), verified.**
- `create_person_principal_on` is the only production creator or promoter of persons (`participant.rs:330-410`). Its callers are `company.rs:177` and `company_invite.rs:409`, both passing the username.
- `users.username` is unbounded, with no format check.
- `validate_slug` does not check format (F5).
- No agent or desk is auto-seeded at a fixed slug; all creation paths are user-initiated (checked `create_company_agent_on`/`assistant_channel_on` callers).
- The profile route exists at `/ui/profile` and is user-scoped.

**D1.4 (resolution), verified.**
- Parser grammar (`selector.rs:62-170`): `+` pipelines, context suffix stripping, and the address-wide context hint.
- `classify`/`route` (`ingress.rs:247-337`), and `AddressedTarget` with its 3 match sites.
- `resolve_addresses` is all-or-nothing (F1).
- The SMTP 550 mapping, and that bounce emails are sent only from the webhooks (F1).
- The relay's pre-ingest resolution and egress's retryable mapping (F2).
- `sender_trust`/`sender_needs_spam_scan` treat a principal as a stranger and so fail closed; acceptable.
- SendGrid and Resend are both signed.
- The Resend harness exists (F7).

**D1.5 (invariants and races).**
- PK-decided desk/join race: sound. Whichever transaction inserts first wins; the other gets the suffix or the conflict.
- Edit-vs-demotion and insert-vs-demotion are unaddressed (F4).
- I4 overstated (F7).

**D1.6 and D1.7.**
- Checked against the plan's three focused-check families. `_help lists both` is moved to p02, with a recorded disposition.
- Gaps: F7.

**Notices.**
- C001, C003 and C004 have no p01 effect under D1.1 (verified that no admission routing moves).
- C002: the new demotion trigger is independent of `record_department_role_retirement`.
- C005: holds only if no new platform-composed bounce is introduced (F1).
- I001: respected; no shim or view.
- I003: focused checks; the final gate is p04's.

**Startup notes.** `NO_BACKWARD_COMPATIBILITY_NOTES` already lists all five plan03 files (`src/main.rs:233-237`).

**Lighter coverage.**
- I read the profile UI, CSRF and progress-feedback mechanics only at route level; they are not architecture-gate material.
- I did not exhaustively verify the inventory's test counts (about 78) or its "others" bucket. They are not load-bearing for the decision.

### Outcome

**Changes required. No PASS for D1/r1.**

- Actionable defects:
  - [F1](agent-review-findings.md#03pa-p01-d1-r1f1--per-recipient-principal-refusal-with-the-desk-still-admitted-is-not-a-mechanism-the-ingress-has-and-d1r1-does-not-specify-one) (High)
  - [F2](agent-review-findings.md#03pa-p01-d1-r1f2--the-internal-relay-does-not-reach-classifyresolve_addresses-first-a-principal-target-becomes-a-retryable-internal-error) (Medium)
  - [F3](agent-review-findings.md#03pa-p01-d1-r1f3--d11-moves-scope-between-phases-without-settling-p02s-new-dependency-and-without-a-coordination-notice) (Medium)
  - [F4](agent-review-findings.md#03pa-p01-d1-r1f4--the-kind-guard-and-demotion-trigger-pair-is-a-concurrency-protocol-but-d1r1-gives-neither-its-locking-nor-the-edit-paths-lock-order-and-has-no-competing-test) (Medium)
  - [F5](agent-review-findings.md#03pa-p01-d1-r1f5--slug-derivation-can-produce-a-slug-the-format-check-rejects-failing-company-creation-or-joining) (Low)
  - [F6](agent-review-findings.md#03pa-p01-d1-r1f6--the-new-primary-key-drops-the-index-that-every-desk-slug-read-used) (Low)
  - [F7](agent-review-findings.md#03pa-p01-d1-r1f7--i4-overstates-what-the-schema-proves-and-d16d17-leave-required-coverage-out) (Low)
- Optional suggestions: [F8](agent-review-findings.md#03pa-p01-d1-r1f8--tell-the-user-about-the-resequencing-and-the-schema-deviations) and [F9](agent-review-findings.md#03pa-p01-d1-r1f9--resolve-in-one-read-and-note-what-deriving-from-the-username-exposes) (Info).
- A recheck must cover a D1/r2 that resolves F1–F7, plus the activated notice for F3.
- These parts of D1/r1 were reviewed as sound and can be reused unchanged unless r2 alters them:
  - D1.1's root rationale;
  - D1.2's table and constraint shape, apart from the index and trigger locking;
  - D1.3's editing authorization model.

### Process and instruction feedback

- **The gate was warranted.** It covers schema, cross-phase sequencing and a trigger concurrency protocol. It was the right size: one decision set, reviewable in one pass.
- **Most of the defects come from the decision prose, not missing evidence.** The inventory itself said the relay skips classification, but D1.4 claimed convergence (F2). D1.4/D1.6 restated the plan's "per recipient" without the existing all-or-nothing contract (F1).
  - Candidate (skill, architecture-gate checklist): "For each changed routing or resolution decision, cite the current contract of the function changed, such as its whole-message vs per-item semantics and each transport's rejection mapping, and list each path's first resolution site."
  - Evidence: F1, F2.
- **Moving scope across phases is not prompted by the TASK template.**
  - Candidate (skill, Large batches / architecture gate): "When a gate moves scope between phases, list the receiving phases' focused checks that depend on the moved items, and request the cross-phase notice in the same decision revision."
  - Evidence: F3.
- **Persistence AGENTS lacks the BEFORE-trigger lock interaction.**
  - Candidate (`src/adapters/persistence/AGENTS.md`): "A BEFORE ROW UPDATE trigger runs after the child tuple is locked; a parent-row lock taken inside it can deadlock against a parent-side trigger that writes the child."
  - Evidence: F4. This is a hypothesis from Postgres semantics; no reproduction was run.
- **Demonstrably useful.**
  - The inventory's stale-line table and its §4 relay note.
  - The trail CLI for the bounce-path search.
  - The real-`rg` path supplied in the assignment.

## 03PA-P01-D1-R2

### Identity

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md) (section "D1 — architecture gate (revision r2)"), [REVIEW](REVIEW.md), [findings](agent-review-findings.md#round-03pa-p01-d1-r2-recheck-of-d1r2).
- Batch and stage: p01 B1, architecture-gate recheck. The round was requested by the coordinator.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to me. I am the same B instance as R1; I spawned no agents.
- Phase `03PA-P01`, same worktree and branch.
- Commits: review base and HEAD are both `cb46c6fdfc728634f4943e5e7b47a30fd89ce5c1`.
- Digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`, with the same four exclusions. Identical at start and before return. There are no source changes.
- Gated decision: **D1/r2**, `logs/D1-r2.md`, sha256 `f51249eeb72c36f941a6b25e8ac1849281e17edbb7b69ae30246404459693a81`. The TASK "revision r2" section is byte-identical to it, and r1 is preserved in TASK and in `logs/D1-r1.md`.
- Coordination: revision 8, sha256 `f0a604f72d0e9bed7733a346f00396a60b782050e6ad16da6eaf9e8d5c59daca`.
  - Freshness: compared with the committed revision 6 and with the revision 7 text I read in R1. The only additions are C005/r1 and the adoption section (both already in revision 7), plus the new C006/r1 (proposed) and the revision 8 note. C001–C004 and I001–I003 are unchanged. Rechecked before returning.
  - Applicable: I001/r1, I003/r1, C001/r1–C005/r1, and C006/r1 as proposed.

### Coverage and evidence

- **Delta review.** I reviewed every section of r2 against the R1 findings, and against the code at the sites each answer relies on: `routing.rs:129-230`, `policy.rs:26-104`, `smtp/server.rs:797-825`, `relay.rs:10-103`, `egress.rs:500-517`, `onboarding.rs:355-365`, `channel.rs:92-100/531`, init_schema `:6638`, `:15528-15571`.
- **Probed, not just read.** The D1.2 lock-mode premise was tested with a disposable-database probe on PostgreSQL 18.7: [logs/D1-R2-review-lock-probe.txt](logs/D1-R2-review-lock-probe.txt), script `logs/D1-R2-review-lock-probe.sh.txt`. It refutes r2's stated premise (R2/F2) and confirms that the chosen `FOR SHARE` works.
- **Lock interleavings traced by inspection.** I followed both orders for each pair:
  - edit against removal, under the one-statement edit with no principal lock and the guard not firing on slug-only updates;
  - insert against demotion.

  No cycle forms, and no address survives for an `external` principal.
- **Rechecked and accepted unchanged in substance:**
  - D1.1's stand-in and its limits;
  - C006/r1;
  - the D1.3 derivation and person paths;
  - the D1.4 whole-message refusal, relay refusal and consumer dispositions;
  - the D1.5 restatements;
  - the D1.6 checks and gates;
  - the D1.7 tables.
- **Reused from R1 without re-reading:**
  - D1.1's root rationale (the `agent:` desk route);
  - the D1.2 table shape and FK deviations;
  - D1.3's edit authorization model.

  r2 leaves all three unchanged.
- **Not exercised:** no project tests were run (a gate needs none). The probe uses a minimal table with the same constraint shape, not the migrated init schema.

### Outcome

**Changes required. No PASS for D1/r2.** R1/F1–F7 are resolved in substance (see the resolution updates); F8 is deferred to the root's user report. Two residual Low defects remain.

- **[R2/F1](agent-review-findings.md#03pa-p01-d1-r2f1--the-non-team-indistinguishability-claim-is-not-carried-to-the-smtp-reply-line-the-outsiders-main-channel) (Low):** SMTP is not covered by the non-team equivalence or its assertion.
- **[R2/F2](agent-review-findings.md#03pa-p01-d1-r2f2--d12s-lock-mode-premise-is-false-a-demotion-takes-for-update-not-for-no-key-update) (Low):** the recorded lock-mode premise is false.
- **[R2/F3](agent-review-findings.md#03pa-p01-d1-r2f3--optional-scope-the-one-read-resolution-to-the-messages-slugs) (Info):** optional suggestion.

Both defects are text-level corrections to D1.2, D1.4 and D1.6; neither changes the protocol. A recheck of a D1/r3 that touches only those statements can be confined to the R2/F1 and R2/F2 deltas.

### Process and instruction feedback

- **The correction round worked.** All seven R1 defects were answered in one revision, with a section map, and C006 was proposed in the same revision, as R1/F3 asked.
- **Two candidate lessons**, recorded in the findings:
  - assert an indistinguishability property on every observer channel (R2/F1);
  - cite or probe lock-mode claims in gated decisions, and note in persistence AGENTS that `principals.kind` is a key column (R2/F2).
- **Probing was cheap and decisive.** A disposable-database probe resolved a disputed concurrency premise in seconds.
  - Candidate (skill, architecture gate): "When a gated decision rests on a lock-mode or isolation claim, the author or B probes it on a disposable database and retains the log."

## 03PA-P01-D1-R3

### Identity

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md) (the r3 text is the second section headed "revision r2", at line 365; see R3/F1), [REVIEW](REVIEW.md), [findings](agent-review-findings.md#round-03pa-p01-d1-r3-delta-recheck-of-d1r3).
- Batch and stage: p01 B1, architecture-gate delta recheck.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to me. I am the same B instance as in R1 and R2, and I spawned no agents.
- Phase `03PA-P01`, same worktree and branch.
- Commits: review base and HEAD are both `cb46c6fdfc728634f4943e5e7b47a30fd89ce5c1`.
- Digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`, with the same four exclusions. Identical at start and before return.
- Gated decision: **D1/r3**, `logs/D1-r3.md`, sha256 `d5fd5109c8d559a644ca810c82c2f54fb64bd5726697935aab1ba32e8f06d7ee`.
- Coordination: revision 8, sha256 `f0a604f72d0e9bed7733a346f00396a60b782050e6ad16da6eaf9e8d5c59daca`. Unchanged since R2, and rechecked before return. Applicable notices: I001/r1, I003/r1, C001/r1–C005/r1, and C006/r1 as proposed.

### Coverage and evidence

- **Delta, by diff.** `diff` of `logs/D1-r2.md` against `logs/D1-r3.md`, ignoring the title lines, shows changes only in:
  - D1.2: the guard's lock mode and its rationale, and the create/promote line;
  - D1.4: the scoped one-read and the non-team `UnknownChannel` rule;
  - D1.6: the equality assertions and the SMTP byte-equality check;
  - D1.7: the `UndeliverableSlugs` row.

  Everything else, including D1.1 (C006's origin), is identical to r2.
- **Checked against code.**
  - The `kind()` precedence and the `unknown()` and suggestion path (`policy.rs:233-285`, `routing.rs:166-217`);
  - `as_str` (`policy.rs:84-85`) and the SMTP 550 (`server.rs:819-823`);
  - the bounce body (`notices.rs:102-140`);
  - team status through `access_context` (`routing.rs:178`, `:681`), which propagates errors.
- **Probe evidence reused from R2.** [logs/D1-R2-review-lock-probe.txt](logs/D1-R2-review-lock-probe.txt) shows `FOR KEY SHARE` is blocked by a `kind` update and not by a `display_label` update. The environment and PostgreSQL version are unchanged.
- **Reused without re-review.** Every part of r2 that r3 leaves unchanged: those parts were accepted in R2, apart from R2/F1–F3, which are now resolved.
- **Not exercised.** No project tests were run; a gate needs none.

### Outcome

**PASS for decision revision D1/r3**, the decision text in `logs/D1-r3.md`, sha256 `d5fd5109…d7ee`, for the artifact at digest `eff341d3…d991`.

- **Resolved:** R1/F1–F7 (in R2); R2/F1 and R2/F2 (here); R2/F3 adopted.
- **Deferred:** R1/F8, to root's report to the user.
- **Records defect, root-owned: [R3/F1](agent-review-findings.md#03pa-p01-d1-r3f1--revision-labels-the-task-copy-and-c006r1-still-name-d1r2-where-they-mean-r3) (Low).**
  - The TASK heading and the immutable copy's internal heading and intro say "r2".
  - C006/r1's activation condition names D1/r2.
  - It is not a defect in the decision, and it does not reopen the gate. Root must still fix the TASK label, and cite this D1/r3 PASS when it activates C006, before dependent edits.
- **What the pass covers.** It accepts the architecture and the verification plan in D1.6/D1.7; it does not accept their execution. The B1 batch review must still establish the planned tests, competing-claimant checks, gates and before/after test lists. Any change to D1 reopens the gate.

### Process and instruction feedback

- **The round was small and complete.** A narrow delta round with an exact `diff` recipe made the recheck efficient and complete.
- **A labelling slip survived two hand-offs.** It was in TASK, in the immutable copy and in the coordinator's message. The candidate instruction is recorded in R3/F1.

## 03PA-P01-B1-R1

### Identity

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md) ("B1 implementation", "B1 tests and verification mapping", "B1 checks and review round 03PA-P01-B1-R1 inputs"), [REVIEW](REVIEW.md), [findings](agent-review-findings.md#round-03pa-p01-b1-r1-batch-review-of-p01-b1-under-d1r3).
- Batch and stage: p01 B1, the whole phase as resequenced by D1.1, in a batch review.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to me. I am the same B instance as in the D1 rounds, and I spawned no agents.
- Phase `03PA-P01`, worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`, branch `room/check-last-handoff-md-under-plan-02-work-qc8d`.
- Commits:
  - phase start and source review base: `cb46c6fd`;
  - HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`. That commit holds only the two `.trail/learnings` files, which I verified with `git show --stat`.
- Artifact: the working-tree delta plus three untracked new source files. Digest `888f0a805e0a1fa00b37892acf298493f573eca5897be111ba3fb9d7e255416c`, with exclusions `plan/03-principal-addresses/phases/p01/docs plan/03-principal-addresses/COORDINATION.md plan/02-workflow-roles/COORDINATION.md plan/02-workflow-roles/phases/p05/docs`. It matched at start and again before return.
- Gated decision: D1/r3 (`logs/D1-r3.md` sha256 `d5fd5109…d7ee`, unchanged).
- Coordination: revision 9 (sha256 `27e266bc2dd9b09979c177ce0b48d5a539078341829b512ca205c265f0f45cf0`). C006/r1 is active and cites the R3 pass. plan02 is still revision 41. Rechecked before return.
  - Applicable: I001/r1, I003/r1, and C001/r1 through C006/r1.
- User decision recorded in TASK: the two schema departures from decision 1's wording are accepted, which closes R1/F8.

### Coverage and evidence

**Requirements checked.**
- Plan 01, as resequenced by D1.1 and C006/r1, and D1/r3's D1.2–D1.7.
- Root, src, application, persistence, http, pages and smtp AGENTS.

**Inspected, with diffs read site by site.**
- Schema: the `addresses` DDL, the indexes, both FKs, `guard_address_principal` (`FOR KEY SHARE`, on INSERT or `UPDATE OF` the target columns), `addresses_follow_principal_kind`, and `require_channel_reply_proposal_links` with `company_id`. All match D1.2.
- Every moved desk reader and writer (`channel.rs`, `provisioning*.rs`, `message.rs`, `department*.rs`). Each filters on `company_id`. An exhaustive `rg` found no `channel_slugs` site left.
- `ensure_person_address_on`, which runs on every `create_person_principal_on` branch.
- `slugify`, `person_address_base` and `is_address_slug`.
- `address_targets`, a single `slug = ANY($2::citext[])` read, together with `DirectoryCache::load_addresses` and `load_addressed_namespace`.
- `resolve_addresses`:
  - a desk is resolved by id;
  - a principal gives the team-only refusal;
  - a stranger is recorded as `unknown` with the same suggestions;
  - the `kind()` precedence is unknown, then principal, then disabled.
- The policy, notices and bounce text; the `principals` list is empty for strangers.
- The relay: the `InternalDestination`/`RelayAuthorization` split, with a principal giving a definite `Refused`. The egress mapping is unchanged.
- The selector, ingress and transport renames.
- The profile path:
  - the persistence `UPDATE … FROM` is one statement;
  - the use case validates;
  - the route renders inline;
  - the company name, the slug and the messages are escaped;
  - the central same-origin check covers it.
- Changes to existing tests: SQL and identifier renames, with no assertion weakened.
- C001–C005: the agent desk, the notice route and the escalation code are unchanged. No `.invalid` identity and no `OutboundEmailV1 {` construction were added.

**Executed by me.**
1. `logs/B1-R1-review-focus.txt` (sha256 `9b53334e…31ed`). Filters `address_ relay egress selector ui_profile onboarding participant company_invite resend_api` on the B-owned disposable database `b_review_p01_b1`. Result: 246 passed, 0 failed, 1 ignored, including all 19 new `address_*` tests.
2. `logs/B1-R1-review-regress.txt` (sha256 `b521847e…a9e`). Filters `department provisioning escalation_ channel_reply integration simulation smtp sendgrid use_cases::thread company_agent agent_api channel`. Result: 1056 passed, 0 failed, 5 ignored.
3. `logs/B1-R1-review-gates.txt` (sha256 `a38dfaa9…7215`). With a real `rg` 14.1.1 on PATH, all seven CI boundary commands (`ci.yml:126-132`) passed, and so did `cargo fmt --check` and `git diff HEAD --check`.
4. `logs/B1-R1-review-edit-first-probe.txt` (sha256 `f9cfd06f…5029`), with its script beside it. On the real init schema, in a disposable database the script dropped, the edit-first rename against a demotion leaves no address and raises no error. The widened-guard variant also passes under a hold, which shows a hold test cannot detect the R1/F4 deadlock regression; see F2(a).

All B-owned databases are dropped.

**Checked from the author's evidence.**
- `check-evidence.py` validated the manifest: 10 PASS, 2 FAIL kept as history, 1 NOT EXERCISED.
- I recomputed the test-name delta: 4,493 to 4,512, with 0 removed and the 19 `address_*` names added.
- `.sqlx` is unchanged, as expected: only runtime queries were added.

I did not rerun clippy or `source_modules`; I accepted the author's logs for those.

**Verification mapping, checked against the assertions.**

| Area | Status |
| --- | --- |
| namespace, slug and resolution | decisive |
| SMTP stranger equality | decisive: it asserts the literal 550 lines plus the teammate's distinct line |
| Resend | adapter routing only |
| SendGrid | composition only (F2e) |
| self-edit against removal | removal-first only (F2a) |
| user deletion, department desk | not tested (F2b, F2c) |
| competing tests | shared database (F2d) |

**Reviewed lightly.** The profile page layout and styling (only feedback and escaping checked); the vault page, which passes an empty address list; and the `docs/system_addresses.md` one-line rename.

### Outcome

**Changes required. No PASS for digest `888f0a80…416c`.** There are three Low actionable findings and four optional notes. No production behaviour defect was found: everything I executed passed.

- [F1](agent-review-findings.md#03pa-p01-b1-r1f1--the-new-address-form-gives-no-progress-feedback) (Low): the address form has no progress feedback.
- [F2](agent-review-findings.md#03pa-p01-b1-r1f2--several-d16-checks-are-missing-or-do-not-exercise-what-they-promise) (Low): D1.6 coverage gaps (a)–(e).
- [F3](agent-review-findings.md#03pa-p01-b1-r1f3--resolve_addresses-grew-past-the-split-threshold-instead-of-extracting-the-new-branch) (Low): `resolve_addresses` extends a src/AGENTS split violation.
- Optional: F4 (`AddressSlug` newtype), F5 (pin the 409/400 classification), F6 (p02/p03 note: keep the relay sender check ahead of any principal delivery), F7 (coordination revision 8 text rewritten in place).

The disclosures are dispositioned:
- (a) the SendGrid gap is avoidable (F2e);
- (b) the profile convention is accepted (F5);
- (c) the removed `NotInternal` arm is confirmed unreachable at HEAD;
- (d) the in-check fixes, the fixture desk and the `Box`, are verified.

What the recheck should cover:
- the F1 markup;
- the F2 tests, including the trigger-scope assertion and a move to `own_database`, with new before/after test lists;
- the F3 extraction, by rerunning the address-resolution and thread suites;
- a new digest.

Everything else can be reused unless the correction touches it.

### Process and instruction feedback

- **Batch size was right.** The whole phase was reviewable in one pass, with about 1,200 changed lines.
- **The TASK mapping table was demonstrably useful.** It named the test, the decisive assertion and the disclosed gaps, and made checking each D1.6 promise mechanical.
- **Candidate lesson (verification-contracts):** "A competing-claimant test whose outcome does not depend on order proves the outcome, not the interleaving. Either assert the competitor actually blocked, on a database of its own, or say that only the outcome is proved. When a lock-order hazard is only a sub-millisecond race, guard the schema or code shape that prevents it with a deterministic structural assertion." Evidence: F2(a) and F2(d), and the edit-first probe.
- **Candidate lesson (src/AGENTS):** none new. F1 and F3 are existing rules that were missed; the rules are adequate.
- **Tooling:** the session's output compressor elided function bodies in diffs and file reads. Reading in narrow `awk`/`sed` ranges was needed to review the actual code. Candidate for the skill's reviewer guidance: "if tool output shows elision markers, re-read the exact ranges; never review elided bodies."

## 03PA-P01-B1-R2

### Identity

- Date: 2026-11-14. Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md) ("B1-R1 result and corrections"), [REVIEW](REVIEW.md), [findings](agent-review-findings.md#round-03pa-p01-b1-r2-recheck-of-the-b1-r1-corrections).
- Stage: p01 B1, correction recheck by finding ID.
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`). The effort setting is not visible to me. I am the same B instance, and I spawned no agents.
- Phase `03PA-P01`, same worktree and branch.
- Commits: HEAD `830561e2` (unchanged); source base `cb46c6fd`.
- Digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`, with the same four exclusions. It matched at start and again before return.
- Coordination: revision 10, sha256 `284a4c60d5db9a85d5389ca884c4e5c946567e66a6816e63069c598a2dd268fe`.
  - The revision 10 entry is appended; earlier entries are as at revision 9.
  - plan02 is still revision 41.
  - Applicable: I001/r1, I003/r1, and C001/r1–C006/r1.
- Gated decision: D1/r3, unchanged.

### Coverage and evidence

**Delta identity, checked independently.** The only source files modified after my R1 gates log are the five declared:
- `pages/profile.rs`
- `routing.rs`
- `address_tests.rs`
- `department_lead_desk_tests.rs`
- `sendgrid/inbound.rs`

**Inspected.** Each correction, against its R1 finding; dispositions are in the findings file. F3's semantics were compared case by case with R1.

**Executed by me.**
1. `logs/B1-R2-review-focus.txt` (sha256 `1a5c24e3…c8ad`), on B-owned disposable database `b_review_p01_b1`, since dropped.
   - Filters: `address_ department_lead_desk sendgrid use_cases::thread smtp relay ui_profile resend_api pages::tests department`.
   - Result: 488 passed, 1 failed, 5 ignored.
   - All 24 `address_*` tests pass, including the five new ones.
   - The one failure, `rendered_markup_carries_no_inline_javascript`, is pre-existing and unrelated. It flags the unchanged `workflow_authoring/sample.rs:153`, the scan logic is unchanged, and plan02 p05 TASK `:71` and `:83` record the same line. The author's correction runs omitted `pages::tests`.
2. `logs/B1-R2-review-gates.txt` (sha256 `c97a266f…708e`): with a real `rg` 14.1.1, all seven CI boundary commands passed, and so did `cargo fmt --check` and `git diff HEAD --check`.

**Checked from the author's evidence.**
- `check-evidence.py` validated `logs/03PA-P01-B1-R2.evidence.json`: 7 PASS, 2 FAIL kept as history, 1 NOT EXERCISED (F1 markup, which I verified by inspection).
- Test names: I recomputed 4,493 to 4,517 from `B1-tests-after2.txt`, with 0 removed.
- I did not rerun clippy (`B1-clippy4-diagnostic.txt` PASS) or `source_modules` (`B1-source-modules2.txt` PASS).
- No SQL changed in the correction, so the R1 migration and `.sqlx` evidence still applies.

**Reused from B1-R1 without re-review.** Every file outside the five, whose production behaviour I reviewed in R1 together with the 1,056-test regression run. That remains valid, because no other source file changed.

### Outcome

**PASS for digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`** (batch p01 B1, under D1/r3).

- **Coverage:** plan 01 as resequenced by C006, D1/r3's D1.2–D1.7, and C001–C006.
- **Checks:**
  - the R1 inspection and test runs: 246 + 1,056 passed;
  - the R2 focused run: 488 passed, plus one pre-existing unrelated failure;
  - the gates;
  - the author's clippy, `source_modules`, migration and `.sqlx` evidence.
- **Findings:** B1-R1/F1–F3 fixed; F4 deferred and F5 dismissed (optional); F6 and F7 closed.
- **Gaps that remain, stated plainly:**
  - the signed SendGrid and Resend checks are adapter-level stand-ins, without the DNS lookup or a route harness;
  - the agent-address paths are deferred to p03 under C006;
  - `rendered_markup_carries_no_inline_javascript` remains a pre-existing failure, outside this phase.

### Process and instruction feedback

- **The correction round was complete and narrow.** Every R1 finding had a mapped change, and the delta was exactly five files.
- **The reviewer's wider filter caught something the author's runs did not.** Including `pages::tests` surfaced a pre-existing failure that the author's correction runs omitted. It was harmless here, because it is unrelated and already recorded by plan02.
  - Candidate (skill, verification scope): "when a change touches a page's markup, include the page source-scan tests (`pages::tests`) in the focused set."
