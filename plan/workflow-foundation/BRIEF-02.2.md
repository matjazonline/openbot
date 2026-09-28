# 02.2 — Registered step types

Original phase02 **Registered step types** and phase acceptance remain authoritative.
Expansion only, Astra `/root/astra_02_2`; verified `CODEX_THREAD_ID`
`01a0e3dd-c601-7021-a953-9bc355d256b6`. No blocking product decision.
Preserve accepted, uncommitted 02.1 and parent-owned PROGRESS/RESUME. Implementer is
Astra/low under the user's latest override; independent review remains Astra/medium.

## Scope and accepted foundation

Register all **18** original types with real compile/configuration contracts, schemas,
capability requirements, effect/recovery constraints and matching authoring help. This is a
pure compiler catalogue, not handler implementation. No execution callbacks, fabricated
success, SQL, publication/binding commands, providers, routes, runtime integration or legacy
replacement. Later points own publication and representative end-to-end fixtures; phases
3–7 own execution. Do not claim the complete phase02 acceptance gate is satisfied here.

Reuse 02.1 evidence: 82 workflow tests passed at 2 MiB; offline all-target check, Clippy,
format and whitespace gates passed. YAML byte/depth/node/scalar bounds, duplicate/alias/tag
rejection, source locations, typed bindings, missing versus null, dependency bounds,
availability analysis and native-resource JSON Schema validation are accepted. Do not repeat
their correction history or rebuild those mechanisms. Rerun them as regression coverage.

## Existing seams and implementation shape

- `compiler/compile.rs:56–98,279–358,440–520`: descriptors currently contain schemas,
  exact routes and control facts, keyed by **type**, with facts hashed by type. Real registrations
  need **per-step specialization**: two agents/maps/decisions of the same type can have different
  output schemas and choices. Extend the compiler input with a narrow registration provider or
  explicit catalogue abstraction; resolve a descriptor for each parsed step before checking
  inputs/routes. Keep an explicit-descriptor implementation for existing compiler tests.
  Store/hash specialized facts by **StepId**, with deterministic registry contract revision,
  capability/effect/retry facts and all supplied contracts affecting validation. Preserve the
  pre-serialization 1 MiB fact budget; do not hash unchecked/unbounded registries.
- `compiler/wire.rs:191–241` and `ParsedWorkflow`: retain one binding grammar. Static schema,
  choices, fixed tool names and other compile-time declarations live under `with` as
  `{literal: ...}` bindings. Require literal values where specialization needs them; reject a
  dynamic schema/tool declaration with a located actionable diagnostic. **Do not introduce a
  second `config` authority or move `with.capability_profile`/`with.output_schema`.** Existing
  top-level `rule`, child IDs and `max_iterations` retain their existing spelling.
  Future-plan shorthand YAML is illustrative: document executable v1 wrapper/route spellings.
- `compiler/schema.rs:22–81,122–209`: use the existing bounded Draft202012 validator for every
  declared schema, including nested output/data/payload contracts. Preserve local `$ref`,
  `$dynamicRef`, `$id` scopes, sibling constraints and no external retrieval. A schema-valued
  field is not an ordinary closed configuration object; permit valid JSON Schema vocabulary.
- `compiler/compile.rs:180–225`: resolved input/output validators remain mandatory. Carry any
  registration-specific semantic checks needed after binding resolution into these hooks;
  unresolved values must never receive silent success. Pure checks only, no policy/network I/O.
- `domain/workflow/outcome.rs:12–21,49–89,132–166,186–227` already owns completion choice,
  durable wait deadline, failure class and engine retry eligibility. Reuse these meanings; do
  not create a parallel outcome or retry engine. Indexed domain code has no existing generic
  `EffectClass`/replay-policy type. Add small explicit catalogue metadata enums in application
  workflow registry; future shared actions can consume/refine them. Do not import legacy harness
  effect state or treat `FailureClass::Retryable` as proof of safe effect replay.
- `domain/workflow/definition.rs:29–33` supplies checked resource slot/kind/contract. Validate
  literal slot references against these declarations, including kind compatibility. Dynamic
  resource selections require runtime binding authorization later; catalogue checks are never
  proof of company ownership. Keep domain independent of application/adapters.

