# P04 affected checks (pre-implementation inventory)

Architecture pending; no implementation/test claims. Actual source tracing:
- PreparedAgent::spec -> WorkflowCheckpointSpec -> native checkpoints::matching/open -> RunCheckpoint new/validation.
- protocol::history -> Execution::model_turn; native WorkflowBinding definitions/reservation/admission. Seed must not consume turns.next() used only for current Assistant messages.
- RunCheckpoint::validate_conversation is ledger validation; separate seed leaves current messages/turns invariant intact. Shared constructor/decode/state tests affected.
- AgentPreparation native current_on -> history::check_prepared_on; checkpoints::authorize_on invokes current_on before own source locks/model/execution/tool writes. New audience gate must map existing KnowledgeRelease to knowledge.audience_below_content in Rig failure().
- Existing HistoryThread is genuine message/admission/current manifest fixture with scripted model, exact real thread identity, and level-change fixtures. ordering::runtime supplies tool-free actual provider runtime; child/tool history fixtures cover richer continuations.

Required selected families: agent_history_selection_, agent_history_level_, agent_history_preparation_, agent_history_protocol_. Require nonzero list and executed sets, counts/overlap reconciled.
Focused regressions: history all; workflow_agent_preparation_; workflow_agent_run_; workflow_harness_; actual services::harness::runs state/byte-budget tests; Rig supported-provider protocol/failure/diagnostics tests; knowledge run-level/current audience/release and native checkpoint/authority subsets; affected human/tool/reply cases. Select actual names from cargo --list after implementation; nonexistent prefixes add no coverage. No full/stack/lease suite.

Cases: two actual separate calls include exact historical JSON pairs current last; source input nonrecursive; selection order17metadata/16pairs/64KiB and controlled current-budget boundary; oversized newest stops older; hidden L5 does not stop older L0 or change omission; source current exclusion/failed/cancelled/private/model-only; snapshots survive old source payload prune and fresh claim/checkpoint reopen; atomic freeze+taint rollback, real applied/aborted COMMIT snapshot proof; current audience drop after preparation/at actual checkpoint model resume leaves provider calls unchanged and returns knowledge.audience_below_content. Provider normalized seed + real tool/repair ledgers and all supported providers; current revision/provider/schema selection invariant, counted messages/fingerprint/diagnostics, checkpoint strict size bound and stock2MiB focused run.

Fresh own openbot_ah_p04/openbot_ah_p04_test migrate reset init; actualSQLxprepare/check, offlinealltargets, Clippy-Dwarnings/fmt/diff/Graft. B runtime rerun only if missing evidence, serialized by root; baseline prior P03 460 accepted is prerequisite, not P04 evidence.
