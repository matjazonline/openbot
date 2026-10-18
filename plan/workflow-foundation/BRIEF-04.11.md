# 04.11 — Provider-neutral messaging

No backward compatibility is required; reset the database when schema or payload
changes cannot apply cleanly. Do not add backfills or compatibility paths.

Status and verification live in [TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md).
The effective architecture is decision04.11-D2 amended by D3 and D4 in docs/logs/04.11.

`message.send` and `message.reply` are trusted built-in tools with fixed schemas. Each
send destination is a company user or a company channel; groups are rejected. A send
always opens a new thread: an in-app channel thread, or a user email thread in the run
channel. A reply uses the source message's own thread and headers: an external email
reply is a conversation entry with Received threading, an internal reply is a note.
There is one canonical message per destination, at most 20 destinations and at most 128
message markers per execution (a Rust probe under the attempt lock, and an SQL trigger
for SQL-only writers).

Publication is one transaction under current company authority: the `message` marker
is its first write, then the action, threads, messages, deliveries and destination
links. A routing or authority refusal while no marker exists (actor demoted, recipient
removed, source changed) rolls back with nothing written and fails the step Terminal
`message.rejected`. A user email starts its relay chain at hop 0 from the run channel; an
email reply continues the relay trace its source arrived with, as the source's inbound
task recorded it, and starts at hop 0 when none was recorded.
All-in-app publication commits its receipt atomically. A step with an email
destination parks in an effect wait in the same transaction (attempt `effect_parked`,
job completed, run `waiting/effect`, wait deadline = run deadline); the supervisor
confirms that exact close and never completes or releases it. No provider I/O happens
in the workflow; the delivery engine sends after commit and owns provider truth.

Delivery status changes of linked deliveries are recorded by the parent status trigger.
The receipt consumer (`message.consume`, one action per transaction, at most 8 per
visit) aggregates destination states into the action state and the receipt, which a
database guard recomputes. An accepted receipt completes the step; a failed one takes
the final-error writer with `message.delivery_failed` and never resends. An
`outcome_unknown` delivery leaves the action `uncertain` and is reconciled per delivery
through the existing 04.7 service: applied marks it delivered, final not-applied
reopens only that delivery on a live run (once per proof), and supersedes it on a
terminal run or when attempts are exhausted. Workflow-linked `begin_part` rechecks the
run, the actor, the recipient, execution conflicts and reopen-proof expiry before any
provider call. A finality breach vetoes effect-wait resume: the step parks for
reconciliation and unsent siblings are superseded. No production email evidence
verifier is installed; tests inject a test-host verifier through the real service.

Divergence from 04.7, recorded deliberately: the 04.7 `continue_on` reopens a
reconciliation-parked owner for remote invocations. Message settlement never does, for
every outcome (`DeliveryReopened` reopens only the delivery). Step-origin progression
already flows through the effect wait, and reopening a `model_tool` agent job before
phase 06 would resume an execution whose saved-call replay does not exist.

Phase 06 obligation: a `model_tool`-origin agent execution parked for reconciliation is
reopened by the phase-06 saved-call owner once every message marker of that execution
has a receipt and `workflow_action_reconciliation_reopen_safe` holds; until then it
stays parked. This is unreachable in phase 04 because publication refuses agent
snapshots. Trigger: the first phase-06 change that exposes direct message tools in an
agent host must implement that reopen and the saved-call resume before shipping.

Production worker startup and agent tool exposure remain phase 06/08 obligations, as
for HTTP and MCP; `MessageRuntime` is the composition seam.
