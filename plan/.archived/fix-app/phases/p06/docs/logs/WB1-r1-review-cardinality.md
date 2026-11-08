# WB1 r1 admitted-run cardinality review

Frozen source diff: a23e85b0fb6675374168f0e0f0feeb7902550c7f43409c38bba477a9713f2502.

The retention query in `src/adapters/persistence/workflow/storage_retention.rs:31-39` fetches at most Activation.maximum()+1 and rejects a tree with more than Activation.maximum()=100000 runs. This is a run-count restriction, not merely a query work bound.

Source trace:

- `children.rs:33-115` activates the PARENT before admitting its child. Repetition is charged only when control.repeat is present. Ordinary workflow.call does not charge repetition.
- `children.rs:219-315` inserts the child run, its first job, admission and wait; it does not reserve or consume a child activation. `admission_write::insert_run` and `insert_workflow_budget_link` link the new run to the existing root budget, with no admitted-run cardinality enforcement.
- `activation.rs:74-88` resolves inputs before `budget::activate`. `pending_recovery.rs:39-66` settles invalid input before an activation is consumed.
- `pending_io_tests.rs:132-176` supplies a normally admitted invalid RFC3339 wait deadline and proves the run becomes terminal with NULL frozen inputs/activated_at. It is not a corrupted fixture for that path. Independently rerun successfully in WB1-r1-review-run-budget-premises.log.
- A child failure can resume its already activated parent's final-error route: `child_runtime_tests.rs:449-478` and the child completion/wait writer. No additional activation is charged merely to resume the same activated call.
- `src/domain/workflow/graph.rs:7` allows 1024 nodes; its DAG restriction prevents a same-run graph loop, but does not make the total tree size equal to activated steps. Distinct call steps and reusable compiled child workflows suffice. `src/domain/workflow/budget.rs:5-7` bounds activations/model calls/repetitions separately.

Consequently there is no invariant that every admitted run consumes an activation. A construction using only ordinary calls is a root with a sequence of call steps, each invoking a workflow with many sequential ordinary call steps. Each leaf is admitted with valid run input but has the invalid wait-deadline first activation above, then fails without a charge; its parent error edge continues to the next call. Every charged call creates a child. With no extra pure steps and enough call steps across the DAG hierarchy, 100000 charged call activations can have created 100000 children, in addition to the root. Root-budget exhaustion subsequently terminates the remaining work. This illustrates the off-by-one failure even before considering workflow-tool children or repeat rounds, which have different admission/charge relationships. It is a source-derived construction, not a claimed execution of a 100001-run fixture.

The cardinality premise is false. A correction must establish a bound over every supported admission path, or remove this unsupported retention exclusion while preserving bounded atomic work. Merely substituting another convenient number without a proof and boundary tests would repeat the defect. Author owns that design. The original G1 r2 PASS remains historical; its D4 cardinality assumption is reopened by FIXAPP-P06-WB1-F1-MAC02.