Put catalogue code in `application/workflow/registry/`, split registration families and tests.
Use checked existing identifiers or narrow newtypes for semantic keys. Trace actual callers before
changing public constructors: graft callers has no indexed edges for StepDescriptor; graft grep
finds compile.rs, mod.rs, compile_test_support.rs, compile_tests.rs and
compile_validation_tests.rs. Keep functions around 80 lines and tests below ~500 per module.

## Concrete minimum catalogue contract

The following wire decisions complete unspecified details in the original table. All fields in
this table are `with` fields unless explicitly marked top-level. `?` means optional; other fields
are required. Inputs are bindings; declarations marked **static** must be literal. Schemas describe
resolved values, not wrapper syntax. Registration-owned objects are closed recursively. User data
(`context`, `arguments`, mapped data, payloads and schema-governed results) remains extensible only
according to its declared schema. Bound strings, collections, histories, timeouts and outputs with
named enforced ceilings; document exact bounds alongside the registration, not promises alone.

| Type | Required contract and output |
| --- | --- |
| `context.load` | `sources` bounded selection list, `max_tokens` positive; output `{items: array, token_count: integer}` with source/provenance identity per item. Admission cutoff comes from frozen run facts, never an author-supplied later timestamp. |
| `memory.load` | `scope` closed `{kind: company/agent/user, id?}` (ID required for agent/user), `query` string, `limit` positive; output `{items: array}` with provenance per memory. Capability scoped read. |
| `memory.save` | same `scope`, nonempty bounded `facts` array; output `{saved_ids: string[]}`. Capability scoped write; logical idempotent persistence required later. |
| `ai.classify` | `context`, **static** `output_schema`; output conforms directly to that schema. Require an explicit finite labels/profile enum in the declared result contract (a scalar enum or a named label/profile property is sufficient); no tools or capabilities selection by inference. Provider/model resource/config belongs frozen facts, no raw secrets. |
| `agent.run` | `agent` checked identifier/reference, `context`, **static** `output_schema`, `capability_profile?`; output conforms directly to declared schema. No implicit memory/message behavior. Profile omission semantics below. |
| `decision.rule` | `data` (explicitly constructed JSON); existing top-level ordered `rule` and choice routes. Output `{choice: declared enum, data: declared data schema}`; use **static** `data_schema` to validate data. Choices are the explicit route keys; rule cases/default must select them. Pure deterministic. |
| `decision.human` | `proposal`, `reviewer` closed user/group selector, `deadline`, **static** `data_schema`, optional **static** `feedback_required` list of declared choices; output `{choice: enum, feedback: string, data: schema}`. Explicit choice routes supply names; no fixed approve/reject assumption. Deadline required, no automatic acceptance. |
| `decision.agent` | `agent`, `context`, **static** `data_schema`; output `{choice: declared enum, data: schema}`. Choices come from routes, model selects only eligible declared choices later. No arbitrary next-step IDs. |
| `data.map` | `value`, **static** `output_schema`; output is the constructed value itself validated against that schema. No scripts/templates; pure. |
| `http.request` | `connection` fixed declared HTTP slot, `method` finite HTTP-method enum, `path` relative string, `headers?` bounded string map, `body?` JSON; output bounded `{status: integer, headers: object, body: JSON}`. No origin/endpoint/credential override. Compile rejects known invalid paths/forbidden credential headers; resolved-input hook checks dynamic equivalents. HTTP verb never establishes replay safety. |
| `tool.call` | **static** `tool`, `arguments` object; specialize argument/result schemas from explicit caller-supplied tool contract facts. Unknown contract fails. Shared-action capability and conservative effect policy. |
| `mcp.call` | **static** `connection` declared MCP slot, **static** `tool`, `arguments` typed object; require caller-supplied selected-tool contract facts. Output closed envelope `{content: array, structuredContent?: declared schema, isError: boolean}`; normalization to false on absent provider flag belongs handler later, not validator fabrication. No URLs/credentials/authorization headers in configuration. |
| `message.send` | `content` canonical closed `{subject?: string, body: string}`, `destinations` bounded nonempty array of scoped destination objects; output `{message_id: string, status: enum[accepted]}` means required provider acceptance/local commit, never read receipt. Message action capability. |
| `message.reply` | `source_message` explicit checked message identifier, same `content`; same output. Never infer latest message. Preserve provider-neutral domain contract; email transport is later. |
| `workflow.call` | existing top-level pinned child IDs, `input` explicit object; caller-supplied pinned child input/output schema facts specialize `input` and direct output. Child control, inherited budgets/authorization metadata and durable continuation constraint. |
| `flow.repeat` | same child IDs plus existing bounded top-level `max_iterations`, `input` initial child input; output `{result: child output schema, rounds: positive integer}` bounded by max iterations. Registration marks sequential child control and inherited budgets. Full next-round mappings/exit predicate/exhaustion control is phase07, not an invented executable loop here; document this compiler-only boundary explicitly. |
| `wait.event` | `event` scoped event name, `correlation` nonempty string, `deadline`, **static** `payload_schema`; output payload validated directly by that schema. Durable event wait with scoped atomic consumption later. |
| `wait.timer` | `deadline`; output `{deadline: string}`. Durable timer wait, no sleeping worker. |

