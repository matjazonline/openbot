# 03PA-P01 review record

- Phase: `03PA-P01` (unified addresses). Workflow: `implement-opus-reviewed-2_3`.
- Records: [TASK](TASK.md), [HANDOFF](HANDOFF.md), [review notes](agent-review-notes.md), [findings](agent-review-findings.md).
- Worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`, branch `room/check-last-handoff-md-under-plan-02-work-qc8d`.

## Rounds

| Round | Date | Stage | Reviewed identity | Coordination | Outcome | Findings |
| --- | --- | --- | --- | --- | --- | --- |
| [03PA-P01-D1-R1](agent-review-notes.md#03pa-p01-d1-r1) | 2026-11-14 | Architecture gate, B1 | Decision **D1/r1** (`logs/D1-r1.md` sha256 `2105b82b…773c`); base/HEAD `cb46c6fd`; digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`; exclusions as in TASK "Status" | rev 7 (`05d8c4f6…6abd`): I001/r1, I003/r1, C001/r1–C005/r1; I002/r1 superseded | **Changes required; no PASS** | F1 (High); F2, F3, F4 (Medium); F5, F6, F7 (Low); F8, F9 (optional) |
| [03PA-P01-D1-R2](agent-review-notes.md#03pa-p01-d1-r2) | 2026-11-14 | Architecture gate recheck, B1 | Decision **D1/r2** (`logs/D1-r2.md` sha256 `f51249ee…3a81`); base/HEAD `cb46c6fd`; digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`; same exclusions | rev 8 (`f0a604f7…daca`): as rev 7, plus C006/r1 proposed | **Changes required; no PASS**. R1/F1–F7 resolved in substance; F8 deferred to the user report | R2/F1, R2/F2 (Low); R2/F3 (optional) |
| [03PA-P01-D1-R3](agent-review-notes.md#03pa-p01-d1-r3) | 2026-11-14 | Architecture gate delta recheck, B1 | Decision **D1/r3** (`logs/D1-r3.md` sha256 `d5fd5109…d7ee`); base/HEAD `cb46c6fd`; digest `eff341d36a36dddbf9b081322d91e1cc1a9f759425ab4546d812a9948be5d991`; same exclusions | rev 8 (`f0a604f7…daca`), unchanged | **PASS for D1/r3**. R2/F1 and R2/F2 fixed; R2/F3 adopted | R3/F1 (Low; a root-owned records defect, not a decision defect) |
| [03PA-P01-B1-R1](agent-review-notes.md#03pa-p01-b1-r1) | 2026-11-14 | Batch review, B1 (all of p01) | Source base `cb46c6fd`; HEAD `830561e2` (trail learnings only); digest `888f0a805e0a1fa00b37892acf298493f573eca5897be111ba3fb9d7e255416c`; same four exclusions; under D1/r3 | rev 9 (`27e266bc…f0cf`): C006/r1 active | **Changes required; no PASS**. No behaviour defect; B's runs: 246 + 1056 passed, 0 failed; gates pass | F1, F2, F3 (Low); F4–F7 (optional) |
| [03PA-P01-B1-R2](agent-review-notes.md#03pa-p01-b1-r2) | 2026-11-14 | Correction recheck, B1 | HEAD `830561e2`; digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`; same four exclusions; under D1/r3 | rev 10 (`284a4c60…68fe`), append-only | **PASS for digest `dc9329df…ca23`**. B1-R1/F1–F3 fixed; F4 deferred, F5 dismissed, F6 and F7 closed. B's run: 488 passed, plus 1 pre-existing unrelated failure | none new |

## Finding index (03PA-P01-D1-R1)

The full entries are in [agent-review-findings.md](agent-review-findings.md).

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| 03PA-P01-D1-R1/F1 | defect | High | Refusing only the principal while still admitting the desk needs an unspecified partial-delivery and bounce protocol. The existing ingress is all-or-nothing, SMTP answers with a synchronous 550, and outsiders would learn that the address exists. | fixed in substance (residual R2/F1) (R2) |
| 03PA-P01-D1-R1/F2 | defect | Medium | The internal relay resolves its target before `resolve_addresses`. A principal target becomes a retryable `AppError::Internal`, so "one change covers them all" is wrong. | fixed (R2) |
| 03PA-P01-D1-R1/F3 | defect | Medium | D1.1 leaves p02's agent-direct checks without the agent addresses they need, and publishes no coordination notice for the moved scope. | fixed (R2) |
| 03PA-P01-D1-R1/F4 | defect | Medium | Neither the guard's locking nor the edit path's lock order is specified. The house `FOR KEY SHARE` guard would deadlock a self-edit against a member removal, and there is no competing test. | fixed in substance (residual R2/F2) (R2) |
| 03PA-P01-D1-R1/F5 | defect | Low | Trimming before truncating can leave a trailing `-`, which the format check refuses, so company creation or joining fails. | fixed (R2) |
| 03PA-P01-D1-R1/F6 | defect | Low | The new primary key drops the `channel_id` index that `CHANNEL_SELECT`, the settings rewrite and cascades use. | fixed (R2) |
| 03PA-P01-D1-R1/F7 | defect | Low | I4 overstates what the schema proves. D1.6/D1.7 omit the promotion, rejoin and reuse paths and Resend, and lack the gates and mapping tables. | fixed (R2) |
| 03PA-P01-D1-R1/F8 | optional | Info | Report the resequencing and the schema deviations to the user; note the p03 slug-equality invariant. | closed: the user accepted the departures (recorded in TASK, noted at B1-R1) |
| 03PA-P01-D1-R1/F9 | optional | Info | Resolve through one read; note that the slug publishes the login username; list onboarding and department collisions as failure modes. | addressed (refinement R2/F3) (R2) |

## Finding index (03PA-P01-D1-R2)

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| 03PA-P01-D1-R2/F1 | defect | Low | The non-team indistinguishability claim does not cover the SMTP 550 reply line, which comes from `IngestRejection::as_str`/`UndeliverableKind`, and no assertion checks it. | fixed (R3) |
| 03PA-P01-D1-R2/F2 | defect | Low | D1.2 says a demotion takes `FOR NO KEY UPDATE`. In fact `kind` is a key column, so a demotion takes `FOR UPDATE` (probe log `logs/D1-R2-review-lock-probe.txt`). The chosen `FOR SHARE` is safe, but the rationale must be corrected. | fixed (R3) |
| 03PA-P01-D1-R2/F3 | optional | Info | Scope the one-read resolution to the message's slugs, so the work stays bounded. | adopted (R3) |

## Finding index (03PA-P01-D1-R3)

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| 03PA-P01-D1-R3/F1 | records (root) | Low | TASK has two different sections both headed "revision r2" (the r3 text is at line 365), and the r3 copy's own heading and intro say r2. C006/r1 activates on a "D1/r2 PASS". Fix the TASK label and cite the D1/r3 PASS when activating C006. | fixed (checked at B1-R1: TASK section retitled; COORDINATION rev 9 activates C006 citing R3) |

## Finding index (03PA-P01-B1-R1)

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| 03PA-P01-B1-R1/F1 | defect | Low | The address form has no `hx-disabled-elt` or "Saving..." feedback, unlike every sibling profile form (pages AGENTS). | fixed (R2) |
| 03PA-P01-B1-R1/F2 | defect | Low | D1.6 gaps. (a) Edit-first self-edit vs removal is untested, and the trigger scope that prevents the deadlock is unasserted. (b) User deletion is untested. (c) Department-desk creation at a person's address is untested. (d) The competing tests run on the shared `test_pool`. (e) An avoidable SendGrid NOT EXERCISED. | fixed (R2) |
| 03PA-P01-B1-R1/F3 | defect | Low | `resolve_addresses` grew from 101 to 117 lines and gained a nesting level, extending a src/AGENTS split violation; extract the per-selector classification. | fixed (R2) |
| 03PA-P01-B1-R1/F4 | optional | Info | An `AddressSlug` newtype for person slugs. | deferred (R2, optional) |
| 03PA-P01-B1-R1/F5 | optional | Info | Pin the use case's Conflict/BadRequest classification now that the route answers 200 inline. | dismissed (R2, optional) |
| 03PA-P01-B1-R1/F6 | optional | Info | For p02/p03: keep the relay's sender check ahead of any principal delivery. | closed (rev 10 carry-forward) |
| 03PA-P01-B1-R1/F7 | optional | Info | COORDINATION revision 9 rewrote the revision 8 note in place. | closed (rev 10 append-only) |

## Acceptance

B's review is complete for batch p01 B1. **PASS for digest `dc9329df080197b948e37372460f55476974a800811cb217182826193400ca23`** (round B1-R2), under D1/r3 (the R3 PASS).

Root still has to recompute the digest, recheck coordination, and decide acceptance. The recorded gaps:
- the SendGrid and Resend checks are adapter-level stand-ins;
- the agent-address paths are deferred to p03 under C006;
- `rendered_markup_carries_no_inline_javascript` is a pre-existing unrelated failure, also recorded in plan02 p05.

Any later substantive edit invalidates this pass.
