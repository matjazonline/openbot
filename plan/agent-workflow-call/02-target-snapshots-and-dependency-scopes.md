# 02 — Target snapshots and dependency scopes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: 01. Outcome: one durable, authorized target and immutable dependency scope per saved
channel call, with database constraints preventing forged relationships.

## Batch A — Capture the destination binding

1. Add a narrow application port for channel-call target preparation/dispatch. Implement target
   reads beside channel reply selection and binding persistence. Reuse selection, resource
   readiness, current authority, response-contract validation, and bounded bundle restoration.
2. Extend the init schema with the minimum invocation provenance required by the call: company,
   parent run/execution/saved call, destination channel, selected binding/configuration revision,
   response-settings revision/contract/review policy, child reference, and its dependency scope.
   Reuse existing call/wait/result rows wherever they already own a fact. Avoid a parallel mutable
   status column that can disagree with child, wait, review, or tool-result state.
3. Key invocation identity by company + parent execution + saved tool-call ID. Bind it to the
   canonical saved arguments/fingerprint so reuse with changed channel/content is rejected.
   Tenant and exact execution identifiers belong in composite foreign keys, not just app checks.
4. Capture the chosen binding version, params, resources, response configuration, and target
   dependency closure under coherent locks. Record the global lock order alongside existing action
   and binding owners. Do not hold database locks across a model/provider call.
5. Reconfiguration races must select either the complete old configuration or the complete new
   configuration, never a mixture. A committed invocation is immutable. Retrying after a lost
   response returns that same invocation even if the channel now points elsewhere.
6. Refusals before launch create no child, thread, source message, job, or response obligation.
   Retention/deletion cannot erase invocation evidence or dependencies while a caller still waits;
   teardown follows existing company/run cleanup ownership.

Focused checks: `channel_call_snapshot_` on a fresh isolated schema. Include SQL-only cross-company
and wrong-parent inserts, changed-arguments replay, concurrent dispatch versus responder replacement,
concurrent params/resource edits, and revoked target. Count exactly one coherent captured revision.
Regenerate SQLx metadata, run offline compilation, and check actual schema constraints.

## Batch B — Permit an explicit nested dependency scope

1. Refactor manifest capture to accept the configured target and actual invoking actor without
   fabricating a root admission. Share the existing bounded closure walk, agent revision selection,
   canonical bundle verification, grant checks, and seal protocol.
2. Introduce an explicit dependency-scope owner separate from workflow tree lineage. A root run
   owns its initial scope; a channel-call child may own a new scope only through a valid invocation
   record; an ordinary static/tool child inherits its parent's scope. Keep existing manifests
   immutable. Do not preload every company channel into the caller's root manifest.
3. Update run-manifest attachment and SQL guards to enforce those cases. Prove that a random child
   cannot claim a fresh scope, inherit a sibling's scope, select an unrecorded agent revision, or
   splice a foreign workflow into its dependency graph.
4. Walk downstream consumers: agent preparation/origin checks, history selection, dependency/run
   views, samples, revocation, retention, and teardown. Resolve agent/workflow pins through the
   run's recorded scope, not an assumption that every child uses the causal root's manifest.
5. Capture the target's transitive static dependencies at first dispatch; ordinary descendants
   use that capture. A later nested `channel.call` captures its own scope while retaining causal
   tree limits. Preserve bounded closure size and capture timeouts; avoid increasing limits.

Focused checks: `channel_call_manifest_`, existing `autonomous_agent_storage_manifest_`, and
affected `autonomous_agent_children_` cases. Prove publication after capture cannot change child
agent revisions; unrelated sibling scopes cannot leak tools/history; static calls preserve their
old scope behavior. Because this changes shared infrastructure, run the affected manifest, agent
preparation, and child-schema groups now rather than postponing them all to phase 06.

## Acceptance and record

- New scopes are explicit authorized invocation boundaries, never a bypass of a sealed manifest.
- Concurrent first dispatches converge on one invocation and one captured target.
- Runtime SQL is exercised against the fresh schema; an empty SQLx cache diff is not SQL proof.
- Record focused commands, counts, database identity, results, and unavailable checks here.

Implementation evidence: pending.
