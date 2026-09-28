# 02.1 — Definition and context contract

Original phase02 **Definition and context contract** and phase acceptance remain authoritative.
Expansion only by Astra `/root/astra_02_1`, verified `CODEX_THREAD_ID`
`01a0e37f-aa52-7840-b946-72708d9246ee`. Phase01 is committed at `e5909d6`;
preserve it and parent-owned PROGRESS/RESUME changes. No blocking product decision.

## Scope and boundaries

Implement the language/compiler foundation now: bounded YAML decoding with source mapping,
typed bindings, structural and static context validation, schema checks, immutable compiled
artifact and runtime validation interfaces. Concrete registered step catalogue is **next**;
publication commands, frozen dependency bundles, binding authorization and storage are later
phase02 points/phase03. No production executor, handlers, SQL, routes or legacy replacement.
Do not mark the whole phase acceptance complete after this point.

Compiler takes an explicit descriptor catalogue supplied by its caller. This point provides
the narrow contract and test registrations, not fake implementations of the named production
steps. Unknown types fail against that catalogue. Descriptor facts needed now: input/output
schema, allowed route shape/declared choices, and optional typed control/dependency facts.
Later registry adds real config validation, capabilities, effect/retry policy and authoring help.
Provide no default-success callbacks. A structural `ValidatedWorkflow` is not a compiled or
publishable workflow. Keep this distinction explicit in types and docs.

## Existing code and bounded edits

- `domain/workflow/definition.rs:8–59`: existing identity/schemas/resources/limits/step bindings,
  success-or-choice routes and separate final error. Extend only as needed; retain checked IDs.
- `domain/workflow/context.rs:17–109,142–194,264–366`: checked JSON pointers, immutable borrowed
  context, missing-only defaults, null semantics and byte/depth/work bounds. Extend this resolver,
  do not create a second expression engine. Split tests/module before size limits are exceeded.
- `domain/workflow/graph.rs:47–65,172–231`: immutable structural wrapper, bounded DAG and
  reachability checks. Reuse it; add pure availability/dependency analysis in separate modules.
- `domain/workflow/transition.rs:29–62`, `outcome.rs:186–227`: existing routing/retry ownership
  remains authoritative. No handler-selected targets, fall-through or error route before retry
  exhaustion. Preserve prior outcome tests.
- New `application/workflow/compiler/` owns schema compilation and compilation orchestration;
  adapter-owned YAML decoding/source spans can live under `adapters/workflow/`. Define the
  consuming source-decoder contract in application; no application import of adapters.
  Domain owns pure typed expressions/graph facts, never YAML or external I/O.
- Existing `application/services/tool_schema.rs:5–66` supplies bounded schema resource checks
  and `NoExternalReferences`; reuse narrowly or factor only genuinely common logic. Its generic
  compile is not itself metaschema validation. `adapters/response_schema/mod.rs:11–24` shows
  explicit Draft202012 meta-validation. Do not import that adapter into application.
- Existing `OwnedVersion` (`application/workflow/contracts.rs:58–63`) and admission
  (`service.rs:51–81,139–178`) are structural-only foundation callers. Do not silently turn them
  into proof of publication or weaken their company/resource authorization. Compiler runtime
  hooks are tested here; authoritative published admission integration belongs publication/runtime.

Graft callers for WorkflowDefinition/ValidatedWorkflow had no indexed edges; exhaustive graft
grep found domain graph/outcome tests plus application workflow contracts/service/tests. These
are the constructor updates to preserve if fields change. `resolve` is an ambiguous indexed
name: trace actual workflow uses before edits rather than trusting its unrelated callers.
Read root/src/application guides; functions ~80 lines, no monolithic modules, no upward imports.

## Language and artifact decisions

