# P08 review record

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Owner: Reviewer B (`implement-opus-reviewed-2_2`). Task record: [`TASK.md`](TASK.md). Notes:
[`agent-review-notes.md`](agent-review-notes.md). Findings:
[`agent-review-findings.md`](agent-review-findings.md).

## Rounds

| Round | Date | Stage | Reviewed identity | Outcome | Findings |
| --- | --- | --- | --- | --- | --- |
| [WF-P08-R001](agent-review-notes.md#wf-p08-r001) | 2026-10-22 | Architecture gate P08-D1 rev 1 (B1 + D1.9 + D1.0) | decision sha256 `453bf4a0…7bb`; HEAD/base `da868710`; digest `e07e4b1d…47e9`; coordination r5 `38fd9afe…5c74` | **Changes required** | High: F001, F002, F003. Medium: F004, F005, F006, F007. Low: F008, F009, F010, F011. Optional: F012 |
| [WF-P08-R002](agent-review-notes.md#wf-p08-r002) | 2026-10-22 | Gate correction recheck P08-D1 rev 2 | decision sha256 `5271630f…0c63a`; HEAD/base `da868710`; digest `e07e4b1d…47e9`; coordination r5 `38fd9afe…5c74` | **Changes required** | F001–F011 fixed. New: Medium F013, F014. Low F015, F016, F017. Optional F018 |
| [WF-P08-R003](agent-review-notes.md#wf-p08-r003) | 2026-10-22 | Gate correction recheck P08-D1 rev 3 | decision sha256 `7c4b9457…21aaf7`; HEAD/base `da868710`; digest `e07e4b1d…47e9`; coordination r6 `23fdc190…c4db` | **PASS** | F013–F017 fixed (F016.3 dismissed); F018 adopted. New: Low F019 (B1 implementation check), optional F020 |
| [WF-P08-R004](agent-review-notes.md#wf-p08-r004) | 2026-10-22 | B1 batch review | base/HEAD `da868710` + working tree; digest `90d4e0ab…7390`; contract D1 rev 3; coordination r6 | **Changes required** | Medium F021. Low F022, F023, F024, F025, F026. Optional F027. Carried: F019 fixed, F020 items 1–2 adopted |
| [WF-P08-R005](agent-review-notes.md#wf-p08-r005) | 2026-10-22 | B1 correction recheck | base/HEAD `da868710` + working tree; digest `500ae85b…1b11`; contract D1 rev 3; coordination r6 | **Changes required** | F021–F025 fixed; F026 by deviation D-5; F027 adopted. New: Medium F028, Low F029 (verification gap) |
| [WF-P08-R006](agent-review-notes.md#wf-p08-r006) | 2026-10-23 | B1 correction recheck | base/HEAD `da868710` + working tree; digest `b90a7f9c…a468`; contract D1 rev 3; coordination r6 | **PASS** | F028 fixed; F029 fixed (root cause: sqlx 0.8.6 cancelled `begin` leak; pool guard). New: optional F030 (Info) |
| [WF-P08-R007](agent-review-notes.md#wf-p08-r007) | 2026-10-23 | B2 batch review | base/HEAD `da868710` + working tree; digest `7b932911…5c51`; contract D1 rev 3 §D1.9; coordination r8 `0950b907…5e22` | **Changes required** | F030 fixed (WF-C004 r2 exemption accepted). New: Low F031, F032, F033; optional F034 |
| [WF-P08-R008](agent-review-notes.md#wf-p08-r008) | 2026-10-23 | B2 correction recheck | base/HEAD `da868710` + working tree; digest `e43d8c37…b94f`; coordination r8 | **PASS** | F031, F032, F033 fixed; F034 adopted. New: optional F035 (Info) |
| [WF-P08-R009](agent-review-notes.md#wf-p08-r009) | 2026-10-23 | Architecture gate P08-D2 rev 1 (B3a) | decision sha256 `dfa1bf4d…0c07`; facts at B2 digest `e43d8c37…b94f`; coordination r8 | **Changes required** | Medium F036, F037, F038. Low F039–F044. Info F045 |
| [WF-P08-R010](agent-review-notes.md#wf-p08-r010) | 2026-10-23 | Gate recheck P08-D2 rev 2 | decision sha256 `b88fe8ad…8afd`; tree `d98a8fd7…8f28`; coordination r8 | **PASS** | F036–F044 resolved; F045 adopted. New: Low F046, F047, F048 (carried to B3a); Info F049 |
| [WF-P08-R011](agent-review-notes.md#wf-p08-r011) | 2026-10-23 | B3a batch review | base/HEAD `da868710` + working tree; digest `9bdad3da…1f3c`; contract D2 rev 2; coordination r8 | **Changes required** | F035, F046, F047, F048 fixed; F049 adopted; DV-1..6 accepted. New: Medium F051, Low F050 |
| [WF-P08-R012](agent-review-notes.md#wf-p08-r012) | 2026-10-23 | B3a correction recheck | base/HEAD `da868710` + working tree; digest `22334ec4…77e1`; coordination r8 | **PASS** | F050, F051 fixed. No new findings |
| [WF-P08-R013](agent-review-notes.md#wf-p08-r013) | 2026-10-23 | B3b-1 batch review | base/HEAD `da868710` + working tree; digest `22923f7f…2c20`; coordination r8 `0950b907…5e22` | **Changes required** | DV-7..DV-10 accepted. New: Medium F054. Low F052, F053, F055, F056, F057, F058. Info F059, F060, F061 |
| [WF-P08-R014](agent-review-notes.md#wf-p08-r014) | 2026-10-23 | B3b-1 correction recheck | base/HEAD `da868710` + working tree; digest `b075aa52…11ef`; coordination r8 | **Changes required** | F052–F059, F061 fixed; F060 partly (→ F062). New: Low F062. Info F063, F064 |
| [WF-P08-R015](agent-review-notes.md#wf-p08-r015) | 2026-10-23 | B3b-1 correction recheck | base/HEAD `da868710` + working tree; digest `57566f92…dd33`; coordination r8 | **PASS** | F062 fixed (F060 completed); F063 resolved by disposition; F064 accepted. No new findings |
| [WF-P08-R016](agent-review-notes.md#wf-p08-r016) | 2026-10-23 | Architecture gate P08-D3 rev 1 (B3b-2) | decision sha256 `121779fc…25d4`; facts at digest `57566f92…dd33`; coordination r9 `e636593b…` | **Changes required** | Medium F065, F066. Low F067, F068, F069. Info F070 |
| [WF-P08-R017](agent-review-notes.md#wf-p08-r017) | 2026-10-23 | Gate recheck P08-D3 rev 2 | decision sha256 `0cacf996…558c`; facts at digest `57566f92…dd33`; coordination r9 | **PASS** | F065–F069 resolved; F070 adopted. New: Low F071 (carried to B3b-2); Info F072 |
| [WF-P08-R018](agent-review-notes.md#wf-p08-r018) | 2026-10-23 | B3b-2 batch review | base/HEAD `da868710` + working tree; digest `4156165b…20b3`; contract D3 rev 2; coordination r9 | **PASS** | F071 fixed; F072 adopted; DV-11..13 accepted. New: Info F073 |
| [WF-P08-R019](agent-review-notes.md#wf-p08-r019) | 2026-10-23 | Architecture gate P08-D4 rev 1 (B4) | decision sha256 `2f46c343…647d`; facts at digest `4156165b…20b3`; coordination r10 `1df326af…` | **Changes required** | Medium F074, F075, F080. Low F076, F077, F078, F079, F081, F082. Info F083 |
| [WF-P08-R020](agent-review-notes.md#wf-p08-r020) | 2026-10-23 | Gate recheck P08-D4 rev 2 | decision sha256 `d88aeb86…3bce`; facts at digest `4156165b…20b3`; coordination r10 | **PASS** | F074–F082 resolved; F083 adopted. New: Low F084 (carried to B4a); Info F085 |
| [WF-P08-R021](agent-review-notes.md#wf-p08-r021) | 2026-10-24 | B4a batch review | base/HEAD `da868710` + working tree; digest `5b27364a…2c16`; contract D4 rev 2; coordination r10 | **Changes required** | F084 fixed; F085 adopted. New: Low F086; Info F087 |
| [WF-P08-R022](agent-review-notes.md#wf-p08-r022) | 2026-10-24 | B4a correction recheck | base/HEAD `da868710` + working tree; digest `a5440048…20c8`; coordination r10 | **PASS** | F086 fixed; F087 resolved. No new findings |
| [WF-P08-R023](agent-review-notes.md#wf-p08-r023) | 2026-10-24 | B4b batch review | base/HEAD `da868710` + working tree + index (B4b `git rm`); digest `ac0a8958…63c7`; coordination r10 | **Changes required** | New: Medium F088; Low F089–F092; Info F093. DV-B4b-1..6, 8 accepted; DV-B4b-7 except F088 |
| [WF-P08-R024](agent-review-notes.md#wf-p08-r024) | 2026-10-24 | B4b correction recheck | base/HEAD `da868710` + working tree + index; digest `c4b7eaa9…c896`; coordination r10 | **PASS** | F088–F092 fixed; F093 resolved. New: Low F094 (pre-existing intermittent lease test, carried to the phase gate) |
| [WF-P08-R025](agent-review-notes.md#wf-p08-r025) | 2026-10-24 | B4c-1 batch review (memory, P08-Q1) | base/HEAD `da868710` + working tree + index; digest `94139bfd…1e11`; coordination r10 | **PASS** | New: Low F095 (two stale `.sqlx` files from B4b, carried to B4c-3); Info F096 |
| [WF-P08-R026](agent-review-notes.md#wf-p08-r026) | 2026-10-24 | B4c-2 batch review (channel agents, agent runtime config) | base/HEAD `da868710` + working tree + index; digest `97531db8…802d`; coordination r10 | **Changes required** | F095 fixed; F096.1 fixed. New: Medium F097 (gate eligibility claim unimplemented; test checks mechanics only); Info F098 |
| [WF-P08-R027](agent-review-notes.md#wf-p08-r027) | 2026-10-24 | B4c-2 correction recheck | base/HEAD `da868710` + working tree + index; digest `2d4ea381…7098`; coordination r10 | **Changes required** | F097 fixed (revision + gate, real-commit competing tests); F098 resolved. New: Low F099 (commit-first test edit lock budget 2 s); Info F100 |
| [WF-P08-R028](agent-review-notes.md#wf-p08-r028) | 2026-10-24 | B4c-2 F099 recheck | base/HEAD `da868710` + working tree + index; digest `95fe3918…cbe5`; coordination r10 | **PASS** | F099 fixed; F100 resolved. No new findings |
| [WF-P08-R029](agent-review-notes.md#wf-p08-r029) | 2026-10-24 | B4c-3a batch review (legacy runtime schema) | base/HEAD `da868710` + working tree + index; digest `fad11e27…1dde`; coordination r10 | **PASS** | New: Info F101. DV-B4c3-1..7 accepted |
| [WF-P08-R030](agent-review-notes.md#wf-p08-r030) | 2026-10-24 | B4c-3b batch review (runtime metrics retarget, F101.2) | base/HEAD `da868710` + working tree + index; digest `845d5596…e1d6`; coordination r10 | **PASS** | F101.1 adopted, F101.2 fixed. New: Info F102 |
| [WF-P08-R031](agent-review-notes.md#wf-p08-r031) | 2026-10-24 | B4d batch review (phase close-out) + phase acceptance | base/HEAD `da868710` + working tree + index; digest `4ec81c1a…0fc1`; coordination r10 | **Changes required** | F094 fixed; F101, F102 resolved. New: Low F103 (rig.md `max_turns`, deploy.md `read_resource`), Low F104 (`mcp_connections_by_ids` obligation), Info F105 (acceptance matrix, gate notes) |
| [WF-P08-R032](agent-review-notes.md#wf-p08-r032) | 2026-10-24 | B4d F103/F104 correction recheck | base/HEAD `da868710` + working tree + index; digest `00ec454f…6829`; coordination r10 | **PASS** | F103, F104 fixed; F105 adopted. No new findings |

## Open findings (latest disposition)

| ID | Severity | Summary | Status |
| --- | --- | --- | --- |
| [WF-P08-F001](agent-review-findings.md#wf-p08-f001--d13-which-associations-answer-and-so-get-an-intent-and-keep-an-external-audience-is-undefined-and-still-wired-to-the-legacy-channel-trigger) | High | B1's commit-time fold is undefined and coupled to the legacy channel trigger; it decides both intent creation and stored audience | fixed (R002, rev 2) |
| [WF-P08-F002](agent-review-findings.md#wf-p08-f002--d15-a-reply-to-any-outbound-message-of-an-active-run-is-swallowed-whenever-the-run-declares-any-messagereply-wait) | High | `record_event` matches on event name only, so replies no wait will consume are swallowed and admit nothing | fixed (R002, rev 2) |
| [WF-P08-F003](agent-review-findings.md#wf-p08-f003--d15-wait-matching-drops-the-responder-identity-criterion-and-ingress-refuses-the-replies-the-design-needs) | High | No responder-identity match (a wrong party can satisfy a wait); member email replies to `user` sends are bounced as thread injection | fixed (R002, rev 2) |
| [WF-P08-F004](agent-review-findings.md#wf-p08-f004--d15-dedup-order-contradicts-the-stated-crash-guarantee-deliver_reply-errors-and-messagereply-payload-schemas-are-unconstrained) | Medium | Wrong dedup order (crash replay admits again); `deliver_reply` errors unclassified; `message.reply` schema unconstrained | fixed (R002, rev 2) |
| [WF-P08-F005](agent-review-findings.md#wf-p08-f005--d14d19-transient-admission-conflicts-are-classified-as-permanent-refusals-and-a-transient-failure-starves-later-sibling-bindings) | Medium | `Conflict` from a concurrent reconfigure becomes a permanent refusal; a transient failure stops later siblings | fixed (R002, rev 2) |
| [WF-P08-F006](agent-review-findings.md#wf-p08-f006--d11-run-as-selection-weakens-the-existing-admin-may-only-attribute-automation-to-themselves-rule-and-silently-re-attributes-on-reconfigure) | Medium | Run-as drops the admin-self-only rule and stored-attribution preservation; "visible" refusal surface not named | fixed (R002, rev 2) |
| [WF-P08-F007](agent-review-findings.md#wf-p08-f007--d10-the-batch-plan-omits-the-06c-obligation-to-ship-create_agent_channel-and-list_company_agents-as-workflow-tools) | Medium | 06C workflow tools `create_agent_channel` / `list_company_agents` are not scheduled | fixed (R002, rev 2) |
| [WF-P08-F008](agent-review-findings.md#wf-p08-f008--d13d14-intent-queue-protocol-is-under-specified-fence-token-outcome-fencing-work-bound-vs-lease-fairness-tenant-fks) | Low | Fence token, outcome fencing, binding bound vs lease, fairness ranking, tenant FKs | fixed (R002, rev 2) |
| [WF-P08-F009](agent-review-findings.md#wf-p08-f009--d17-in_app_only-shape-for-user-destinations-conflicts-with-the-destination-check) | Low | `in_app_only` user destination conflicts with the destination CHECK | fixed (R002, rev 2) |
| [WF-P08-F010](agent-review-findings.md#wf-p08-f010--d18-pool-budget-shutdown-bridge-and-lifecycle-tests-are-missing-from-the-composition-contract) | Low | Worker slots vs pool, shutdown bridge, lifecycle tests | fixed (R002, rev 2) |
| [WF-P08-F011](agent-review-findings.md#wf-p08-f011--d19-authority-visibility-revalidation-and-verification-gaps-in-the-schedule-contract) | Low | D1.9 authority, channel visibility, revalidation, tests | fixed (R002, rev 2) |
| [WF-P08-F012](agent-review-findings.md#wf-p08-f012--optional-suggestions-non-blocking) | Optional | Activation timing, trace retention, assistant setup, WF-C002 for B2, B2 size | items 1, 2, 4, 5 adopted; 3 deferred to D2 (R002) |
| [WF-P08-F013](agent-review-findings.md#wf-p08-f013--d11-rev-2-the-composite-fk-from-run_as_user_id-to-company-members-blocks-removing-any-member-who-was-ever-a-run-as) | Medium | The run-as composite FK to company members blocks member removal; contradicts P08-Q2 | fixed (R003, rev 3) |
| [WF-P08-F014](agent-review-findings.md#wf-p08-f014--d14d15-rev-2-notyet-deferrals-consume-the-5-attempt-poison-budget-and-can-fail-the-intent-before-the-5-minute-grace-ends) | Medium | `NotYet` deferrals spend the 5-attempt budget, so the intent can fail before the grace ends | fixed (R003, rev 3) |
| [WF-P08-F015](agent-review-findings.md#wf-p08-f015--d18-rev-2-the-pool-check-refuses-the-repositorys-default-configuration) | Low | The pool check (≥12) rejects the default pool of 10, so default startup fails | fixed (R003, rev 3) |
| [WF-P08-F016](agent-review-findings.md#wf-p08-f016--d15-rev-2-remaining-windows-where-an-awaited-reply-is-swallowed-or-refused) | Low | Open-wait vs expiry race, expiry beats a recorded event, allowlist ACL on user-send replies | items 1–2 fixed; 3 dismissed (R003) |
| [WF-P08-F017](agent-review-findings.md#wf-p08-f017--d12-rev-2-the-8-binding-bound-is-checked-only-at-activation-so-configure-bypasses-it) | Low | The 8-binding bound can be bypassed through configure of an active binding | fixed (R003, rev 3) |
| [WF-P08-F018](agent-review-findings.md#wf-p08-f018--optional-suggestions-from-the-recheck-non-blocking) | Optional | Correlation-shape publication rule; stale B2→B3 references in TASK/COORDINATION | adopted (R003) |
| [WF-P08-F019](agent-review-findings.md#wf-p08-f019--d155b-rev-3-locking-the-wait-row-before-the-run-row-inverts-resume_ons-lock-order) | Low | `deliver_reply` must lock the run row before the wait row (current order deadlocks with expiry); mandatory B1 batch-review check | fixed in B1 (R004) |
| [WF-P08-F020](agent-review-findings.md#wf-p08-f020--optional-precision-notes-on-rev-3-non-blocking) | Optional | Backoff `k`, event `created_at` vs check instant, run-deadline window | items 1–2 adopted; 3 → F027.4 (R004) |
| [WF-P08-F021](agent-review-findings.md#wf-p08-f021--responder-identity-for-reply-and-channel-destinations-is-untested) | Medium | Responder rules for `reply`/`channel` destinations untested (only `user`) | fixed (R005) |
| [WF-P08-F022](agent-review-findings.md#wf-p08-f022--envexample-still-documents-a-pool-default-the-new-startup-check-refuses) | Low | `.env.example` still has pool 10 (refused with default slots); `WORKFLOW_WORKER_SLOTS` undocumented | fixed (R005) |
| [WF-P08-F023](agent-review-findings.md#wf-p08-f023--a-terminal-intent-failure-is-neither-logged-nor-counted) | Low | Intent `failed` transitions emit no log or metric | fixed (R005) |
| [WF-P08-F024](agent-review-findings.md#wf-p08-f024--a-retried-intent-can-resume-a-wait-after-it-has-already-admitted-runs) | Low | A retry re-offers the reply after admissions, so it can both match and admit | fixed (R005) |
| [WF-P08-F025](agent-review-findings.md#wf-p08-f025--verification-gaps-against-the-contracts-test-list) | Low | Tenant FK, policy replay, intent→run policy and D-3 refusal-branch tests missing | fixed (R005) |
| [WF-P08-F026](agent-review-findings.md#wf-p08-f026--templatecatalogue-from-the-gated-composition-is-neither-composed-nor-recorded-as-a-deviation) | Low | `TemplateCatalogue` neither composed nor recorded as a deviation | resolved by deviation D-5 (R005) |
| [WF-P08-F027](agent-review-findings.md#wf-p08-f027--optional-suggestions-from-the-b1-review-non-blocking) | Optional | Stale doc, Resend wakeup, HTTP private origins, run-deadline window doc | adopted (R005) |
| [WF-P08-F028](agent-review-findings.md#wf-p08-f028--a-wall-clock-timeout-in-deliver_reply-is-treated-as-cannot-be-delivered-so-an-awaited-reply-is-permanently-routed-away-from-its-wait) | Medium | `deliver_reply` wall-clock timeout (`AppError::Conflict`) classified `Unmatched` and settled | fixed (R006) |
| [WF-P08-F029](agent-review-findings.md#wf-p08-f029--nondeterministic-failure-of-the-allowlist-e2e-test-under-a-parallel-focused-run) | Low | Allowlist e2e test failed once in 6 parallel runs; cause not established | fixed (R006): sqlx cancelled-`begin` leak, `after_release` guard |
| [WF-P08-F030](agent-review-findings.md#wf-p08-f030--ad-hoc-test-pools-bypass-the-leaked-transaction-guard-optional) | Optional | Ten test-local `PgPoolOptions::new()` pools bypass `discard_leaked_transactions` | fixed (R007): `pool_options()`; `lease_resolution` exempt under WF-C004 r2 |
| [WF-P08-F031](agent-review-findings.md#wf-p08-f031--nested-schedule-routes-ignore-the-path-channel-and-the-new-routes-have-no-other-tenant-tests) | Low | Nested schedule routes ignore the path `channel_id`; no other-tenant tests for `/runs`, `/schedule-bindings`, run-now | fixed (R008) |
| [WF-P08-F032](agent-review-findings.md#wf-p08-f032--deleted-legacy-schedule-tests-leave-live-behaviours-unguarded-b2-g-and-the-lease-fence-change-untested-or-unrecorded) | Low | Poison batch, cadence-unchanged edit and one-off disable tests not ported; no B2-g test; lease-fence change unrecorded | fixed (R008) |
| [WF-P08-F033](agent-review-findings.md#wf-p08-f033--schedule-occurrence-terminal-failure-and-lease-loss-are-neither-counted-nor-logged-as-such) | Low | Occurrence terminal failures and lease loss are neither counted nor logged distinctly (recurs F023) | fixed (R008) |
| [WF-P08-F034](agent-review-findings.md#wf-p08-f034--optional-suggestions-from-the-b2-review-non-blocking) | Optional | Live runs column now static; stale schedule docs; test leftover | adopted (R008) |
| [WF-P08-F035](agent-review-findings.md#wf-p08-f035--the-expired-occurrence-sweep-can-lose-a-terminal-failures-log-and-metric-optional) | Optional | The sweep logs and counts only after its schedule `last_error` updates succeed | fixed (R011) |
| [WF-P08-F036](agent-review-findings.md#wf-p08-f036--the-provisioned-binding-runs-as-the-builders-run-as-not-as-the-human-who-asked) | Medium | Provisioned binding runs as the Builder run-as, not the requester (bypasses D1.1 admin-self rule) | resolved (R010, rev 2) |
| [WF-P08-F037](agent-review-findings.md#wf-p08-f037--a-message-root-requester-can-be-established-by-email-which-the-plan-does-not-accept-for-privileged-human-authority) | Medium | Message-root requester accepted from email (DMARC) origin; require `application` origin | resolved (R010, rev 2) |
| [WF-P08-F038](agent-review-findings.md#wf-p08-f038--no-unvalidated-path-is-shown-for-the-bundle-only-the-agent-channel-and-binding-validations-that-one-transaction-bypasses-are-not-named) | Medium | Agent/channel/binding service validations and publish checks bypassed by the one transaction are not enumerated | resolved (R010, rev 2) |
| [WF-P08-F039](agent-review-findings.md#wf-p08-f039--list_company_agents-gives-any-member-of-the-builder-channel-what-the-app-shows-only-to-owners-and-admins) | Low | `list_company_agents` discloses the agent directory to any Builder-channel member | resolved (R010, rev 2) |
| [WF-P08-F040](agent-review-findings.md#wf-p08-f040--the-d22-local-action-path-leaves-authority-composition-refusal-mapping-and-the-read-tools-path-unspecified) | Low | Local path: authority composition, refusal mapping, read-tool path, reconciliation unspecified | resolved (R010, rev 2) |
| [WF-P08-F041](agent-review-findings.md#wf-p08-f041--ui-command-replay-and-conflict-are-inferred-from-mutable-rows-with-no-stated-ordering) | Low | UI replay/conflict inferred from mutable rows; ordering unstated; no command record | resolved (R010, rev 2) |
| [WF-P08-F042](agent-review-findings.md#wf-p08-f042--the-reserved-name-rule-rests-on-an-inaccurate-premise-and-covers-too-few-registration-paths) | Low | Reserved names: HTTP premise inaccurate; MCP/registry/trusted-check sites missing; `tools` propagation undecided | resolved (R010, rev 2) |
| [WF-P08-F043](agent-review-findings.md#wf-p08-f043--the-verification-plan-has-no-competing-claimant-tests-for-the-new-protocol) | Low | No competing-claimant tests for the new local-action and UI idempotency protocol | resolved (R010, rev 2) |
| [WF-P08-F044](agent-review-findings.md#wf-p08-f044--the-builders-new-grants-and-explicit-model-do-not-reach-a-company-builder) | Low | Library Builder grants and explicit model do not reach a company Builder | resolved (R010, rev 2) |
| [WF-P08-F045](agent-review-findings.md#wf-p08-f045--optional-notes-on-d25-and-the-batch-split-non-blocking) | Optional | `/run/deadline` canonical rendering and docs; split sound; compile under company lock | adopted (R010, rev 2) |
| [WF-P08-F046](agent-review-findings.md#wf-p08-f046--refusals-must-be-classified-by-deterministic-site-never-by-apperror-variant-from-reused-functions) | Low | Refusals must come from deterministic sites, not `AppError` variants (permanent receipts) | fixed (R011) |
| [WF-P08-F047](agent-review-findings.md#wf-p08-f047--with-the-narrower-classification-a-provisioning-grant-can-resolve-to-a-third-party-mcp-tool-of-the-same-name) | Low | `granted_tool` may resolve a provisioning grant to a captured MCP tool of the same name | fixed (R011) |
| [WF-P08-F048](agent-review-findings.md#wf-p08-f048--requester-derivation-should-match-the-stored-trigger-source-exhaustively-and-fail-closed) | Low | Requester derivation by intent existence fails open; match the stored source exhaustively | fixed (R011) |
| [WF-P08-F049](agent-review-findings.md#wf-p08-f049--optional-notes-on-rev-2-non-blocking) | Optional | 422 vs project mapping; deterministic "first model"; same key by another admin | adopted (R011) |
| [WF-P08-F050](agent-review-findings.md#wf-p08-f050--list_company_agents-bounds-the-agent-count-but-not-the-receipt-size-so-a-large-company-fails-the-call-deterministically) | Low | `list_company_agents` receipt has no byte bound against the 64 KiB result cap; no truncation report | fixed (R012) |
| [WF-P08-F051](agent-review-findings.md#wf-p08-f051--the-created-channels-address-omits-the-app-domain-on-every-path) | Medium | Created channel address is `{slug}@{company}` without `.{app_domain}` on every path | fixed (R012) |
| [WF-P08-F052](agent-review-findings.md#wf-p08-f052--workflow-listings-silently-cut-at-200-rows-and-visibility-is-filtered-after-the-cut) | Low | Listings cut at 200 with no truncation report; visibility filtered after the cut; oldest 200 executions kept | fixed (R014) |
| [WF-P08-F053](agent-review-findings.md#wf-p08-f053--per-thread-visibility-is-a-query-loop-and-two-new-lookups-have-no-supporting-index-or-recorded-evidence) | Low | Per-thread visibility loop (~600 round trips per mail-binding page); id-only decision lookup and run sort lack index or recorded evidence/metric | fixed (R014) |
| [WF-P08-F054](agent-review-findings.md#wf-p08-f054--the-validate-route-cannot-validate-any-draft-that-uses-an-http-or-mcp-operation) | Medium | `validate` passes no HTTP/MCP selections, so every HTTP/MCP draft gets a false 422 `registry.tool` | fixed (R014) |
| [WF-P08-F055](agent-review-findings.md#wf-p08-f055--a-publish-retry-under-the-same-key-with-different-httpmcp-selections-replays-the-old-version-instead-of-409) | Low | Same publish key with different selections replays the old version (200), not 409 | fixed (R014) |
| [WF-P08-F056](agent-review-findings.md#wf-p08-f056--the-planned-wf-c002-child-control-rule-is-neither-implemented-nor-recorded-and-the-retry-refusal-message-is-wrong-for-most-of-the-cases-it-covers) | Low | Planned child-through-root control rule not implemented or recorded; retry 409 message wrong; no runtime-child test | fixed (R014) |
| [WF-P08-F057](agent-review-findings.md#wf-p08-f057--the-grant-route-lets-any-agent-including-a-provisioned-one-be-granted-the-provisioning-tools) | Low | Grant route accepts provisioning names for any agent, against D2 invariant 6 | fixed (R014) |
| [WF-P08-F058](agent-review-findings.md#wf-p08-f058--the-decision-comment-route-and-the-two-ui-decision-mutations-have-no-other-tenant-test) | Low | No other-tenant test for the JSON comment and the UI submit/comment routes | fixed (R014) |
| [WF-P08-F059](agent-review-findings.md#wf-p08-f059--the-workflow-routes-set-no-explicit-body-limit-optional) | Info | No explicit body limit on the workflow routes (implicit 2 MiB) | fixed (R014) |
| [WF-P08-F060](agent-review-findings.md#wf-p08-f060--decision-page-inert-daisyui-v4-classes-partial-busy-state-untested-ui-success-path-optional) | Info | Decision page: inert `form-control`, no `aria-busy`/relabel on submit, UI success untested | fixed (R015) |
| [WF-P08-F061](agent-review-findings.md#wf-p08-f061--grant-protocol-notes-optional) | Info | Grant retry after success is 409; unregistered names grantable; validation before auth (no disclosure) | fixed (R014) |
| [WF-P08-F062](agent-review-findings.md#wf-p08-f062--the-decision-pages-new-submitting-status-uses-a-utility-the-committed-css-lacks-it-never-shows-and-cis-css-check-fails) | Low | `[.htmx-request_&]:inline-flex` not in committed `app.css`: busy status never shows; `npm run check:css` fails | fixed (R015) |
| [WF-P08-F063](agent-review-findings.md#wf-p08-f063--the-child-control-rule-lives-only-in-the-http-route-and-its-stated-reason-does-not-hold-optional) | Info | Child-control rule only in the route; the cascade and p07's child cancel do not use the service, so it could move there | resolved by disposition (R015) |
| [WF-P08-F064](agent-review-findings.md#wf-p08-f064--paging-notes-next-can-name-a-hidden-record-the-definitions-cursor-follows-a-mutable-title-and-the-probe-plans-are-index-only-optional) | Info | `next` may name a hidden row; title-based definitions cursor; probe plans index-only | accepted (R015) |
| [WF-P08-F065](agent-review-findings.md#wf-p08-f065--d31-dispatches-on-the-target-variant-so-every-unslotted-http-action-a-normal-production-case-is-refused) | Medium | D3.1 refuses `Local{http}` (unslotted HTTP) by dispatching on the target variant | resolved (R017, rev 2) |
| [WF-P08-F066](agent-review-findings.md#wf-p08-f066--the-routes-outcome-to-status-contract-contradicts-the-existing-command-and-the-verification-plan-asserts-the-contradiction) | Medium | Route status table contradicts the command: conflicts are `Ok` outcomes (some recorded); fresh-key rule; plan asserts 409 | resolved (R017, rev 2) |
| [WF-P08-F067](agent-review-findings.md#wf-p08-f067--the-discovery-read-is-not-enough-to-form-a-valid-message-delivery-command-or-to-tell-which-markers-are-uncertain) | Low | Discovery read lacks delivery status and receipt presence; lists `local` markers | resolved (R017, rev 2) |
| [WF-P08-F068](agent-review-findings.md#wf-p08-f068--reconciliation-requires-the-connection-to-be-ready-now-the-failure-modes-and-tests-omit-it) | Low | Reconciliation requires current readiness (disabled/secret/unregistered → 404); unstated, untested | resolved (R017, rev 2) |
| [WF-P08-F069](agent-review-findings.md#wf-p08-f069--the-verification-plan-does-not-prove-the-new-composition-on-every-target-or-the-competing-cases-precisely) | Low | Verification misses the new composition per target, exact competing interleavings, same-key race, MCP | resolved (R017, rev 2) |
| [WF-P08-F070](agent-review-findings.md#wf-p08-f070--notes-on-d32-d33-and-d35-optional) | Info | D3.2 accepted (cancel + rerun); Timeout → 500; child rule accepted; lock order confirmed | adopted (R017) |
| [WF-P08-F071](agent-review-findings.md#wf-p08-f071--replay-and-timeout-are-stated-more-strongly-than-the-command-guarantees-which-reverses-the-retry-rule-after-a-503-carried-to-b3b-2) | Low | Replay needs current access/readiness; a timeout may follow a committed settle: retry 503 with the same key (vs new key after 409) | fixed (R018) |
| [WF-P08-F072](agent-review-findings.md#wf-p08-f072--rev-2-precision-notes-optional) | Info | `evidence_conflict` is post-record; revision per evidence row; destination flag narrower than the subject check | adopted (R018) |
| [WF-P08-F073](agent-review-findings.md#wf-p08-f073--b3b-2-test-and-observability-notes-optional) | Info | Race-test status/refusal-row tightening; foreign marker pin 404; 503 branch unlogged; 422 body errors; message test asserts only `is_err` | open (R018) |
| [WF-P08-F074](agent-review-findings.md#wf-p08-f074--d43s-per-reviewer-decision-notifications-do-not-fit-the-notification-model-and-nothing-withdraws-them) | Medium | Per-reviewer decision notifications vs one-recipient-per-source model; no withdrawal events; NULL-channel rule | resolved (R020, rev 2) |
| [WF-P08-F075](agent-review-findings.md#wf-p08-f075--delivery-failure-recipients-and-branches-are-under-specified-and-the-b4c-column-drop-leaves-runtime-only-failures-in-plpgsql) | Medium | Delivery recipients multi/undefined; `delivery_failure` branch not unchanged; runtime-only plpgsql refs to dropped `task_id` | resolved (R020, rev 2) |
| [WF-P08-F076](agent-review-findings.md#wf-p08-f076--a-conditional-notice-guard-loses-the-provable-notice-invariant-unless-null-is-tied-to-a-recorded-opt-out) | Low | Conditional notice guard loses provable-notice invariant; tie NULL to recorded opt-out | resolved (R020, rev 2) |
| [WF-P08-F077](agent-review-findings.md#wf-p08-f077--a-listen-reconnect-loses-wakes-silently-d410s-the-reconnect-logic-emits-lag-does-not-hold) | Low | LISTEN reconnect drops wakes silently (`recv()`); D4.10 claim false | resolved (R020, rev 2) |
| [WF-P08-F078](agent-review-findings.md#wf-p08-f078--the-attention-branches-need-a-multi-reviewer-row-model-a-live-source-for-refusals-and-the-indexes-the-new-predicates-need) | Low | Attention: multi-reviewer row model; no live source for refusals; rerun and schedule_runs indexes | resolved (R020, rev 2) |
| [WF-P08-F079](agent-review-findings.md#wf-p08-f079--one-inbound-message-can-admit-several-runs-but-d46-gives-a-message-one-run) | Low | One inbound message admits several runs; single `run_id` and simulation 'its run' ambiguous | resolved (R020, rev 2) |
| [WF-P08-F080](agent-review-findings.md#wf-p08-f080--onboardings-create-its-channel-is-refused-for-the-agents-onboarding-creates) | Medium | Onboarding channel step refused: onboarding agents have no explicit model; no-connection path; key | resolved (R020, rev 2) |
| [WF-P08-F081](agent-review-findings.md#wf-p08-f081--b4as-plain-channels-conflict-with-the-active-agent-constraint-triggers-that-stay-until-b4c) | Low | B4a plain enabled channels vs active-agent constraint triggers kept until B4c | resolved (R020, rev 2) |
| [WF-P08-F082](agent-review-findings.md#wf-p08-f082--the-verification-plan-does-not-prove-the-new-live-protocol-for-every-consumer-or-the-cases-above) | Low | Verification misses per-consumer live tests and the F074–F081 cases | resolved (R020, rev 2) |
| [WF-P08-F083](agent-review-findings.md#wf-p08-f083--d4-notes-optional) | Info | Trigger WHEN/OLD and notify_thread_activity mechanics; accepted items; 7-day copy | adopted (R020) |
| [WF-P08-F084](agent-review-findings.md#wf-p08-f084--publish-resync-after-the-outer-listener-reconnect-too-carried-to-b4a) | Low | `Resync` also after the outer listener reconnect (backoff path), with a failed-then-recovered test | fixed (R021) |
| [WF-P08-F085](agent-review-findings.md#wf-p08-f085--rev-2-implementation-notes-optional) | Info | Catalog-check precision; preference row lock; onboarding replay test; keep channel join | adopted (R021; item 1 in B4c) |
| [WF-P08-F086](agent-review-findings.md#wf-p08-f086--the-mailboxs-open-in-simulator-button-now-leads-non-owners-to-company-not-found) | Low | Mailbox "Open in Simulator" shown to all readers; the page is now owner-only ("Company not found.") | fixed (R022) |
| [WF-P08-F087](agent-review-findings.md#wf-p08-f087--b4a-notes-optional) | Info | Index deviations accepted; lookup wording; demoted-actor link; check:css and run-counts at the end; legacy approval link | resolved (R022) |
| [WF-P08-F088](agent-review-findings.md#wf-p08-f088--b4b-silently-dropped-four-workflow-checkpoint-store-tests-two-of-them-competing-claimant-tests) | Medium | `mod store_tests` declaration removed with the legacy test; four live workflow checkpoint-store tests (two competing-claimant) no longer compile | fixed (R024) |
| [WF-P08-F089](agent-review-findings.md#wf-p08-f089--a-legacy-test-helper-is-left-behind-uncompiled-since-its-includer-was-deleted) | Low | Orphaned legacy `thread/model_usage_tests.rs` (includer deleted in B4a) | fixed (R024) |
| [WF-P08-F090](agent-review-findings.md#wf-p08-f090--dead-code-the-deletions-exposed-is-still-in-place) | Low | Exposed dead code kept: `Suspended` disposition, `stamp_duration`, `Driven::Suspended(None)`, three ThreadUseCases methods, `addressed_in`, `with_structured`, test helpers | fixed (R024) |
| [WF-P08-F091](agent-review-findings.md#wf-p08-f091--comments-and-intra-doc-links-still-name-items-this-batch-deleted) | Low | Stale comments and unresolved doc links to deleted items (`InboundTaskPayloadV1`, `AgentHarness`, `TaskPersistence`, `super::task`, task worker, registry) | fixed (R024) |
| [WF-P08-F092](agent-review-findings.md#wf-p08-f092--the-relocated-stuck-delivery-census-has-no-database-backed-test) | Low | New `census_stuck_deliveries` SQL tested only through a mock; index comment wrong (Seq Scan) | fixed (R024) |
| [WF-P08-F093](agent-review-findings.md#wf-p08-f093--b4b-notes-optional) | Info | `.sqlx` verified; removed reviewer leaves decisions unsettleable; memory jobs undrained until B4c; `AGENT_RUN_TIMEOUT_SECS` docs; guides for B4d; idle NOTIFY triggers | resolved (R024) |
| [WF-P08-F094](agent-review-findings.md#wf-p08-f094--an-intermittent-lease-resolution-timing-test-from-before-b4b-and-outside-it) | Low | `workflow_lease_resolution_stall_beyond_deadline_unknown_then_one_expiry` exact `elapsed >= T` bound fails intermittently (likely ms-floored server lock_timeout → early unknown); not B4b | fixed (R031) |
| [WF-P08-F095](agent-review-findings.md#wf-p08-f095--two-sqlx-files-are-stale-no-macro-produces-them-and-would-fail-cargo-sqlx-prepare---check) | Low | `.sqlx` `e83a43e1…`/`fe922e72…` have no macro since B4b (text survives in runtime calls); `prepare --check` would fail; B4c-3 must check macro-bound | fixed (R026) |
| [WF-P08-F096](agent-review-findings.md#wf-p08-f096--b4c-1-notes-optional) | Info | `AGENT_RUN_TIMEOUT_SECS` documented but unread (B4c-2); replacement page assert; schema test scope; doc deferral accepted | item 1 fixed (R026); rest notes |
| [WF-P08-F097](agent-review-findings.md#wf-p08-f097--the-rewritten-channel-gate-claims-an-eligibility-guarantee-the-inbound-commit-does-not-implement-and-its-new-competing-test-cannot-see-the-difference) | Medium | Gate doc says the inbound commit re-reads enabled/access under the shared gate; `commit_on` does not (eligibility decided before it); new competing test uses a bare lock | fixed (R027) |
| [WF-P08-F098](agent-review-findings.md#wf-p08-f098--b4c-2-notes-optional) | Info | Mailbox avatar always None; "used by" for p09; address preview copy; pre-existing uncalled `get_by_company_slug_and_agent_slug`; fixture `LIMIT 1`; slug conflict now 409 | resolved (R027) |
| [WF-P08-F099](agent-review-findings.md#wf-p08-f099--the-commit-first-gate-test-gives-the-queued-edit-only-the-default-2-s-lock-budget) | Low | `a_settings_edit_queued_behind_a_commit_applies_after_it` spawns the edit with `EditBudget::DEFAULT` (2 s lock timeout) across sleep + `commit_on`; flake risk under load | fixed (R028) |
| [WF-P08-F100](agent-review-findings.md#wf-p08-f100--f097-correction-notes-optional) | Info | Out-of-scope races (binding status, membership); check position OK; `Superseded` justified; error-class DB round-trip test suggested | resolved (R028) |
| [WF-P08-F101](agent-review-findings.md#wf-p08-f101--b4c-3a-notes-optional) | Info | Thin apply/catalog logs; catalog test could use `pg_get_functiondef` and scan retired values; unused `Icon::People`/`Goal`; DV-B4c3-5 to B4d; failed-job dashboard meaning OK | items 1–2 done (R030), 3–4 done in B4d; resolved (R031) |
| [WF-P08-F102](agent-review-findings.md#wf-p08-f102--b4c-3b-notes-optional) | Info | Gauge scope sound (≤ slots, clamp); recommendation is an observed safe level; README bullet accurate, §3.4 rest to B4d; F101.2 lists sufficient | resolved (R031) |
| [WF-P08-F103](agent-review-findings.md#wf-p08-f103--two-rewritten-doc-claims-name-a-removed-setting-and-a-removed-tool) | Low | `rig.md:137` names a `max_turns` 1–16 setting that no longer exists (fixed 8/model step); `deploy.md:177` skill pilot requires the removed `read_resource` | fixed (R032) |
| [WF-P08-F104](agent-review-findings.md#wf-p08-f104--the-06c-obligation-to-remove-mcp_connections_by_ids-is-neither-done-nor-dispositioned) | Low | TASK criterion 13 / 06C: `McpPersistence::mcp_connections_by_ids` (test-only callers) neither removed nor dispositioned | fixed (R032) |
| [WF-P08-F105](agent-review-findings.md#wf-p08-f105--phase-acceptance-evidence-and-gate-notes-optional) | Info | Acceptance matrix; criterion 2 decisions/artifacts structural only; turn guard untested (pre-p08); DV-B4d-1 revisit to plan; recreate stale `workflow08` before gate | adopted (R032) |

## Gate status

- **P08-D1 rev 1:** not passed (WF-P08-R001).
- **P08-D1 rev 2** (`5271630f…0c63a`): not passed (WF-P08-R002). F001–F011 are fixed. The new
  F013 and F014 (Medium) and F015–F017 (Low) block. A rev 3 recheck can be limited to those deltas
  and any section they touch.
- **P08-D1 rev 3** (`7c4b9457e146890eaaaa2b8936324726dcdd2da45f86333e7125eb8e2121aaf7`): **PASS**
  (WF-P08-R003). Carried into the B1 batch review: WF-P08-F019 (lock order and race test).
  Optional: WF-P08-F020.

- **P08-D2 rev 1** (`dfa1bf4d2b2455d7eef714441707173a3103c2809d673481a3dc52af77840c07`): **not
  passed** (WF-P08-R009).
  - Medium WF-P08-F036, F037 and F038, and Low WF-P08-F039 to F044, block.
  - WF-P08-F045 is optional.
  - A rev 2 recheck can cover the deltas.

- **P08-D2 rev 2** (`b88fe8add6ae5738708b4e07d1ed93154dc030f452291ee983946cd6a1b08afd`):
  **PASS** (WF-P08-R010).
  - Carried into the B3a batch review: WF-P08-F046, F047 and F048 (Low).
  - WF-P08-F049 is optional.

- **P08-D3 rev 1** (`121779fc3c94826a41c2b1a2d2413fa4b70d15643b468ee513cc7e60508625d4`): **not
  passed** (WF-P08-R016).
  - Medium WF-P08-F065 and F066, and Low F067 to F069, block.
  - WF-P08-F070 is optional.
  - A rev 2 recheck can cover the deltas.

- **P08-D3 rev 2** (`0cacf996a4a2d6a995938a8c9c7cc6deab6e1459ccd9d6f9716f26a7748f558c`):
  **PASS** (WF-P08-R017).
  - Carried into the B3b-2 batch review: WF-P08-F071 (Low).
  - WF-P08-F072 is optional.

- **P08-D4 rev 1** (`2f46c343954fd586bd83bc6e3298069690feef51d93086d5cfa33ece6387647d`): **not
  passed** (WF-P08-R019).
  - Medium WF-P08-F074, F075 and F080, and Low F076–F079, F081 and F082, block.
  - WF-P08-F083 is optional.
  - A rev 2 recheck can cover the deltas.

- **P08-D4 rev 2** (`d88aeb869d546d6308e106dddcb66e7274838351c4b2b001629da706027d3bce`):
  **PASS** (WF-P08-R020).
  - Carried into the B4a batch review: WF-P08-F084 (Low).
  - WF-P08-F085 is optional.

## Batch status

- **B1** (digest `90d4e0abbb34a6426258697b3459e8ba3dbdc6889dfeb8fba163f764370e7390`): **changes
  required** (WF-P08-R004). WF-P08-F021 (Medium) and WF-P08-F022 to WF-P08-F026 (Low) are open.
  The recheck covers the correction deltas.
- **B1** (digest `500ae85b9eb6ecb3e11aa1739adf3c30fc56893db95fd232d3f001c6fc691b11`): **changes
  required** (WF-P08-R005). WF-P08-F021–F027 are closed. WF-P08-F028 (Medium) and WF-P08-F029
  (verification gap) are open.
- **B1** (digest `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468`): **PASS**
  (WF-P08-R006). WF-P08-F028 and WF-P08-F029 are fixed. No blocking finding is open for B1.
  WF-P08-F030 is optional.
- **B2** (digest `7b932911f8a76a9fb4100af447f33df9388e052f7601576c22f6aa90b8125c51`): **changes
  required** (WF-P08-R007).
  - Open: WF-P08-F031, WF-P08-F032 and WF-P08-F033 (Low), and WF-P08-F034 (optional).
  - WF-P08-F030 is fixed.
  - A recheck can cover the correction deltas.
- **B2** (digest `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f`): **PASS**
  (WF-P08-R008). WF-P08-F031 to WF-P08-F033 are fixed and WF-P08-F034 is adopted. No blocking
  finding is open for B2. WF-P08-F035 is optional.
- **B3a** (digest `9bdad3dae00d010689c5660c3c9a0c8895145079ea6c1989cc3dea27287d1f3c`): **changes
  required** (WF-P08-R011).
  - Open: WF-P08-F051 (Medium) and WF-P08-F050 (Low).
  - F035 and F046–F048 are fixed, F049 is adopted, and DV-1..DV-6 are accepted.
  - A recheck can cover the deltas.
- **B3a** (digest `22334ec4414b79d0a68f8d052a3c787fca241611b8afa44e5bcd24ca7a4e77e1`): **PASS**
  (WF-P08-R012). WF-P08-F050 and WF-P08-F051 are fixed. No blocking finding is open for B3a.
- **B3b-1** (digest `22923f7f5c2c0e626ae535f8452c946f1e1c1028a3d859fd41b7f11484792c20`):
  **changes required** (WF-P08-R013).
  - Open: WF-P08-F054 (Medium), and WF-P08-F052, F053 and F055 to F058 (Low).
  - WF-P08-F059 to F061 are optional.
  - DV-7..DV-10 are accepted.
  - A recheck can cover the correction deltas.
- **B3b-1** (digest `b075aa52c21ccd966c2372c3a6685b4ee7eeb91032942eda364683fb3ff711ef`):
  **changes required** (WF-P08-R014).
  - WF-P08-F052 to F059 and F061 are fixed; F060 is partly fixed.
  - Open: WF-P08-F062 (Low).
  - WF-P08-F063 and F064 are optional.
  - A recheck can cover the F062 delta.
- **B3b-1** (digest `57566f926c9396e4d9b3eae72e5d6b46d74e5c8b3455d6fca912aa50d18ddd33`):
  **PASS** (WF-P08-R015).
  - WF-P08-F052 to F062 are fixed; F063 is resolved by disposition, and F064 is accepted.
  - No blocking finding is open for B3b-1.
  - `npm run check:css` was not run here (no `node_modules`); run it at the final gate.
- **B3b-2** (digest `4156165b358161367b1822179edbde0df23aa64d1ac6148e04797dbed68e20b3`):
  **PASS** (WF-P08-R018).
  - WF-P08-F071 is fixed, F072 is adopted, and DV-11..DV-13 are accepted.
  - WF-P08-F073 is optional.
  - No blocking finding is open for B3b-2.
- **B4a** (digest `5b27364aa4651526e4fec1b3b158e078f80fdfc171cdf0d687bc6d48573c2c16`): **changes
  required** (WF-P08-R021).
  - Open: WF-P08-F086 (Low).
  - WF-P08-F084 is fixed and F085 is adopted.
  - WF-P08-F087 is optional.
  - A recheck can cover the F086 delta.
- **B4a** (digest `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8`): **PASS**
  (WF-P08-R022).
  - WF-P08-F086 is fixed and F087 is resolved.
  - No blocking finding is open for B4a.
  - At the phase end: `npm run check:css` and `npm run test:run-counts`.
- **B4b** (digest `ac0a895808f9c091f86ca10cb74ebb7b9aba5524f3200d1d2aab0632eead63c7`): **changes
  required** (WF-P08-R023).
  - Open: WF-P08-F088 (Medium), and WF-P08-F089 to F092 (Low).
  - WF-P08-F093 is optional.
  - DV-B4b-1..6 and DV-B4b-8 are accepted; DV-B4b-7 is accepted except for F088.
  - A recheck can cover the correction deltas.
- **B4b** (digest `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896`): **PASS**
  (WF-P08-R024).
  - WF-P08-F088 to F092 are fixed and F093 is resolved.
  - No blocking finding is open for B4b.
  - Carried to the phase gate: WF-P08-F094 (Low; a pre-existing intermittent timing test outside
    B4b), `cargo sqlx prepare --check`, `npm run check:css`, `npm run test:run-counts`,
    `scripts/stack-budget.sh` and the full suite.
- **B4c-1** (digest `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11`): **PASS**
  (WF-P08-R025).
  - The memory deletion is complete. Positional SQL, the migration and the catalog are verified,
    and no non-memory coverage was lost.
  - Open: WF-P08-F095 (Low; B4b's two stale `.sqlx` files), fixed in this tree or by B4c-3 at the
    latest. WF-P08-F096 is optional.
- **B4c-2** (digest `97531db8994a61e52cb4fe227c5fc2961b7873690e87ce6304a6ae047340802d`): **changes
  required** (WF-P08-R026).
  - Open: WF-P08-F097 (Medium). WF-P08-F098 is optional.
  - WF-P08-F095 and F096.1 are fixed.
  - DV-B4c2-1, 3 and 4 are accepted. DV-B4c2-2 is accepted for the edit, not for the protocol
    claim.
  - A recheck can cover the correction delta.
- **B4c-2** (digest `2d4ea381960e8134768631a06eacc425442310dd0ed06c7a4723ba880e8a7098`): **changes
  required** (WF-P08-R027).
  - WF-P08-F097 is fixed: revision plus gate, with competing tests on the real commit. F098 is
    resolved.
  - Open: WF-P08-F099 (Low). WF-P08-F100 is optional.
  - A recheck can cover the F099 delta.
- **B4c-2** (digest `95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`): **PASS**
  (WF-P08-R028).
  - WF-P08-F097 and F099 are fixed, and F098 and F100 are resolved.
  - No blocking finding is open for B4c-2.
- **B4c-3a** (digest `fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde`): **PASS**
  (WF-P08-R029).
  - The migration is verified statement by statement against the B4c-2-accepted file. The catalog
    is clean, and the narrowed checks match their writers.
  - WF-P08-F101 is optional. DV-B4c3-1..7 are accepted.
  - No blocking finding is open for B4c-3a.
- **B4c-3b** (digest `845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6`): **PASS**
  (WF-P08-R030).
  - The runtime metrics now sample active workflow jobs and slots (D4.8), and
    `TASK_WORKER_CONCURRENCY` is removed.
  - WF-P08-F101.2 is fixed. WF-P08-F102 is optional.
  - No blocking finding is open for B4c-3b.
- **B4d** (digest `4ec81c1a9bf670f8efb5ef46b9ebac7f8c44261a70fa538b19cecfa598140fc1`): **changes
  required** (WF-P08-R031), with the phase-level acceptance review.
  - Open: WF-P08-F103 (Low; two doc lines) and WF-P08-F104 (Low; `mcp_connections_by_ids`).
    WF-P08-F105 is optional.
  - WF-P08-F094 is fixed. WF-P08-F101 and F102 are resolved. DV-B4d-1..3 are accepted.
  - Acceptance criteria 1–6 are supported by the evidence in F105.1. Criterion 6 is subject to
    F104, and criterion 2's decisions and artifacts rest on structure.
  - A recheck can cover the F103/F104 delta.
  - **Phase gate still owed:**
    - recreate `workflow08`;
    - then `cargo sqlx prepare --check`, `npm run check:css`, the full suite,
      `scripts/stack-budget.sh` and `npm run test:run-counts`.
- **B4d** (digest `00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829`): **PASS**
  (WF-P08-R032).
  - WF-P08-F103 and F104 are fixed. WF-P08-F105 is adopted, and F105.1 is the gate's evidence
    map.
  - No blocking finding is open for B4d or for the phase acceptance review.
  - **Phase gate still owed:**
    - recreate `workflow08` (and drop `workflow08_scratch`);
    - then `cargo sqlx prepare --check`, `npm run check:css`, the full suite,
      `scripts/stack-budget.sh` and `npm run test:run-counts`.