Deadline wire values are RFC3339 strings: validate syntax now/after resolution, future-relative
checks require runtime clock later. Pure registration checks must not depend on wall clock.
Resource/tool/child fact lookup must be bounded, explicit and side-effect free; no discovery,
authorization or fetching dependencies here. Missing required facts produce diagnostics.
Do not conflate author declarations with approved recovery policies. Keep metadata conservative
when actual action effects depend on later published tool/connection contracts.

For child/repeat facts validate pinned workflow/version identity as well as schemas; retain 02.1
dependency-cycle checks. No worker scheduling, child creation, iteration binding namespace, or
pretend repeat handler in this point. Help must state that execution remains unavailable.

## Capabilities, recovery, optional profiles and help

Each registration exposes meaningful metadata, not a shared placeholder: pure (`data.map`,
`decision.rule`); bounded read (`context.load`, `memory.load`); model invocation
(`ai.classify`, `decision.agent`); agent with possible actions (`agent.run`); local durable write
(`memory.save`); shared action (`http.request`, `tool.call`, `mcp.call`, messages); durable
suspension (human/waits); child control (call/repeat). Supply capability requirements for each
family (none for pure computations; explicit context/memory/model/agent/resource/action/wait/child
requirements otherwise), plus structured execution constraints: deadline/budget ownership,
may suspend, shared action required, sequential repeat, frozen inputs and retry/recovery mode.
Keep requirements distinct from actual authorization grants.

Pure operations can recompute; reads/models must reuse committed results; effects require the
logical idempotency/receipt/reconciliation protocol; waits/children resume durable identities.
Never mark unknown remote writes safe based on HTTP method, MCP metadata or YAML assertions.
No retry counters/scheduler or current-company policy resolution now.

`agent.run.with.capability_profile` is **optional and non-null**. Omission remains omitted in
prepared inputs and records selection mode `agent_defaults`; do not insert an empty profile.
A supplied inline profile is closed `{tools: [...], skills: [...]}`, both lists mandatory;
explicit empty lists mean select none. A supplied name selects only a known profile from explicit
caller-supplied frozen profile facts (no unknown-name fallback). Build the schema from those facts
so dynamic references are validated after resolution too. Profile names/IDs/lists are bounded,
duplicates rejected or consistently normalized without changing saved order. Null, malformed
profiles, unknown selections and missing required references fail. An explicit reference resolving
to null is not omission. No classifier is required. Actual saved-agent capability loading,
grants/ceilings/revocation and skill requirements are phase06 responsibilities; metadata must not
claim those checks happened. Test their compile-time prerequisites, not mock a successful grant.

Authoring help is returned by the registration and includes exact type name, required/optional
fields, schemas, literal-only declarations, routes/control, constraints, effect/retry semantics
and a valid v1 example. Generate field structure from the same schemas where practical; compile
examples in tests so help cannot drift. Update `docs/workflow-language-v1.md` to describe the real
catalogue API and limitations; do not advertise authoring endpoints or executable handlers.

## Acceptance and regression matrix