1. Format version 1 is explicit; reject unsupported versions. Document one wire spelling per
   field (recommend existing names: `workflow_id`, schemas, `resources`, `entry`, `steps`,
   `limits`; step `type`, `with`, explicit success/choices/error transition object).
   Version identity comes from compilation/publication context, not a tenant-chosen published
   version claim. Schemas are explicitly declared; `true` may express unconstrained JSON.
   Missing required definition fields fail; no YAML-order execution semantics.
2. Bindings are unambiguous one-operator objects: `{literal: VALUE}`, `{ref: POINTER}`,
   `{object: {key: BINDING}}`, `{array: [BINDING]}`, `{concat: [BINDING]}`,
   `{default: {ref: POINTER, value: BINDING}}`, comparisons (`eq/ne/lt/le/gt/ge` with
   two bindings), membership (`in` with value and array bindings), `{exists: POINTER}`,
   `{and: [...]}`, `{or: [...]}`, `{not: BINDING}`. Exact names may follow a clearer existing
   convention but must have one documented grammar and reject mixed/unknown operands.
   Literal wrapper escapes operator-shaped JSON. Never evaluate templates, scripts or code.
3. Concat accepts strings only; membership uses typed JSON equality in arrays; ordering accepts
   numbers or strings of the same kind (no string/number coercion or lossy integer-to-float
   shortcuts). Booleans require booleans; ordered short-circuit evaluation is deterministic.
   Null is present and remains null. Default/exists catch **Missing only**; malformed pointers,
   scalar traversal, incompatible operands and budget failures remain errors. All new operands,
   including unselected branches, count toward binding size/depth preflight; evaluated work and
   resulting allocations consume limits. Document equality semantics explicitly.
4. Deterministic rules use ordered predicate/choice cases plus a mandatory default declared
   choice. Add a pure checked rule contract/evaluator using the same bindings and choice names;
   actual `decision.rule` registration/execution is next/later work. Unknown case/default choices
   reject; first true case wins. No implicit YAML map order for cases.
5. Compiled artifact owns exact source, deterministic serializable compiled representation,
   source map, and SHA-256 content identity. Hash versioned, unambiguous canonical data; define
   whether source bytes participate (recommended: yes), and include supplied schema/descriptor
   facts affecting compilation. No timestamps/random IDs in content identity. Keep fields private
   with read access; changing source produces a new artifact. Hash is integrity, not authorization.
   Publication later includes frozen dependency hashes and atomically stores source/compiled/hash;
   this point does not claim database immutability/archive semantics are implemented.

## Static availability and schema contracts

Compute must-available committed outputs in topological order. Entry has none. Each incoming
success/choice edge carries predecessor's available set **plus predecessor output**; an error
edge carries only its prior set. Intersect all incoming edge sets, even when success/error share
a target. Thus merely dominating a step is insufficient if that predecessor can fail. Self,
future, sibling-branch and post-merge optional outputs reject without explicit default. An exists
probe may observe absence; it does not justify a separate unsafe reference. No predicate-based
schema refinement is required initially; authors can supply defaults.

After output availability, prove referenced property/index availability from root/output schema:
required properties at every level, compatible container types, and guaranteed array length.
`run.id` is string; `run.parent_id` is always present string-or-null, preserving phase01.
Schema alternatives require guarantees in every viable branch. Handle local references with
bounded resolution; conservatively unknown guarantees require explicit defaults. Never treat
`properties` alone as required or an unknown schema as proof. Clearly impossible paths/type
errors fail even with defaults; optional but feasible paths can default. Reject unknown step IDs
even under default/exists. Do not silently infer output from an example value.

Use Draft202012 metaschema validation, explicit dialect checks, existing bounded schema limits,
no external/file/network retrieval, bounded local-reference expansion and diagnostics. Full
runtime schema validation and conservative static projection are separate: unsupported static
proof does not imply a schema passed a proof. Validate literal/subtree types and known impossible
bindings against destination schemas; unknown types remain subject to runtime checks, never
coercion. Report the lack of a required path guarantee actionably.

