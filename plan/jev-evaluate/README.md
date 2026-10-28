# Jev evaluation workflow step

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and scope

Add one workflow step type, `jev.evaluate`, for TypeSafe Jev's System One API. One step sends one
state and a bounded map of independently evaluated typed questions. The keys in that map are
question IDs; the workflow step ID remains the key under `steps`. The first text slice supports
`noul`, `choice`, and `score`. A later phase adds image assets and `box` before the final gate.

`ai.classify` remains the current tool-free, agent-selected Rig classifier. The discussed generic
LLM `ai.evaluate` type does **not** exist in this repository; defining or implementing it is
separate work. Jev does not enter `agent.run`, the Rig provider registry, or an agent's tool list.
The Jev step has no tool calls or side effects beyond a charged model invocation.

## Implementation sequence

| Phase | File | Deliverable |
| --- | --- | --- |
| 01 | [Step contract](01-step-contract.md) | Source syntax, typed questions and answers, compiler and registry checks |
| 02 | [Company connection](02-connection-and-publication.md) | Encrypted TypeSafe credential, resource binding, frozen publication facts and current authority |
| 03 | [Runtime](03-runtime-and-recovery.md) | Bounded HTTP adapter, model-call accounting, fenced execution and recovery |
| 04 | [Images and Box](04-images-and-box.md) | Authorized image assets and the Box question/answer variant |
| 05 | [Authoring and release](05-authoring-and-release.md) | UI, samples, operating guidance, end-to-end tests and final verification |

Do not deploy a partially implemented step. Keep the phases on one implementation branch until
complete, or gate production registration while compiler/runtime tests develop; the final phase
makes the complete contract available to authors.

## Fixed architecture decisions

- The step is provider-specific: `type: jev.evaluate`, not one step type per question primitive.
  `with.questions` is a literal, bounded tagged map; `with.state` is a normal binding. A question
  that depends on an earlier answer belongs in a later workflow step.
- `with.connection` names a declared `typesafe` resource slot bound to a company-owned TypeSafe
  connection. Store only its ID in a binding/run. A published version pins the selected versioned
  Jev model and question definitions, not the API key. Recheck current company authority and
  connection readiness before provider use.
- Preserve Jev's typed answer fields, model ID, and usage in the validated step output. A Noul
  has only its probability; Choice and Score include probability maps and confidence; Box has
  coordinates and confidence. Downstream `decision.rule` owns thresholds and routing.
- Use an exact versioned model ID in published source, not a moving alias such as `jev-latest`.
  TypeSafe says aliases may move and change answers without a source change.
- The application owns request/response and persistence ports; the HTTP adapter and SQLx
  implementation remain outside it. The request is bounded, deadline-limited, and charged as
  one model call regardless of how many questions it contains.

## Current code and upstream anchors

The step catalogue is `src/application/workflow/registry/`; the compiler is
`src/application/workflow/compiler/`; publication and binding are
`src/application/workflow/publication/` and `src/application/workflow/binding/`. The worker
dispatches via `src/infra/workflow.rs`. Existing model reservation/checkpoint behavior is in
`src/application/workflow/checkpoints.rs` and `src/adapters/persistence/workflow/checkpoints.rs`.
Company model connections in `src/application/use_cases/company.rs` are for agent-selectable
LLMs, so the TypeSafe connection is a separate resource.

Upstream API: [System One](https://docs.typesafe.ai/api/system-one),
[question types](https://docs.typesafe.ai/primitives),
[models and aliases](https://docs.typesafe.ai/models),
[request errors](https://docs.typesafe.ai/api/errors), and
[experimental images/Box](https://docs.typesafe.ai/primitives/box).
Recheck these contracts during implementation. `graft ask` was attempted while scoping this plan
but `graft` was unavailable in this shell; refresh the graph with `graft build` after major code
changes when the tool is installed.

## Verification record

Each batch records the exact command, result, number of tests actually run, and any unavailable
check in its phase file or implementation PR. A filter matching zero tests is not verification.
Run focused checks after each batch, broader checks when a shared compiler, publication, binding,
or execution boundary changes, and the full suite once after all phases. Phase 05 lists the
required final commands. This plan records intended checks only; none has run yet.
