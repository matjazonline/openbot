# 01.7 causal links and phase01 gate

Original01 item7 and acceptance govern. Astra `/root/astra_01_7`,session `01a0de39-ecf0-7c22-9c15-9bab659da4b8`. Preserve01.1–01.6. STOP after01; no02. No blocker.

## Model and invariants

Add domain/workflow/causality.rs, needed UUID IDs TriggerId/ActionInvocationId/ScheduleId; reuse CanonicalMessageId,RunId,ExecutionId,StepId,CorrelationId. Move existing workflow CompanyId to domain ids, reexport application for internal imports (ownership correction). Execution reference company/run/execution/step; action ref embeds execution+actionID. Stable TriggerId and source Message(canonical),Schedule(schedule+occurrence identity),Manual,Child(parent execution optionally specific action). Run causality company/run/trigger/correlation; step causality execution/step/run; message effect link canonical message+causing action. No recursive full parent history. Immediate refs only.

Checked constructors enforce parent company matching, consistent run/execution/action relationships, no self-parent run; avoid disagreeing optional parent fields. Containing company scopes message/schedule IDs. Correlation observability ONLY, caller-supplied allowed, never auth/causal proof/dedup/fence. Reuse existing CorrelationId.

## Existing integration

AdmitWorkflowRequest typed trigger+correlation; PreparedAdmission freezes and exposes first-execution causality derived from its generated identities. RunHead/ClaimedExecution preserve authoritative causality; no duplicate writable ID tuples. Admission equivalence includes stable trigger/source, excludes correlation/random proposed IDs. Replay preserves original stored causality despite changed trace; changed source/parent under same key conflicts. Actor/visibility unchanged: provenance sources not alternate authority.

Trusted admission port MUST validate authoritative company-scoped messages/schedule occurrences/parent executions/actions before commit, including parent action->execution->run and applicable message/channel/thread associations. Claimed links not proof; missing/foreign/inconsistent fail atomically, errors propagate. Implement fixture with seeded records, no production adapter/allowall. Future action/message persistence atomically keeps causal link with receipt/message; types/docs only now, no action runtime/schema.

## Files and edit scope

contracts.rs65–150 request/prepared,169–177 head,245–254 claim;service.rs51–81 prep;ports.rs22–34 admission,49–68 scheduling/inspection;tests.rs191–241 admit/equivalence,246–262 inspection,300–345 claims;tests/cases237–331 head fixtures;domain ids/mod/newcausality/tests. Graft callers missing indexed edges; exhaustive grep confirms service+workflow fixtures only,no production migrations. Public docs required. Root/src/application guides; exact source discovery/callers. Parent owns records.

## Acceptance/checks

Four sources same admission/run/claim model; trigger->run->step->action->child/message exact IDs. Repeated same node distinguished by execution ID. Foreign/inconsistent parent/action no writes/errors propagate. Changed correlation replay original; same correlation distinct keys independent; changed trigger/source/parent same key conflicts. Correlation cannot bypass denied auth or substitute fence. Existing races/control orders intact.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

Refresh graft. No DB/SQL/cache/services/dependency/temp/commit/deploy/reset. Phase01 combined review: definitions/context/graph/outcomes/states, application scoped auth/ports, replacement map and common4source run model satisfy acceptance; handlers cannot own arbitrary routing/retries/mutate earlier output. Persistence/full runtime/actions/compiler/trigger authorities/UI intentionally later, never claim operational engine. Save exact resume02 Definition and context contract then stop as user requested.

## Final review corrections

Behavior and aggregate01acceptance pass inspection; gate pending cleanup:
1. Parentclippy exit101: causality.rs124/159,tests/source_validation.rs32 collapse nestedif;authorization_cases/association.rs33/52 needlessborrows;state_cases.rs208/269 lexical lock-guard blocks before awaits.
2. causality_cases.rs26–125,128–210,213–316,319–407 long tests split into meaningful scenarios/helpers preserving all assertions,~80line guideline.
3. contracts.rs179–181 Conflict docs include stabletrigger/source differences.
Run fmt/diff/workflow2MiB/offlinealltargets AND clippy --locked --offline --all-targets -- -D warnings after all edits. Same Astra finalreview; no new behavior scope. Next recreatedworker Sol/high peruser.
