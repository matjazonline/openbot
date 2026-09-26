# 01.1 implementation brief

Expanded by Astra `/root/astra_01_1`, verified session `01a0ddd1-bf31-74c2-a4bd-44b6e17e92f6`. Original requirement: phase 01 Implementation item 1, pure workflow domain identifiers, definitions, graph validation, context resolution, and transition rules. Read original phase and README alongside this brief.

## Scope and repository integration

Add `src/domain/workflow/` and `pub mod workflow;` at `src/domain/mod.rs:1–2`. Existing `src/lib.rs:1–4` already exports domain. Existing string macro in `src/domain/entities/value_objects.rs:12–96` unconditionally derives serde, publicly exposes strings, and accepts arbitrary strings, so use checked workflow-local wrappers instead. Do not alter the shared macro. Existing dependencies uuid, serde_json, thiserror suffice; no lockfile change. Trace callers before modifying any other existing symbol.

No ports, StepOutcome, run-state machine, authorization service, replacement inventory, or causal-link records yet. Phase 02 owns YAML/compiler, complete expression vocabulary, path-sensitive availability, schema/registry checks, source locations, dependency cycles and publication. No legacy compatibility shims. No commits, deployment, database resets or unrelated edits.

## Contracts

- In-memory WorkflowDefinition: format version, identity, schemas, resource requirements, entry, deterministic step map, explicit transitions, execution limits. Schema/resource fields remain descriptive, not advertised as validated or authorized.
- Checked identifiers only where consumed: workflow/version/run/execution UUID types as needed; distinct step, choice/error route, type and resource names. Names use bounded ASCII rules, e.g. 128 bytes and `[A-Za-z_][A-Za-z0-9_-]*`; dotted type names have a separate rule. `$end` is reserved, represented by TransitionTarget::End rather than an ordinary step.
- Private owned ValidatedWorkflow wrapper exposes immutable accessors. Structural validation rejects empty/oversized graphs, missing entry/targets, empty choice maps, duplicate routes, self/multinode/disconnected cycles. Iterative topological validation gives deterministic diagnostics and bounded stack usage.
- Normal routes are explicit Success(target) or Choices(map), with separate final-error routes. Selection takes the validated definition and success/choice/final-error input; it returns declared target or typed error, never mutation/enqueue/retry. Unknown choice cannot fall through. Final-error routing assumes retry eligibility determined by later runtime.
- Initial Binding vocabulary: literal, reference, object, array, explicit default. Resolver borrows immutable input, params, committed step outputs and narrowly structured safe run metadata. Allowed roots: /input, /params, /steps/<id>/output, /run. Validate pointer escapes/indexes/path shape. Existing JSON null is distinct from missing; defaults catch only missing, never malformed/type/budget errors.
- serde_json::Value is an in-memory typed JSON value; no domain YAML parsing, SQL, storage serialization derives, or adapter/application imports.
- Enforce named positive platform ceilings for graph nodes/edges, binding depth/nodes, pointer bytes/segments, JSON depth, aggregate output size/work. Suggested initial ceilings: 1,024 nodes, 8,192 edges, depth 64; implementation may choose justified constants. User limits cannot disable platform caps. Validate before recursive evaluation/cloning; repeated-reference amplification must consume aggregate budget. Deep referenced values cannot bypass limits.
- Typed diagnostic facts identify relevant step/route/reference; YAML locations are later adapter responsibility.

## Bounded edit order

1. Add module registration and module documentation of validation coverage.
2. Add ids.rs, definition.rs, graph.rs, context.rs, transition.rs (combine very small files if clearer).
3. Add meaningful pure tests beside modules; split test files above ~500 lines, keep functions within src guide limits.
4. Verify and inspect diff; refresh graft after substantial module addition.

## Acceptance and checks

Test valid sequential/branch/end graphs; missing entry/target, invalid/reserved IDs, empty graph, self/multinode/disconnected cycles; deterministic insertion-order independence; exact limits and over-limit rejection; large legal chain without recursive graph traversal. Test success/choice/error routing and absent/unknown routes. Test typed JSON preservation, missing vs null, defaults, construction, malformed escapes/indexes, unsupported roots/step paths, context immutability, deep bindings/referenced JSON, excessive nodes and repeated-reference amplification. Near-limit valid graph/context works at 2 MiB stack.

Run:

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

Inspect imports and representation boundary. No database/provider/network fixture or SQLx regeneration is needed. Follow escalation rules for blocked dependency access, preserve locked dependencies. No concurrency protocol yet. Existing full CI/database/stock-stack gates remain required at later meaningful boundaries; do not alter thresholds.

This point proves definition-owned routing, immutable committed-output inputs, and pure trigger-neutral APIs. Durable ports, persisted run model, integrated handlers and replacement map remain unverified until their later points. No blocking product ambiguity. No temporary resources to clean up.

## Review 1 — required corrections

1. context.rs:116: traversing a scalar/null is a type/invalid-reference error, not Missing; defaults must not catch it. Test numbers/booleans/strings/null while direct null remains null.
2. graph.rs:140 and :49: preflight per-step and total route counts before duplicate scanning or cloned/sorted route allocation. context.rs:198 and :228: bound pending child count against remaining node budget before pushing all children, or use depth-bounded iterator frames. Cover wide JSON/references/array and object bindings; rejection must precede unbounded auxiliary allocation.
3. definition.rs:8: add output_schema and minimal typed ResourceRequirement (slot name + compatible kind/contract). Keep schema validation/resource lookup/auth deferred.
4. definition.rs:23: enforce positive ExecutionLimits within platform caps. Document later runtime budget consumption; expose context limits derived from validated definition or explicit enforcement obligation. Test zero, exact cap and over-cap.
5. context.rs:19/:23: replace reference Strings with checked ContextReference; centralize syntax/root/step-path checks. Runtime container/index lookup remains fallible.
6. graph tests: explicitly cover successful Success route, second choice, unknown step and empty choice map.

No acceptance pass yet. Rerun recorded checks after all corrections, then same Astra reviews actual diff. Parent independently confirmed pre-correction 10/10 tests at 2 MiB; those results do not verify corrected code.
