# AH-P07-B1-R1 independent path coverage

All 16 paths were compared independently with the accepted P06 owned archive, falling back to the pinned HEAD for previously unchanged files. Exact cumulative source/archive and external inputs are checked by the entry/exit proof. Source discovery used Graft before exact-span reads. Earlier accepted P01–06 production coverage is reused only for unchanged source.

| Path | Independent coverage |
| --- | --- |
| .github/workflows/agent-history.yml | Fresh PostgreSQL18.6, locked dependency preparation, query-statistics configuration/probe, guarded native parents and real markup exporter, installed Chromium browser invocation; existing external CI unchanged. Hosted execution/actionlint unavailable, YAML/local commands inspected. |
| assets/app.css | Generated semantic diff adds only `.\!visible{visibility:visible!important}`; no removed rules. Current CSS check and actual browser evidence inspected. |
| docs/workflow-verification.md | Exact six process frontiers, uncommitted acquisition and unknown-provider limits, counted transport and hosted/local distinction. |
| plan/04-autonomous-agent-history/07-integration-and-verification.md | Original07A/B requirements retained; appended record accurately states targeted-only override and pending independent/integration acceptance. |
| agent_history_e2e_tests.rs | Parent-owned UUID database, trusted listener, native admission, actual competing WorkflowWorkers and bounded poll/lease recovery. |
| agent_history_process_driver.rs | cfgtest separate libtest process, actual native claim/prepare/runtime, explicit before-wake handoff, ignored driver actually invoked by parents, SIGKILL-only parked barrier. |
| agent_history_process_fixture.rs | Owned child process, bounded/joined stdout reader, kill/reap cleanup, real native trigger gates and company lock witnesses. |
| agent_history_process_tests.rs | Acquisition rollback, durable frozen ownership, committed model response, final checkpoint; exact seed/row/output/counts/stale fence and real request markers. |
| agent_history_queue_crash_tests.rs | Durable enqueue, cancelled middle, native polling wake recovery, old Completed owner replay cannot release successor, exact positions/seeds/provider3. |
| agent_history_knowledge_e2e_tests.rs | Actual uploads/search, real CC ingress, canonical participant removal, restored third-turn seed and level5, actual lower-authorized Inspect/HTML and counted deliveries. |
| agent_history_human_cc_e2e_tests.rs | Two real token decisions, real search raises content, CC before approved resume, same exchange/checkpoint/preparation, typed refusal and no further provider/answer/root. |
| agent_history_private_context_tests.rs | Additive e2e module wiring; inherited tests unchanged/discoverable. |
| knowledge_e2e_tests.rs | Additive knowledge/human journey module wiring; inherited cases unchanged/discoverable. |
| src/application/services/test_support.rs | Additive inheritance export behind existing cfg(test) module; normal guarded registration remains unchanged. |
| src/application/services/test_support/scenario.rs | One reserved proof request does not consume/capture model exchange; ordinary strict framing/assertions/request cap/deadlines/counts preserved; actual affected caller inventory and current regressions. |
| src/application/services/test_support/scenario/inheritance.rs | Opaque issued capability; loopback/credential guard before connect; bounded mutual HMAC challenge, separated request/response domains, no secret on wire, single use. Actual reflection/forgery/external/duplicate/model-count checks. |

Persistence filenames without a directory above are under `src/adapters/persistence/workflow/`. No production schema, authorization, turn, preparation, checkpoint, release or retention protocol changes in this phase. No consequential production architecture gate was missing. Test-only listener inheritance and real process seam were independently reviewed here; no inference of remote-provider exactly-once guarantees.
