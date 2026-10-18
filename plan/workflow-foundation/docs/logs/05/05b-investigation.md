# 05B read-only preparation during 05A acceptance gate

No backward compatibility. No 05B implementation before 05A acceptance.
Author root; accepted requirements D1 §9 and D2 §16 amended D3 §2 and D5/D6 identity lifetimes.

Graph traced authorize_prelocked_on transitively and enumerated ApprovalRequired consumers.
Shared seam: action_dispatch.rs authorize_prelocked_on 85–146, local 189–248,
reserve_remote 381–479, enter_remote 481–530; action_receipts return_authorized 313–359.
Both request digests must match a completed approved decision; current actor/resource/policy
checks still precede it. Persist approval_required=true and decision id on marker policy.

PostgresActionDispatch currently holds persistence, authority, local (58–70). Notice composer
must be explicitly supplied for approval support without phase08 production wiring. Reuse
DecisionNotices and business reviewer/notice writers; no new delivery queue or authority port.

message_publish publish_on 279–346 is a write-bound transaction; protected branch currently
rolls back ApprovalRequired. observe 404–443 is READ ONLY and returns None when no marker
exists. Initial protected messages therefore proceed to publish_on; do not turn a read-only
observer into a park writer. Any change to the literal D2 §16.3 observation inventory must be
stated in the implementation record/review; no existing-marker action may request approval anew.

Ambiguous commits: local resolves only receipts now; reserve_remote resolves exact entry;
message publish has its stored-action resolver. Each new park branch must resolve a committed
approval wait by the exact fence/attempt and subject, including if submit/cancel already won.
Supervisor confirmation reuses completed task_attempt history (approval_parked counterpart to
message_receipts::confirm_suspension 357–384), not a requirement that the run remain waiting.

MCP: dispatch_prepared 137–176 currently skips open based on frozen request.context.approval_required.
McpCall::open 245–258 already asks mcp_use and avoids network while usage.approval_required.
The latter must recognize the exact approved persisted intent; remove the earlier frozen-only
skip so approved calls can open, while unapproved calls still do not contact the server.
Authoritative dispatch check remains reservation. mcp.rs mcp_use 296–332 owns live resource check.

route_parked_failure recovery.rs 293–335 already routes/fails completed parked jobs and leaves
normal recovery ownership. Extend classification for approval reject/expiry. Business timeout
retains decision.timed_out and its current semantics.

Before first 05B source edits: rerun graph callers waits::row and exhaustive workflow_waits SQL
inventory per accepted D2 §16.4; inspect all enum match consumers above. Per-reason unique +
one waiting wait/execution must preserve the already-completed approval then effect-wait case.
