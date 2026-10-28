# 01 — Jev step contract and typed questions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Define `jev.evaluate` as one typed workflow step. A separate Jev question ID selects an answer
inside its output. No `ai.evaluate` or `agent.run` behavior changes in this phase.

## Contract to implement

The source declares a `typesafe` resource slot. Required `with` fields are literal `connection`,
literal versioned `model`, dynamic `state`, and literal `questions`. `state` may be a string,
object, or array, within the existing workflow context and request budgets. Keep question
instructions and criteria literal strings in v1 so the compiler can freeze exact semantics and
derive the output schema. Proposed example:

```yaml
resources: [{slot: classifier, kind: typesafe}]
steps:
  judge:
    type: jev.evaluate
    with:
      connection: {literal: classifier}
      model: {literal: jev-1.14.0}
      state: {ref: /input/ticket}
      questions:
        literal:
          department:
            type: choice
            instructions: Which team should handle this ticket?
            criteria: {billing: Payment issues, technical: Bugs or integrations}
          urgent:
            type: noul
            instructions: Does the ticket need immediate attention?
    routes: {success: next}
```

Use one validated tagged question enum: Noul requires instructions and optionally exactly
`true`/`false` criteria; Choice requires an option-name/description map; Score requires an
ordered description list of 2–10 levels. Box is reserved for phase 04 and cannot compile until
its asset contract is present. Reject unknown fields, blank/duplicate IDs, unsupported model
aliases, unreferenced resource slots, and question maps with no entries. Start with a platform
limit of 32 questions, 64 Choice options per question, 10 Score levels, and 16 KiB per instruction
or description; enforce aggregate source, state, request, output, and context budgets before
allocating or sending. A future increase needs a CI check that fails near the raised bound.

Derive the exact output schema from the literal questions: `answers.<id>.type` must match;
Choice has a selected key from its declared options plus the exact probability-key set and
confidence; Score has a bounded numeric score, level-keyed probabilities, legend, and confidence;
Noul has a single `noul` in [0, 1]. Include versioned `model` and nonnegative
`usage.input_tokens`/`usage.output_tokens`. Reject missing, extra, mismatched, nonfinite, or
out-of-range answers at completion. Allow a small documented tolerance for probability sums.
Do not coerce a Noul probability into a Boolean or treat confidence as correctness.

## Batches and checks

1. Add the source/registry type, constraints (`ModelInvocation`, `ReuseCommittedResult`),
   authoring example/help, static shape checks, and compiled hash coverage. Update the exact
   catalogue test and language documentation.
2. Add specialized output and downstream reference validation, including mixed Noul/Choice/Score
   fixtures and invalid-response fixtures. Update request/response size accounting and the
   classification of model-step failures/retry safety without changing action semantics.

Focused commands after batch 1: `cargo test --locked --offline --lib jev_contract_` and
`cargo test --locked --offline --lib workflow_registry_`. After batch 2: those checks plus
`cargo test --locked --offline --lib workflow_compiler_` and
`cargo test --locked --offline --lib workflow_publication_`. First list each filter with
`cargo test --locked --offline --lib FILTER -- --list`; use the actual test names if a filter
matches zero tests. Record command, result, and executed-test count.

Gate: a mixed text request compiles to exact typed output paths and rejects every malformed
question/output shape before any provider call.
