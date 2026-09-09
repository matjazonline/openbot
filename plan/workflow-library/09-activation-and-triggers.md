# Step 9 — Activation and Automatic Starts

## Outcome and dependencies

Activate a reviewed revision and start ordinary SOP runs from manual actions, inbound messages,
and existing schedules. Depends on [runtime/sample execution](08-runtime-and-sample-runs.md).
Deliver manual activation first, then enable the two automatic sources behind separate rollout gates.

## Activation

1. Add `activate_revision` with installation/revision edit versions, accepted candidate hash,
   successful sample identity, command ID, and authorized manager. Recheck all selected resources,
   role authority, current data/tool grants, model readiness, procedure validity, and sample hash.
   Provider readiness checks happen before the transaction; validate status generations again
   under locks and continue to enforce them on actual execution.

2. Publish the SOP version and installation revision, freeze execution specs, switch the
   installation's active revision/state, apply reviewed trigger/dispatch configuration, and append
   events/command receipts in one transaction. A missing prerequisite or stale sample leaves the
   previous active revision unchanged. Initial activation leaves the installation configured on
   failure. Do not enable personal agent channels that the workflow does not require.

3. Use one `start_installed_workflow` application command for every source. It loads the exact
   admitted revision, authorizes the actor/source, validates bounded start inputs, resolves role
   defaults, then creates the run, run binding, actual role bindings, initial occurrences/tasks,
   SOP events, and any notifications through the SOP start transaction.

   Mark installation-owned procedures explicitly. Ordinary SOP start/publish routes delegate to
   these commands and reject attempts to bypass the active revision or paused/configured state
   with a raw procedure-version ID. Add tenant-scoped `workflow_trigger_bindings` for operational
   source bindings, with stable trigger key, current revision/generation, typed source settings,
   channel/schedule references, and enabled state. Enforce one enabled inbound rule per channel
   in the database and create/update these bindings with activation.

## Durable admission

4. Add a typed `WorkflowStartSource` and stable logical start key. Manual starts use a user command
   UUID; inbound starts use installation + stable trigger key + canonical message ID; scheduled
   starts use installation + stable trigger key + durable schedule-run ID. Company scopes every
   key. Do not include worker attempt or current template revision in dedup identity, because an
   update must not turn a replay into a new case.

5. Add a small `workflow_start_intents` ledger for automatic source handoff, with company,
   installation/trigger/source identity, observed activation generation/revision, source IDs,
   bounded mapped inputs, status, resulting run ID, and typed terminal reason. In the source
   transaction, insert one intent and one `background_tasks` materialization task. Payloads hold
   IDs only. This is a logical admission record using the existing queue, not another task engine.
   The materializer atomically resolves the intent, creates/binds the SOP run and first tasks,
   and completes its own task under the existing lease fence.

6. Define the admission boundary as the transaction that creates the SOP run. A pending intent
   whose installation was paused, archived, or switched to another activation generation resolves
   as `suppressed` with a reason; it does not silently execute later with changed configuration.
   Already-created runs continue on their pins. A manager can explicitly replay a suppressed
   source with a new audited replay identity after reviewing current inputs. Retain the original
   source dedup record so duplicate provider events cannot bypass this decision.

## Inbound messages

7. Start with deterministic rules: an explicitly selected channel/binding, allowed authenticated
   source kinds, and a bounded exact/prefix subject filter if needed. No LLM classification,
   user expressions, unbounded regex, or automatic historical backfill. Exclude context-only
   messages, agent/system messages, delivery echoes, and correlated outreach replies; preserve
   existing ingress authentication, ACLs, anti-loop protections, and outreach handling.

8. Integrate routing into the canonical ingress commit: a matched source is stored once and
   creates the workflow start intent instead of a competing generic agent-dispatch task. Use a
   typed dispatch decision, not an after-the-fact message listener that sends both responses.
   In V1 allow one enabled workflow-start rule per destination channel and explicitly configure
   unmatched-message behavior. A dedicated workflow channel files unmatched/paused traffic for
   human attention. Reusing a general channel requires the manager to review this routing change.
   Source fields satisfy only declared input mappings; email text never authorizes CRM access.

## Schedules

9. Extend the existing schedule target with a tagged choice between the current agent-prompt
   target and an installed workflow plus typed start inputs. Reuse interval/one-off cadence,
   IANA timezone validation, `run_as_user_id` policy, durable run slots, materialization leases,
   and shutdown ownership. A scheduled workflow must have an eligible authorized run-as actor;
   absence must not silently grant system access. Do not build a second scheduler or fabricate
   an agent prompt as a substitute for structured SOP input.

10. Atomically materialize a due slot into its canonical source/thread if needed, start intent,
    and existing-queue task, preserving the slot identity. Define missed-slot/coalescing behavior
    explicitly using the current scheduler contract. Pausing disables future schedule admission;
    bounded recovery suppresses pending stale intents. Changing a schedule creates a new
    configuration generation without rewriting historical slots.

## Verification and acceptance

- Race activation with pause/update, and two starts for the same manual/inbound/scheduled source.
  Prove one admitted run and initial task set, or one durable suppressed outcome.
- Attempt direct SOP start/publication of an installation-owned procedure while configured,
  paused, or on a stale revision; ordinary routes cannot bypass installation admission.
- Race an intent materializer with lease loss and source redelivery; no duplicate run/task set.
- Test inbound handling with generic dispatch, context-only notes, outreach replies, self echoes,
  unauthorized senders, and an event arriving during activation. A source cannot create two
  independent responders unintentionally.
- Test scheduled attribution, deleted run-as members, DST/cadence compatibility, restart recovery,
  and pause/resume with pending slots. Do not introduce a new meaning for existing schedules.
- Run the poison-batch regression, bounded-shutdown checks, and stock-stack budget for the new
  materialization path. An admission failure must not hot-loop or starve other company tasks.
- Accept when all sources converge on the same authorized, idempotent SOP start command.