Expose checked activation preparation that resolves all step inputs into one bounded immutable
snapshot and validates the aggregate input schema; expose output validation before any commit
and workflow input/params/output validation. Tests use these APIs directly. Phase03 must persist
prepared inputs atomically before dispatch and retry from those exact saved values; no helper
may claim it has persisted them. A step output API accepts only its own output, never mutable
prior context. No actual persistence/retry worker is introduced here.

## Cycles, bounds and diagnostics

Reuse graph DAG checks over **all** routes, including unreachable cycles and error edges.
Child dependency validation takes bounded explicit dependency facts/closure supplied by the
caller; reject direct/indirect recursion, missing referenced nodes and mismatched identities.
Later publication resolves company-scoped pinned versions and supplies authoritative facts.
Repeat is a typed bounded child-control fact, not a backward edge: positive capped iteration
count required. Do not implement phase07 child scheduling or feedback state. Avoid inferring
dependencies through arbitrary untyped JSON/string searches.

Although parser hardening is named in the next section, a compiler exposed now must already
bound source bytes, depth, nodes/scalars before dangerous allocation and reject duplicate keys,
custom tags, multiple documents and aliases/anchors (rejecting all aliases is acceptable).
Reuse locked dependencies where possible; do not substitute regex-only YAML parsing or guessed
line searches. `serde_yaml` alone does not provide a complete semantic source map: choose a
bounded span-aware decoding strategy and prove it with repeated keys at different paths, quoted
keys, flow/block forms and Unicode. A necessary parser dependency is allowed with lockfile and
normal network approval rules; stop for a design amendment if safe source mapping cannot be
provided, rather than claiming whole-document location as exact field location.

Diagnostics carry stable code, human action, semantic field path and real source byte span plus
one-based line/column; runtime value errors additionally identify instance/schema paths. Missing
fields point to enclosing mapping; unknown targets/references point to the offending scalar.
Bound error count and message size. Do not embed source bodies/credentials in errors or logs.

## Acceptance and evidence

Test YAML→typed compiled graph, deterministic source/content identity and mutation isolation;
malformed graph/route/format/field/schema/type; no implicit fall-through; ordered rules/default;
all binding operators including exact integer/number behavior, wrong operand types, null versus
missing and missing-only fallback. Cover diamond joins, failed-output error edges including
same-target success/error, self/future references, optional nested properties, nullable traversal,
arrays, local schema refs and union guarantees. Cover DAG/child cycles and missing/zero/over
repeat bounds through supplied descriptor/dependency contracts. Runtime hooks reject bad inputs
and handler outputs; a prepared snapshot stays unchanged after underlying context changes.

