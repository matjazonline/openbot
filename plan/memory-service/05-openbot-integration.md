# 05 OpenBot integration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-observation-generation.md). OpenBot uses the service through a narrow adapter.
All OpenBot-specific scope, workflow, capability and recipient behavior stays here; phases 01–04
must remain independently buildable and usable. Operations and final checks follow in
[06](06-operations-and-verification.md).

## Reuse existing audience and consumed levels

Reuse [AccessLevel and RunKnowledge](../../src/domain/entities/access_level.rs),
[audience and run admission](../../src/adapters/persistence/workflow/knowledge.rs), and the
existing consumption/release checks. Do not introduce a parallel OpenBot clearance system.
The service implements only phase 01's generic numeric-level contract and has no dependency on
these modules or OpenBot's participant directory.

For message-driven calls, use the existing lowest-human-reader rule for the actual authorized
conversation audience. Current admission resolves thread participants; do not assume checking
only the sender covers all channel readers or intended destinations. Manual/scheduled calls use
their existing actor/run-as authority and child calls inherit their parent's context restrictions.
Agent/system principals do not raise human clearance. Preserve existing public handling for
external readers and fail closed on malformed authority.

The effective memory read ceiling is the minimum of the admitted run ceiling, current authorized
audience/actor clearance and service credential/namespace read grants. Participants at 6, 8 and 9
therefore permit reads at 0–6 when the other bounds allow 6. A later audience increase cannot raise
the admitted ceiling. Recheck audience reductions before further model entry, context reuse and
disclosure; an already consumed level-6 context cannot continue for a level-3 audience.

For a memory produced by an agent, use the run's trusted monotone `content_level` as the consumed
watermark, not `knowledge_ceiling` or the highest level in only the latest search response. Audit
every model-visible input path: classified messages, instructions/context, frozen history,
knowledge/memory snippets and details, tool results, and inherited/approval continuations must be
accounted for before exposure. If precise consumption accounting is incomplete, use the trusted
authorized ceiling of the producing context as the fallback: ceiling 6 saves at 6. Preserve that
ceiling with the source context; a later audience reduction cannot replace it with a lower one.
Fix untracked input paths, and refuse if no trusted bound exists or known content exceeds it.
Never assume that missing evidence means level 0.

Save at that consumed level, bounded below by contributing source levels. A ceiling-6 run that
has consumed only level-4 information saves at 4. If its history or any other consumed input was
level 6, a new note saves at 6 even when the latest recalled notes were public. The model cannot
select a lower level or discard earlier exposure. Do not reset the watermark mid-run to obtain a
lower label. When the exact consumed level is unavailable, the ceiling fallback takes the same
role. Namespace grants, write authorization and existing egress checks still apply.

## Batch 05A Connect current OpenBot capabilities to the service

1. Trace current workflow preparation, action authorization, checkpoint consumption and company
   lifecycle owners before editing. Define the memory port in the consuming application layer;
   implement authenticated bounded HTTP in an adapter. Do not import the service package, share
   a SQL pool, restore legacy memory-provider registries, or implement memory storage in OpenBot.
2. Map a company to the same opaque tenant ID without making the service understand companies.
   Resolve explicitly granted company/agent/user scopes to service namespaces. Missing user or
   agent identity omits/refuses that scope and never widens it. Keep source/channel/run identifiers
   as provenance, not automatic access grants. Scope provisioning and credentials to this client.
3. Implement the existing `memory.load` and `memory.save` contracts coherently through the normal
   workflow runtime and authoring paths. Reconcile their schemas with bounded provenance-aware
   results. Remove their unavailable status only after authorization, runtime handling, samples and
   tests exist. Stable logical writes must retain the same service idempotency key across retries.
   Keep authority fields out of model-selectable schemas; derive the read ceiling and consumed
   watermark in the host. Add level-aware results and durable save evidence where those contracts
   need it rather than exposing a caller-controlled classification override.
4. Expose agent capabilities for search, timeline, selected details and intentional save using the
   established tool-publication/dispatch model. Select scopes and ceilings from trusted execution
   state; an agent cannot choose arbitrary tenant IDs, impersonate a user or lower classification.
   The standalone service MCP surface remains available for other clients; it is not a shortcut
   around OpenBot's native consumption and egress checks.