1. Assert the exact 18-name set (no aliases/omissions); every registration supplies nonempty help,
   schemas and family-appropriate metadata. Table-driven valid compilation for every type, plus
   missing required/invalid-type/unknown top-level and nested configuration fields for every type.
   Allow arbitrary data only in explicitly schema-governed fields. Test runtime input and output
   rejection through the compiled hooks, including schema-valued configuration validation.
2. Two steps of the same type with different schemas/choices retain distinct contracts and facts;
   changed relevant registration/tool/profile/child facts change content identity. Catalogue order
   does not. Preserve bounded fact/dependency preflight; no panic on malformed supplied facts.
3. Valid/invalid profile omission, explicit empty lists, inline/name/static/reference selection,
   null/missing/unknown profile, malformed nested objects and unresolved required references.
   Classifier present but unwired does not alter the agent's omission/default-selection metadata.
4. Choice route mismatch, invalid rule choice/default, human feedback choice outside routes;
   resource missing/wrong kind; tool schema mismatch; MCP typed arguments and downstream
   structuredContent reference; child pin/schema mismatch; repeat zero/over limit; missing/invalid
   wait deadline. Dynamic equivalents fail input preparation where compile cannot prove values.
5. Errors carry stable codes, exact escaped field paths/source spans and bounded actionable
   messages; invalid dynamic provider data does not leak payload/credentials. Preserve all 02.1
   graph/type/schema/parser regressions. No need for broad provider/DB/concurrency tests because
   this point introduces no execution or concurrency protocol.