Parser tests include duplicate/unknown keys, tags/alias amplification, size/depth/node boundaries,
accurate semantic locations and safe bounded failure at 2 MiB. Avoid thousands of repetitive
cases: use meaningful tables and focused adversarial fixtures. No database/service resources
required and no concurrency protocol changed; existing races remain regression coverage.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
```

Add a targeted filter if adapter/compiler tests are not selected by `workflow`; record exact
commands/logs. Refresh graft after substantial changes. No commits/deploy/reset. Later phase
gates still owe the real registry, unauthorized binding tests, immutable dependency publication,
fixtures and database-backed checks. No temporary-resource cleanup currently required.

## Review 1 — corrections required

Reviewed actual tracked and new source, relevant callers and finish-worker logs. The 67 tests,
offline all-target check and Clippy pass, but acceptance is not yet met. Prior phase01 ownership,
authorization and structural-only integration remain intact. Correct these together:

1. **P1 dependency bounds:** `compiler/analysis.rs:241–300` takes an unrestricted dependency
   graph and recurses without a depth/node/edge budget. `compile.rs:518–537` also copies/hashes
   the entire supplied graph, including unrelated entries. A long acyclic chain can overflow a
   stock stack; a large supplied graph bypasses YAML limits. Preflight all serialized facts,
   impose named node/edge ceilings and use iterative traversal (or a safely capped depth).
   Test exact/over limits, a long allowed chain at 2 MiB, missing facts and cycles. Documentation
   currently promises a bounded graph without enforcing it.
2. **P1 aggregate activation budget:** `compiler/compile.rs:155–175` grants each input field
   the full context budget, clones it and accumulates it before checking the aggregate. Hundreds
   of bindings referencing one near-1MiB value can allocate hundreds of MiB before rejection.
   Resolve the aggregate binding with shared work/byte accounting, charging keys/containers
   before allocation. Test repeated-reference amplification across sibling input fields, not
   just inside one binding, and exact aggregate limits.
3. **P2 static-check bypass:** `compiler/compile.rs:333–346` only checks binding operators when
   a destination property/additionalProperties schema exists. A descriptor with input schema
   `true` or `{type: object}` accepts `{concat: [{literal: 1}]}`, including errors in skipped
   branches. Whole input schema `false` or a scalar-only schema is also never checked against
   the constructed input object. Always validate operator structure/types and the aggregate
   input shape. `shape.rs:162` additionally skips every known mismatch when any `$ref` occurs:
   a literal number against a local-reference string schema compiles. Preserve root context
   when resolving destination schemas; unknown inference may defer, already-known impossibility
   must not. Include root true/false/scalar, unspecified properties, local refs and skipped
   branches in regression cases. Check JSON Schema's integral `1.0` semantics rather than
   rejecting it solely because serde stores that number as f64.
4. **P2 projection/runtime disagreement:** `schema.rs:176–205` merges a forbidden/missing
   object property branch as `UnsafeTraversal`. For an anyOf of two closed object schemas,
   where one permits/requires `message` and the other forbids it, an explicit default for
   `/input/message` is valid: the latter produces Missing, not scalar traversal. Distinguish
   possible absence from wrong container type. Also `schema.rs:357–361` accepts `+1` through
   `usize::parse`, while runtime pointer indexing rejects it (`context.rs:200–205`); reuse
   canonical decimal-index rules. Test both union/default runtime paths and malformed indexes.
5. **P2 concat limit regression:** `context.rs:473–479` debits each part during evaluate, then
   compares the accumulated result against the *remaining* budget. `{concat: [{literal: hello}]}`
   with output_bytes=7 rejects although the same literal and serialized result fit exactly.
   Avoid double-charging accumulated contents; retain bounded transient allocations and final
   serialization checks. Test exact/one-over limits for one and several parts, nested concat,
   and references.
6. **P2 descriptor panic:** `compiler/compile.rs:285–288` uses unreachable for a descriptor
   with `ordered_rule=true` and Success routes. Such caller-supplied facts pass the route
   check above and panic when a rule is present. Validate inconsistent descriptors with a
   diagnostic; test malformed descriptor contracts without unwinding.
7. **P2 escaped diagnostic paths:** `compiler/compile.rs:156,318` interpolate unrestricted
   input names directly into JSON-pointer paths instead of using `syntax::child`. Input keys
   containing `/` or `~` therefore produce wrong field paths and fall back to the document span,
   despite the correctly escaped source map. Use the common path builder in compile/runtime
   checks and test Unicode plus escaped names with exact source bytes. Preserve the underlying
   runtime error category as an actionable bounded message: `compile.rs:158–165` currently
   collapses missing/type/budget failures into the same "Unable to resolve step input".

After corrections, finish the source-guideline audit: `compile_tests.rs` is over the ~500-line
test-module threshold. Split along meaningful test groups with shared fixtures. Rerun the
affected targeted tests at 2 MiB and listed format/offline check/Clippy gates once edits settle;
then return for review. No implementation was changed during this review.

## Review 2 — one remaining static-schema correction

Review1 findings 1, 2, 4, 5, 6 and 7 are resolved by inspection and focused regressions:
bounded iterative dependency closure before serialization, sibling-shared activation limits,
union absence versus traversal plus common index parsing, exact-limit concat, descriptor
diagnostics, escaped field paths and distinct runtime errors. Test modules are split. Recorded
76/76 tests at 2 MiB and offline check/Clippy pass; independent whitespace check passes.

**P2, finding3 remains partially open.** Destination local-reference handling still misses
statically known invalid input. `compile.rs:393–395,414–420` reads properties/required directly
from raw schema; only `object_possible` resolves a root reference. Thus a descriptor input schema
`{"$ref":"#/$defs/args","$defs":{"args":{"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}}}`
accepts `message: {literal: 1}` (and can miss required fields).

Also `shape.rs:183` skips a literal's validation when a nested schema contains any `$ref`.
For input schema `{"type":"object","properties":{"message":{"type":"object","properties":{"x":{"$ref":"#/$defs/text"}},"required":["x"]}},"$defs":{"text":{"type":"string"}}}`,
`message: {literal: {x: 1}}` compiles: the outer object type matches and the nested reference
causes validation to be skipped. These are fully known literals, not unknown dynamic inference.
`schema.rs:102–120` also replaces reference objects with their target, discarding Draft202012
sibling constraints; e.g. `{$ref: '#/$defs/anything', type: string}` with `anything: true` must
still reject a numeric literal.

Preserve enclosing schema context and sibling constraints for static validation; remove the
blanket nested-reference escape. Do not solve this by banning local references or weakening the
runtime/static contract. Add focused regressions for root-ref required/property checks, nested
literal local refs, and `$ref` siblings, with corresponding valid cases. Reuse unaffected review
evidence; rerun affected tests at 2 MiB plus final fmt/check/Clippy. No implementation changes
were made by the reviewer. Session remains `01a0e37f-aa52-7840-b946-72708d9246ee`.

## Review 3 — preserve all accepted reference scopes

Independent reviewer session `01a0e3cd-363a-7c73-9b0b-dc1f71735540`. The three Review2
fixtures are corrected: root local-ref properties/required fields are inspected, nested literals
are validated, and `$ref` siblings remain constraints. Reused unaffected Review2 evidence;
inspected the 79/79 workflow test and offline all-target check/Clippy logs, and independently
passed `git diff --check`. No implementation or parent progress files changed.

**P2: scoped literal validation rejects valid schemas accepted by the original validator.**
`schema.rs:122–198` moves the root beneath `$defs/__workflow_root` and rewrites only `$ref`,
without preserving schema resource boundaries. Two concrete cases remain:

- An input schema `{type: object, properties: {message: {$dynamicRef: '#/$defs/text'}},
  $defs: {text: {type: string}}}` passes the existing resource guard and Draft202012 validator.
  A valid `{literal: ok}` is rejected by the new scoped validator: `$dynamicRef` is left pointing
  at the wrapper's absent `/$defs/text`. The same issue applies inside a known nested literal.
- An input schema with `$id: https://example.test/args`, `properties.message.$ref:
  '#/$defs/alias'`, and `$defs: {alias: {$ref: '#/$defs/text'}, text: {type: string}}`
  accepts `{message: ok}` originally. After relocation, entering `alias` retains the `$id`
  resource boundary, so its rewritten `#/$defs/__workflow_root/$defs/text` looks inside that
  resource rather than the wrapper and fails. No external retrieval is needed by the original.

Verified both with the repository's already-built jsonschema 0.33.0 validator in an isolated
`/private/tmp/workflow-schema-review.rs` probe: original schemas accept the valid string;
scoped schemas fail with `PointerToNowhere`. The dynamic-reference original also rejects a
numeric value. Preserve reference semantics for schema forms the compiler already accepts,
including local `$dynamicRef` and `$id` resource boundaries; do not quietly narrow the accepted
schema contract. Add corresponding valid/invalid literal regressions and rerun the affected
2 MiB tests plus final gates. Point 02.1 is not yet accepted; earlier findings remain closed.

### Review3 design amendment — native resource selection, no relocation

Expansion only by the same reviewer; original acceptance and limits are unchanged. The installed
jsonschema 0.33.0 already supplies the needed mechanism (`options.rs:258,284`,
`Draft::Draft202012.create_resource`). Prefer this narrow implementation:

1. Delete `relocate_local_refs`. Keep the complete accepted root schema byte-for-byte unchanged
   as an in-memory `Resource`. Compile a small selector schema whose `$ref` addresses the desired
   subschema by its JSON-pointer location in that resource. Retain Draft202012,
   `NoExternalReferences`, and the existing regex limits. This delegates resource scope and
   reference-keyword semantics to the same library used for runtime validation.
2. The following exact API shape was verified locally (no new dependency or retrieval needed):
   `options().with_draft(Draft::Draft202012).with_retriever(NoExternalReferences)`
   `.with_resource("json-schema:///", Draft::Draft202012.create_resource(root.clone()))`
   `.with_base_uri("urn:workflow:selection").build(&selector)`, where `selector` is
   `{"$ref":"json-schema:///#/properties/message"}` for that subschema. `json-schema:///`
   is the installed library's default base used by the current original validator; retain the
   same base for relative `$id` resolution. The selector's base is separate from the registered
   root. Test author `$id` collision with the selector base too (the local probe passes).
3. Carry a subschema location or find it within the already-bounded root by **reference identity**,
   never structural JSON equality: identical fragments can live under different `$id` scopes.
   Every selected value currently originates in the raw root. A bounded walk may recover its
   path without a broad cursor refactor. Use JSON-pointer token escaping and URI-fragment percent
   encoding; `/`, `~`, `%`, `#`, spaces and Unicode property names must select the exact location.
   Do not log source/schema contents or invent success if a location cannot be recovered.
4. In `shape.rs::check_binding_shape`, send fully known literals to this native scoped validator
   **before** coarse `local_constraints`/kind shortcuts. Those shortcuts currently use
   `root.pointer` and cannot independently prove a literal mismatch across a nested `$id`.
   This is not a skipped static check: the whole literal receives full validation, preserving
   nested constraints and siblings. Keep nonliteral operand/shape checks and root required/input
   checks; do not replace them with unconditional success or remove runtime validation.
5. Preserve existing preflight bounds before cloning/building. Root is already limited to
   65,536 bytes, depth32 and 4,096 expanded visits. Bound any location walk by that accepted tree;
   use a constant-size selector plus its bounded encoded path. Do not add remote resolution or
   an unbounded registry/catalogue. A per-schema cache is optional, not required for this fix.

Primary edits: `schema.rs::validate_known_literal` and the relocation helper replacement;
`shape.rs::check_binding_shape` ordering; focused `compile_validation_tests.rs` regressions.
Split helpers/modules if local source-size rules require it. `compile.rs` should need no broad
rewrite. Existing root-ref, nested-literal and sibling tests must remain intact.

Required focused cases: valid and invalid literals for local `$dynamicRef`; root `$id` plus a
two-link local `$ref` chain; a nested `$id` whose local `$defs/text` is string while the enclosing
root's same-named definition is number (must follow the nested scope); relative `$id`; escaped
and Unicode destination names; identical fragments in different scopes; selector-base `$id`
collision. Keep external refs rejected and run at 2 MiB. The isolated native-resource probe
`/private/tmp/workflow-schema-registry-review.rs` verified plain refs, dynamic refs, root-ID
chains, nested IDs, relative IDs and selector-base collision, each accepting strings and rejecting
numbers exactly like the original full validator. Final fmt/workflow/check/Clippy gates remain
the brief's original commands. No production code was edited during this expansion.