5. Apply existing live capability, membership, audience and ownership checks before requests and
   before consuming results. Atomically record the observation/checkpoint result, raise the run's
   content level, and record live consumption before any returned text reaches the model. Reuse
   the pattern in [knowledge consumption](../../src/adapters/persistence/workflow/knowledge_checkpoint.rs)
   without making memory depend on the knowledge-library storage or embedding model.
   Freeze a save's content, sources and consumed watermark under the same live ownership/run
   transaction as its delivery intent. Concurrent recall and save must serialize classification
   accounting so no result exposed before the save can escape its label. Keep the frozen label
   on retry; an earlier completed save does not authorize a new lower-labelled memory.
6. Extend samples with isolated fixtures/intercepted writes; samples cannot contact the service
   or a paid provider. Reconcile old tests asserting the legacy provider schema is absent without
   restoring that schema. Only service connection/configuration and delivery intents belong in
   OpenBot's database; the memory corpus stays in the service database.

Focused checks: `memory_integration_` and `memory_scope_` exercise missing identities, ungranted
tools, tampered arguments, cross-company IDs, revoked credentials, lost workflow ownership,
classification rise, recipient restrictions, sample isolation and duplicate saves. Add
`memory_access_level_` cases for participants 6/8/9, read levels 0–6, consumed-4 saves at 4,
consumed-6 saves at 6 despite public latest recall, missing precise evidence saving at 6, forged
labels, a missing trusted ceiling and an audience drop to 3. Run real competing recall/save
transactions and verify both ordering outcomes preserve the exact or fallback watermark. Run
affected workflow publication, tool dispatch, knowledge-level and sample-boundary regressions.

## Batch 05B Add automatic context and authorized capture

1. Add bounded automatic recall at logical agent preparation, before provider entry. Combine a
   small memory pack with existing history/current input under the total context budget; do not
   replace history or reload memory on every tool round. Preserve the caller's mandatory current
   input and reserved output capacity.
2. Put retrieved records in the existing delimited untrusted context with IDs, provenance,
   timestamps and classifications. Put only usage policy in system instructions. Keep pure prompt
   construction free of network calls. Treat memory as evidence that can be stale or corrected,
   not as instructions or an authorization policy.
3. Freeze the selected pack and its content integrity in the execution's durable preparation.
   Retries replay it rather than choosing a new pack under an existing checkpoint. Revalidate
   live audience, authority and source validity before resume; revocation/deletion produces the
   existing controlled refusal, not a different prompt or a lower content level.
   Replay the pack's recorded classification too; selecting only lower-level memories later
   cannot erase higher-level information already present in the producing context.
4. Define optional recall failure separately from no matches: an eligible new run may continue
   with a recorded unavailable state if policy permits. Authorization failures fail closed.
   Intentional saves and accepted generation requests must not be reported successful when lost.
5. Capture only authorized completed activity windows and relevant session outcomes, with
   privacy filtering and the source classification intact. Submitting capture data alone cannot
   enable inference; explicitly configured capture policy supplies the generation allowance and
   expiry. Failed/cancelled runs, samples, credentials and unrelated private activity are excluded.
6. Where a committed OpenBot outcome promises saved-memory delivery, commit a bounded delivery
   intent with that outcome and use supervised, fenced, idempotent delivery. Delivering a finished
   memory may retry safely; never turn its retry into a new generation request. Generation-intent
   delivery keeps its original logical identity/expiry and cannot replenish its spend allowance.
   Both forms retain the consumed watermark captured at their authorized source boundary. A
   changed audience or retry must not recompute the saved label from only the latest inputs.
7. Order company suspend/delete and connection revocation before further delivery or generation.
   Send reliable lifecycle requests to the service, retain deletion barriers, and prevent delayed
   provisioning/capture from recreating removed namespaces.

Focused checks: `memory_preparation_` and `memory_capture_` inspect actual model context and prove
bounded packing, no instruction promotion, exact retry snapshots, classification-aware resume,
optional unavailability, no duplicate capture, no secret leakage, and durable promised delivery.
Use concurrent delivery claimants and deletion/delivery races against real PostgreSQL. Run affected
agent preparation, history, checkpoint, budget, cancellation and lifecycle regressions.