Run after implementation settles (plus any new tests not matched by `workflow`):

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
```

Refresh graft after substantial code changes; report files, test counts/logs and limitations.
No commit, deployment, database reset, or changes to parent progress files.

## Independent review 1 — corrections required

Reviewer `/root/astra_02_2`, verified thread
`01a0e3dd-c601-7021-a953-9bc355d256b6`. Inspected actual untracked registry/compiler
sources and refreshed graft spans; reused unaffected 02.1 evidence. Reviewed logs for 101/101
workflow tests at 2 MiB, offline all-target check and Clippy, and independently passed
`git diff --check`. No implementation or parent progress files changed. The catalogue covers
the 18 types, per-step schemas/hash, omission/null/empty profile distinctions, and pure runtime
validation hooks, but the following issues prevent acceptance:

1. **P1 — aggregate fact limit runs after amplification.** `registry/mod.rs:64–76` resolves and
   retains every specialized descriptor before calling the compiler; `families.rs:57–59` copies
   all available profile contracts into every agent descriptor. `compile.rs:483–531,598–648`
   compiles/clones those schemas and copies their facts again before the final 1 MiB writer check.
   A bounded near-1 MiB profile catalogue referenced by hundreds of small agent steps therefore
   allocates hundreds of MiB before reporting the 1 MiB limit. Tool/child contract reuse has the
   same multiplication; the explicit-descriptor path at `compile.rs:566–588` also clones by
   step before aggregate validation. Enforce aggregate expanded-fact/schema work and byte
   ceilings **before** retaining each duplicated specialization or compiling its validators;
   reuse immutable shared facts where useful, but ensure the eventual serialized representation
   is bounded too. Keep the existing limit. Test multi-step reuse of large profiles/tool/child
   facts, exact/over limits and the explicit-descriptor path, with evidence that rejection
   precedes amplification rather than merely asserting a late error.

2. **P2 — referenced schema targets escape dialect validation.** New
   `schema.rs:275–285` walks only `schema_budget::schema_children`, whereas the resource guard
   at `schema_budget.rs:49–78` also follows references. A referenced target under a custom
   keyword is a schema even though an unreferenced value there is data. Concrete case:
   `{"$ref":"#/custom","custom":{"$schema":"http://json-schema.org/draft-07/schema#",
   "type":"string"}}`. The new guard, dialect check and root metaschema all accept this;
   native jsonschema 0.33 then accepts a numeric value as well as a string. Without the nested
   `$schema`, the same native reference rejects the number. This was independently reproduced
   in `/private/tmp/workflow-02-2-review-probe.rs` using the actual schema-budget module and
   installed validator (no retrieval). Apply dialect validation to every active/referenced
   schema location, preserving resource scope and bounded traversal. Keep annotation/const/enum
   contents as ordinary data unless explicitly referenced as schemas; do not restore blanket
   rejection of `$schema`-shaped user data. Add direct and chained reference-target regressions,
   plus unreferenced-data controls, and preserve all accepted local-ID/dynamic-ref cases.

3. **P2 — classifier declaration does not ensure finite results.**
   `families.rs:177–202` accepts a required `label`/`profile` enum property without proving the
   result is an object. `{"properties":{"label":{"enum":["support"]}},"required":["label"]}`
   passes that check, but JSON Schema permits any scalar, including undeclared string labels
   and numbers (confirmed with the same native probe). Require the finite declaration to
   constrain every accepted result shape: the property form must exclude nonobjects, including
   scalar alternatives in a type union. Preserve valid scalar enums and object label/profile
   contracts. Add runtime-output tests for unknown scalars, absent/invalid labels and valid
   object/scalar results; malformed configuration must fail compilation.

4. **P2 — MCP catalogue silently narrows the planned result contract to text.**
   `actions.rs:92–106` permits only closed `{type: text, text}` blocks, so image/audio/resource
   and resource-link results fail the output hook. Original phase04 explicitly discusses returned
   resource links and forbids automatically fetching them; the plan did not authorize discarding
   these MCP content variants. Support bounded protocol content variants in the schema (including
   valid empty text and protocol metadata where applicable), retain typed optional
   `structuredContent` and required normalized `isError`, and reject malformed/unknown variants.
   No transport/fetch implementation is needed here. Test representative content variants and
   bounded payload rejection; update documentation's text-only restriction. Treat resource links
   as data, never a request to fetch them.

5. **P2 — shared-action metadata incorrectly forbids suspension.**
   `metadata.rs:100` sets `may_suspend` only for durable waits, child controls and agents.
   HTTP/tool/MCP/message registrations consequently advertise false, despite their shared-action
   approval/reconciliation waits and message-delivery continuation contract. Mark these effect
   families as suspension-capable while retaining conservative replay constraints. Add assertions
   for each shared-action registration (and pure steps remaining false); no execution handler or
   new outcome type is needed.

After these corrections, rerun affected tests at 2 MiB and final fmt/check/Clippy/whitespace gates.
Refresh graft and return for independent review. Do not repeat broad DB/provider checks or change
accepted phase boundaries. The isolated review probe is inert evidence under `/private/tmp`, not
a project dependency or a running resource.

## Independent review 2 — accepted

Reviewer `/root/astra_02_2`, verified thread
`01a0e3dd-c601-7021-a953-9bc355d256b6`. All five Review1 findings are resolved:

- Borrowed expanded-fact scans precede specialization; incremental exact serialization checks
  bound retained descriptors, and complete descriptor/dependency checks precede native validator
  construction. Explicit descriptors receive borrowed preflight before cloning too. Poisoned
  schema fixtures prove early rejection for repeated profiles/tools/MCP/children/repeats;
  exact 1 MiB/one-over and 65,536-node tests retain early failure signals.
- Dialect checks share the bounded active-schema/reference traversal. Custom and chained targets
  now reject unsupported dialects; unreferenced annotation/const/enum data remains accepted.
  Native local resource/dynamic-reference regressions remain green.
- Classifier property contracts require object-only results; scalar enum contracts remain valid.
  Tests reject omitted object types, scalar unions, unknown labels and missing label/profile data.
- MCP schemas include bounded text (including empty), image/audio, embedded text/blob resources,
  resource links and their metadata/icons, consistent with the
  [MCP 2025-11-25 content schema](https://modelcontextprotocol.io/specification/2025-11-25/schema#content).
  URI/icon values remain inert data; typed structured output and normalized isError checks remain.
- All shared-action registrations advertise suspension while retaining conservative recovery;
  pure operations still advertise no suspension.

Inspected actual source and new tests; reused unaffected Review1/02.1 evidence. Verified recorded
109/109 workflow tests at 2 MiB, offline all-target check and Clippy from
`/private/tmp/workflow-02-2-review1-{tests,check,clippy}-verified.log`; parent also verified fmt,
function-size audit and refreshed graft. Independently passed `git diff --check`. No implementation
or parent progress files changed during review. Point **02.2 is accepted** within its documented
compiler-only scope; publication, authorization, executable handlers and later phase gates remain
outstanding as planned.
