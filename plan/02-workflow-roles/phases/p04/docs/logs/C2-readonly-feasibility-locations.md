# C2 read-only feasibility locations during source freeze

No source or test changes; these are primary source locations, not execution evidence.

- application/workflow/message_runtime.rs:115–151: MessageActions::publish calls real observation, message_routes, actual planner, then MessageDispatch::publish. The new MessageActions constructor at17–30 accepts generic persistence and dispatch. A scheduling-only dispatch adapter can gate just before real publish after the actual prepared plan, forwarding behavior rather than fabricating results.
- application/workflow/message.rs:427–440: MessageDispatch port has observe and publish; PlannedDestination/RouteCheck at302–340 retain exact member/user/principal/email/source identity. MessagePlan count bound is20 from registry/mod.rs:44 (distinct from16reviewer limit); do not silently impose a reviewer bound on established message fanout.
- persistence/workflow/message_test_support.rs:25–58: actual Setup and real composer/runtime factory; delivery helpers at384–420/529–604. Fixture claims need own_database for whole-queue assertions.
- persistence/workflow/message_delivery_tests.rs:16–66: real claim→begin_part and decisive refusal facts: dead_letter/superseded,0attempts, request_started_atNULL, one terminal event, actual receipt consumption. These establish no provider entry at the admission boundary; they are not a provider worker invocation counter.
- persistence/workflow/message_receipts.rs:425–516: actual ordinary delivery admission, after final-response admission dispatch; user destination checks exact current member principal/email against stored target.
- persistence/workflow/message_publish.rs:282–360: authority/company/lease first, current route recheck, actual knowledge_release on Frozen(plan recipients), then atomic write. writer recheck at76–125 holds member/principal rows; member_on itself does not lock account rows. Need full outgoing/SQL inventory before claiming no late account edge.
- persistence/workflow/knowledge_message_tests.rs:167–269: current proof fixture, member-route gate and inherited failing race; all unchanged at freeze.
