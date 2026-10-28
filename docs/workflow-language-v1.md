# Workflow language v1 compiler contract

This document describes the phase 02 compiler and its 19-type catalogue. The
application provides library validation/publication commands, but does not yet accept
workflow YAML through an authoring API, persist prepared inputs, or execute these definitions.
`application::workflow::registry::compile` uses the built-in catalogue and explicit
caller-supplied tool/pinned-child and dependency facts. The lower-level
`compiler::compile` remains available for explicit descriptor callers and tests. A structurally `ValidatedWorkflow` is not a compiled or
publishable workflow.

The YAML document has exactly these required fields: `format_version: 1`,
`workflow_id` (UUID), `input_schema`, `parameter_schema`, `output_schema`,
`resources` (array), `entry` (step ID), `steps` (map), and `limits` with positive
`max_steps` and `max_context_bytes`. A schema may be `true` to accept any JSON
value. The version ID comes from compilation/publication context and is not a
field in the source. Each step requires `type`, `with`, and `routes`. Its `routes`
mapping declares exactly one of `success: <target>` or `choices: {<choice>:
<target>}`, plus optional `error: <target>`. Targets are checked step IDs or
`$end`; YAML order never supplies a transition. An error route is for the runtime
to select after safe retries have been exhausted. The compiler does not decide
retry eligibility.

Each `with` value is a one-operator binding mapping. Operators are:

| Form | Meaning |
| --- | --- |
| `{literal: VALUE}` | Preserve a JSON value, including `null` and operator-shaped objects |
| `{ref: /input/name}` | Read a checked JSON pointer from `input`, `params`, `steps.<id>.output`, or safe `run` metadata |
| `{object: {key: BINDING}}`, `{array: [BINDING]}` | Build JSON objects and arrays |
| `{concat: [BINDING]}` | Concatenate strings only |
| `{default: {ref: POINTER, value: BINDING}}` | Evaluate fallback only for a missing reference |
| `{eq: [A, B]}`, `{ne: [A, B]}`, `{lt: [A, B]}`, `{le: [A, B]}`, `{gt: [A, B]}`, `{ge: [A, B]}` | Compare two typed values |
| `{in: [VALUE, ARRAY]}` | Test typed membership in an array |
| `{exists: POINTER}` | Test presence, including present `null` |
| `{and: [BINDING]}`, `{or: [BINDING]}`, `{not: BINDING}` | Compose booleans with ordered short circuiting |

Equality compares JSON recursively; JSON numbers compare by exact decimal value,
so `1` equals `1.0`, without an integer-to-float conversion. Ordering accepts
two numbers or two strings. Other operand pairs fail. `default` and `exists`
catch missing values only; invalid pointers, scalar traversal, type errors and
budget failures remain errors. The compiler preflights all binding branches even
if evaluation later short circuits.

`run.id` is a string and `run.parent_id` is always present as a string or `null`.
`run.deadline` is the run's own deadline, fixed at admission and inherited by child runs, as a
UTC RFC3339 string with microsecond precision (`2030-01-01T00:00:00.000000Z`); bind it where a
step needs a deadline that moves with each run, such as a `decision.human` that should expire
with its run.
Before a step starts, the caller must use `prepare_step_inputs` and durably save
its owned snapshot; retries must reuse that saved value. The helper performs no
I/O. Call `validate_step_output` before committing the named step's own output.
Workflow input, parameters, and final output have separate runtime validators.
Static reference checks require a prior step output on every incoming path;
error routes carry no output from the failing step. Optional schema paths require
an explicit `default`. Clearly impossible paths and known scalar traversal fail
even with a default. Runtime schema validation remains authoritative.

A descriptor with an ordered rule requires `rule: {cases: [{when: BINDING,
choice: NAME}], default: NAME}`. Cases are evaluated in sequence and the first
true predicate wins. Every selected and default choice must be declared in
`routes.choices`. A child-control descriptor requires `child_workflow_id` and
`child_version_id`. A repeat descriptor also requires positive
`max_iterations` at most 10,000. The caller supplies a bounded child dependency
graph; the compiler checks it for missing nodes, mismatches and recursion. Child
scheduling and iteration state belong to later phases.
Dependency facts are limited to 4,096 nodes and 8,192 edges. Only nodes reachable
from this workflow enter its compiled identity; the serialized descriptor and
reachable dependency facts have a combined 1 MiB limit.

The decoder limits source to 256 KiB, depth to 48, nodes to 8,192, scalar bytes
to 256 KiB and one document. It rejects duplicate keys, merges, anchors,
aliases and custom tags. Diagnostics include a stable code, semantic field path,
byte span and one-based line/column; runtime schema errors also include instance
and schema paths. Messages are bounded and never include source bodies.

The compiled artifact exposes read-only source, a deterministic JSON
representation, source map, validated graph and SHA-256 identity. The hash frames
the exact source bytes and the normalized descriptor/dependency facts under
`workflow-compiled-v1`; it is an integrity identity, not authorization or a
published dependency bundle. Publication later freezes dependencies and stores
source, compiled representation and hash atomically.


## Registered contracts and authoring help

`registry::TYPES` is the exact 19-name set. `registry::example(type_name)` returns
valid v1 source plus illustrative caller facts; `registry::authoring_help` compiles
a supplied example and returns its specialized input/output schemas, literal-only
fields, routes/control, constraints, notes and source. Required and optional input
fields come directly from the input schema. Examples use JSON (a YAML subset) so
literal wrappers and route spelling are unambiguous. The schema syntax below is
for **resolved values**; every `with` field still needs a binding wrapper.

| Type | Required `with` fields | Optional fields / result |
| --- | --- | --- |
| `context.history` | `max_tokens` | Result `{items: [{message_id, at, role, audience, entry_kind, author, subject, body, truncated}], token_count, omitted}`: the run's thread history frozen at admission, internal notes excluded |
| `memory.load` | `scope`, `query`, `limit` | Result `{items: [{id, source, content}]}`; registered but unhandled: memory is not in this release |
| `memory.save` | `scope`, `facts` (nonempty strings) | Result `{saved_ids: [...]}`; registered but unhandled: memory is not in this release |
| `jev.evaluate` | literal `connection`, `model`, `questions`; dynamic `state` | TypeSafe typed questions and exact answers; see Jev contract below |
| `ai.classify` | `agent`, `context`, literal `output_schema` | Tool-free call with the agent's instructions and model; direct schema result; finite string enum, or required `label` enum property |
| `agent.run` | `agent`, `context`, literal `output_schema` | Runs with exactly the agent's saved tools and skills; direct schema result |
| `decision.rule` | `data`, literal `data_schema` | Top-level ordered `rule`; result `{choice, data}` |
| `decision.human` | `proposal`, `reviewer`, `deadline`, literal `data_schema` | Optional literal `feedback_required` choice list; result `{choice, feedback, data}` |
| `decision.agent` | `agent`, `context`, literal `data_schema` | Result `{choice, data}` |
| `data.map` | `value`, literal `output_schema` | Direct constructed value, validated against the schema |
| `http.request` | literal `connection`, `method`, `path` | Optional `headers`, `body`; result `{status, headers, body}` |
| `tool.call` | literal `tool`, `arguments` | Supplied tool schemas govern arguments and direct result |
| `mcp.call` | literal `connection`, literal `tool`, `arguments` | Result `{content, structuredContent?, isError}`; selected tool governs structured data |
| `message.send` | `content`, `destinations` | Result `{message_id, status: accepted}` |
| `message.reply` | `source_message`, `content` | Same acceptance result |
| `workflow.call` | `input` | Top-level pinned child IDs; child contract governs direct result |
| `flow.repeat` | `input` | Same pins plus top-level `max_iterations`; result `{result, rounds}` |
| `wait.event` | `event`, `correlation`, `deadline`, literal `payload_schema` | Direct schema-validated payload |
| `wait.timer` | `deadline` | Result `{deadline}` |

Decisions derive eligible names from `routes.choices`; there are no fixed
approve/reject names. A required human feedback choice needs nonempty feedback.
Schemas remain Draft 2020-12 with native local resource/reference semantics, not
closed configuration maps. Classifier object contracts require `type: object` (or `[object]`) and a required
finite `label` enum; scalar alternatives are rejected. There is no second
`config` authority.

Registration-owned objects reject unknown fields. Scope is `{kind, id?}` where
kind is company/agent/user and agent/user require an ID. Reviewer is `{kind, id}`
with user/group kind. Destinations are `{kind, id}` with user/group/channel kind.
Content is `{subject?, body}`. MCP content accepts bounded text (including empty text), image/audio base64, embedded
text/blob resources and resource links, with protocol annotations, icons and `_meta`.
Resource links and icon URIs remain data and are never automatically fetched. These fields follow
the [MCP 2025-11-25 schema](https://modelcontextprotocol.io/specification/2025-11-25/schema).
Structured results use the selected output schema. A handler
must normalize an absent provider `isError` flag before validation; validators
never fabricate it. User context/proposals/HTTP bodies may contain arbitrary JSON
within the workflow context budget; arguments, mapped values, child inputs and
results follow their explicit schemas. HTTP headers are a bounded string map.

An agent's capabilities are defined on the agent. `agent.run` uses exactly the
agent's saved workflow tools and skills, frozen when the workflow version is
published; there is no step-level capability profile and no workflow-level
capability ceiling, so a step can neither add nor narrow a capability.
`ai.classify` and `decision.agent` use only the selected agent's instructions and
model, with no tools and no skills. Agent edits take effect on republish. At
execution the runtime refuses a model step (terminal `agent.rejected`, before any
provider call) when its agent or model is no longer available, or when a prepared
tool or skill has since been removed from the agent.

All identifiers/selections are bounded to 128 characters; selections have
at most 128 tools, skills or destinations. Collections of context/memory
items, saved facts/IDs and MCP content blocks have at most 256 entries. Text fields
are at most 16,384 characters, context token requests at most 131,072, paths 2,048,
correlations 256, and RFC3339 deadlines 64. Header maps have at most 64 properties,
names at most 128 characters and values at most 16,384. The workflow's context-byte
budget additionally bounds resolved input and output JSON, including user data.
MCP base64 payloads are at most 16,384 characters, MIME types and resource names
128, resource URIs 2,048, and metadata maps 64 properties with 128-character keys.
Resource links allow at most 128 icons; each icon source is at most 16,384 characters,
with at most 128 size strings of 128 characters.
Supplied tool/child contracts total at most 256 entries. Borrowed expanded
facts are checked before specialization (1 MiB and 65,536 schema nodes); the exact
serialized descriptor/dependency facts also have a 1 MiB ceiling before validators. Each declared schema uses the existing bounded schema
validator. Duplicate facts fail; named fact lookup is side-effect free.

HTTP resources use kind `http`; MCP resources use kind `mcp`. Literal connection
slots must exist with that kind. These checks do not establish company ownership
or readiness. HTTP methods are GET/HEAD/POST/PUT/PATCH/DELETE/OPTIONS. Paths cannot
supply an origin; credential/origin headers and header line breaks fail, including
after dynamic binding resolution. Deadlines are checked as RFC3339 without a
runtime clock; future-relative checks belong to execution. Context admission
cutoff is a frozen run fact and cannot be configured by the author.

Each registration records capability requirements separately from grants,
conservative effect/recovery semantics, inherited run budgets, frozen input and
runtime deadline requirements, suspension and shared-action constraints. Pure
map/rule steps may recompute; reads/models reuse committed results; effects need
logical idempotency, receipts and reconciliation; waits/children resume durable
identities. Shared HTTP/tool/MCP/message actions may suspend for approval,
reconciliation or delivery continuation. HTTP verbs, MCP metadata and author declarations never establish safe
replay. Existing failure classes/retry eligibility retain their domain meaning.

Specialized schemas, selected tool/child facts,
registration revision and recovery/capability constraints are hashed by **StepId**.
Two same-type steps can have different contracts; catalogue ordering does not
change identity. This is compiler metadata, not an approved publication bundle.
`flow.repeat` describes bounded sequential child control only. Next-round mapping,
exit predicates, exhaustion policy and scheduling are phase07; there is no pretend
executable loop, provider call, timer sleeper or handler in this catalogue.

## Operator starters and company copies

`templates::TemplateCatalogue::build` constructs a trusted composition-time,
immutable starter catalogue using the real bounded YAML decoder and step registry
compiler. This workflow-template catalogue is distinct from the step-type registry.
Assembly supplies illustrative tool/child facts; these are validated and
then discarded, not installed or granted to a company. Future operator ingress
must enforce the account/config operator policy before catalogue construction.
Distribution, operator editing and server configuration are not wired.

The complete offer list is checked before decoding or compiling: at most 64 entries,
nonempty titles of at most 256 bytes, descriptions of at most 4,096 bytes, each
source at most 256 KiB, and at most 2 MiB combined ID/revision/title/description/source bytes.
Duplicate template IDs are rejected, including different revisions of the same ID.
Existing compiler/fact budgets still apply. Validation retains structured diagnostics.
Entries expose a positive revision, exact-source SHA-256 identity and a separate
compiler validation identity. These hashes are not authorization.

`TemplateCopyService` authorizes current company owner/admin membership through
`LifecycleAuthorizer` before looking up a template or preparing source. Operator
catalogue access confers no company access. Requests select an exact template
revision and a fresh non-nil workflow UUID; stale or missing selections fail.
The compiler rebases only the located root `workflow_id` scalar, preserving source
outside it, then re-decodes and checks the resulting typed identity and size.
Comments, literal UUIDs and child pins remain unchanged.

A successful copy owns its company, new workflow ID, revision 1, descriptive metadata,
rebased source and source identity. Its origin is an informational snapshot of the
template ID, revision and original source identity. Replacing or withdrawing the
catalogue cannot alter a copy. A draft contains no live template reference, grants,
credentials or frozen fact bundle, and is not an `OwnedVersion` or executable content.
The template's compiled hash is never presented as the rebased draft's compiled hash.
Later validation/publication must resolve company facts and authorize resources and
freeze dependencies before execution.

`WorkflowDraftCopies::insert_copy` is a required application persistence port with
an explicit test implementation only. Phase 03 must implement atomic insertion of
source, metadata and origin, rechecking current owner/admin membership for the
included actor/company in the same transaction. Identity collisions return conflict
without overwriting; revocation returns non-disclosing not-found. No production
draft persistence or authoring endpoint is supplied by the template-copy layer.
General editing and publication library commands are described below.

## Frozen publication content

`application::workflow::publication::freeze` builds an immutable library bundle from
decoded source and captured company dependencies. It is not a publish command or an
authorization service. The lifecycle layer must load trusted company-owned snapshots
and approved policies; a caller-supplied company identifier is not proof of access.

The bundle retains source/compiler content, agent instructions and explicit model
settings, ordered saved tools/skills, skill instructions and required tools,
tool schemas and approved effect/recovery policy, and pinned immutable child bundles.
Compiler facts are derived from that same content. SHA-256 hashes cover each captured
dependency and the version manifest; child hashes transitively cover child content.
Editing a captured input copy or constructing another version cannot mutate a bundle.
Policies come from the trusted policy loader, never MCP server annotations. Tool names
plus MCP slot identify explicit calls; name-only capability selections reject ambiguous
tool names across slots.

An agent selector is a literal agent key or a binding whose compiled type is a
finite string enum of agent keys; publication captures every agent in that set and
refuses any other selector. `PublishedBundle::prepare_step_inputs` rejects a resolved
selection outside the captured catalogue. Runtime integration must use this bundle
hook rather than the compiler-only input hook. Each captured agent carries exactly
its saved tools and skills, in order, and every skill's required tools must be among
the agent's own tools. These snapshots do not grant execution access: the runtime
re-checks the agent, its model and the prepared tools and skills before each model
call.

Resource declarations retain slot, kind and optional contract. MCP snapshots must
refer to declared MCP slots. Company resource selection, provider compatibility and
readiness checks use the binding library below; connections and
credentials are not publication content. Admission snapshots retain the
exact binding revision and resource IDs; effects must resolve current secrets at use time.

Publication rejects missing/duplicate/foreign dependencies, incomplete tool/skill
selections, missing approved policy revisions, mismatched child pins, conflicting
content for a child version and recursive workflow dependencies. Bounds include 256
direct snapshots/children, 256 unique transitive child versions, 32 bundle levels,
128 tools/skills per selection, 16 KiB instruction strings, the shared 1 MiB fact
budget, and a 4 MiB manifest. The fact budget also bounds schemas before copying.
Library tests enforce these bounds at stock 2 MiB stack size. Lifecycle commands below
provide idempotent publication contracts; production storage remains subsequent work.

## Binding configuration and readiness

`application::workflow::binding::ConfiguredBinding` holds one immutable positive
revision, frozen publication bundle, schema- and size-validated parameters, and
exactly the declared resource slots. Selections contain typed runtime resource IDs,
never credentials. Constructing another revision cannot change existing values.

`check_readiness` queries a required `ResourceDirectory` for current identity,
company ownership, actor authorization, resource kind, supported provider contract
and readiness. Missing, foreign, unauthorized or revoked resources fail closed;
directory errors propagate. Resource facts contain at most 256 supported contracts.
Adapters must derive these facts from actual provider/configuration support and
bound their I/O. No production resource adapter is wired yet.

Readiness is an observation, not an activation commit or lasting authorization.
Lifecycle commands must authorize management and atomically select the expected
revision. Admission now selects by logical binding ID and retains its immutable revision,
validated parameters, resource IDs and complete published bundle. Callers supply input,
not parameter overrides. The service validates the input schema and the aggregate
input/parameter byte limit before handing the snapshot to the atomic admission port.
Exact replay uses the original selection even after edits, archive or deactivation,
while current actor/association authorization is checked again. New admissions require
an active, selectable head; the commit port must reject a stale selection without writes.
Production binding transactions and competing-revision database tests remain phase03 work.

ResourceDirectory returns only current non-secret facts. A readiness observation is not
an execution grant: effect adapters must check current resource access/revocation and
resolve current credentials at every use. Secret rotation does not rewrite a run's
resource identity or frozen contract. Actual provider use enforcement remains phase04 work.

## Authoring lifecycle commands (library)

`DefinitionService` saves bounded draft source (including invalid YAML), validates with
source-located compiler diagnostics, publishes an immutable source/bundle record, and archives.
Saves and archive require the expected draft revision; creation is insert-only. Publishing
requires a company-scoped command key and stable proposed version ID. Equivalent replay returns
the original bundle after current authorization, even after draft edits or archive; reusing a
key for another revision/version fails. A template copy retains its original provenance on edit.

`BindingService` configures immutable binding revisions and activates/deactivates with an
expected lifecycle revision. The lifecycle revision changes on configuration and activity writes,
preventing stale activation after an intervening edit or deactivate/reactivate cycle. Related
channel/thread association is fixed when created. Reconfiguration preserves active state only
after current readiness validation. Archive blocks new selection; deactivation remains available
after archive or resource revocation and never cancels an existing run.

These are authorized application commands and mandatory atomic port contracts. Production SQL,
transactional access/readiness rechecks, history persistence, and real database race enforcement
remain phase03 integration gates. No HTTP routes or production lifecycle storage are wired yet.

## Representative fixtures

`application::workflow::fixtures::ALL` exposes six checked-in YAML sources and their
illustrative captured facts. `Fixture::snapshots(company)` supplies synthetic agent/skill/tool
content for that test tenant; it is not an authorization or production dependency loader.
Use the normal decoder and `publication::freeze`, first freezing `Fixture::child()` when present.
The tests use those same paths, so later runtime/handler tests can reuse and extend these sources.

| Source under `src/application/workflow/fixtures/` | Demonstrated contract |
| --- | --- |
| `reviewed-support.yaml` | Draft → human review → reply using human-edited data; rejection ends without reply |
| `autonomous-response.yaml` | Agent → reply, no classifier; the captured agent's saved tools and skills include a tool and a skill |
| `triage-routing.yaml` | Finite classifier → ordered support/sales rule cases with required default |
| `repeated-human-revision.yaml` | Three-round bounded child pin → accepted reply or escalation |
| `human-revision-round.yaml` | Draft with prior body/feedback → human accept/revise with required revision feedback |
| `mcp-lookup.yaml` | Typed integer/boolean/array/object/null arguments and downstream structured result reference |

These are compiler/publication fixtures, not runnable demos. Deadlines, reviewers and message
IDs are inputs/parameters, not embedded operational identities. The repeated-review source still
needs phase07 next-round input mapping, exit/exhaustion scheduling, durable round identity and
accepted-artifact provenance enforcement. Its child output schema declares the review result;
the runtime's terminal result projection remains part of that handler work. There is no feedback
loop or implicit backward edge in the current language. Phase07 must extend these same fixtures
to prove last-round acceptance, default three-round exhaustion, and exact-artifact delivery.

The MCP envelope can omit `structuredContent`; its downstream binding explicitly defaults a
missing result to an empty typed result. Present `null` is invalid for that result contract and
does not take the missing-value fallback. The future MCP handler must distinguish tool errors
and effect outcomes before selecting success; these fixtures perform no network or delivery work.
Production SQL/transaction races and provider-use authorization remain phases03/04 obligations.


## Jev typed questions and images

`jev.evaluate` declares a `typesafe` resource slot and selects it with literal
`connection`. Use an exact `jev-MAJOR.MINOR.PATCH` model ID, such as `jev-1.14.0`;
moving aliases are rejected. `state` accepts a string, object, or array through any
normal binding. `questions` is a literal map with 1–32 distinct, nonblank IDs, each
at most 128 UTF-8 bytes and without control characters. Question IDs select
`/steps/<step>/output/answers/<question>`, independently of the workflow step ID.
Escape `~` and `/` as `~0` and `~1` when using them in JSON pointers.

Each question has literal string `instructions` and one of these shapes:

- `noul`: optional `criteria` with exactly string descriptions `true` and `false`.
  Its answer has exactly `type: noul` and `noul` in [0, 1].
- `choice`: `criteria` maps 1–64 nonblank option IDs to string descriptions.
  Its answer has `type: choice`, a declared `choice`, `probabilities` with exactly
  the option keys, and `confidence` in [0, 1].
- `score`: `criteria` is an ordered list of 2–10 string descriptions. Its answer
  has `type: score`, a numeric `score` from zero through the last level index,
  `probabilities` keyed by zero-based level numbers as strings, `legend` with
  the exact original descriptions, and `confidence` in [0, 1].

Instructions and descriptions must be nonblank and at most 16 KiB of UTF-8 each.
Unknown fields and duplicate keys are rejected. Box and image assets follow the experimental contract below.
The exact output envelope contains `answers`, the selected versioned `model`, and
nonnegative integer `usage.input_tokens` / `usage.output_tokens`. Missing, extra,
mismatched, nonfinite and out-of-range answers fail completion. Every probability
is in [0, 1]; each distribution must sum to one within absolute tolerance `1e-6`
to accommodate rounding. Noul remains a probability; confidence does not assert
correctness. Use downstream `decision.rule` for thresholds and routing.

The entire serialized provider request (model, state, questions) and response each
have a 64 KiB ceiling. Borrowed preflight counts bytes before descriptor copies;
resolved state is checked again before dispatch. Existing source (256 KiB), schema
(64 KiB / 4096 nodes), compiled-fact (1 MiB), and workflow context limits also apply,
so aggregate budgets can reject combinations below the individual maxima. This
step is one model invocation with committed-result reuse, without agent tools or
action effects. The production catalogue and worker support the complete text/image contract.
Create a company TypeSafe connection in company Settings, allow the exact model,
then select it for the `typesafe` slot on the workflow Bindings tab. The published
source pins the model and questions; the binding stores only the connection ID.
Rotation uses the current key on the next provider call. Disabled, revoked or
model-disallowed connections fail current-use authorization without fallback.

The mixed Noul/Choice/Score example is available through
`registry::example("jev.evaluate")`. Upstream contracts were checked against the
[System One API](https://docs.typesafe.ai/api/system-one),
[primitives](https://docs.typesafe.ai/primitives), and
[versioned models](https://docs.typesafe.ai/models).


### Experimental Jev images and Box

Images and Box are experimental TypeSafe features. This application supports images
on exact `jev-1.13.PATCH` and `jev-1.14.PATCH` models, and Box on `jev-1.14.PATCH` only.
Future model families require an explicit support update. Text-only requests never
send the image opt-in header.

Optional literal `with.assets` maps 1–4 logical identifiers (ASCII letter followed
by letters, digits or underscores, at most 64 bytes) to immutable private message
attachment references. Each reference has exactly `channel_id`, `thread_id` (nonnil
UUIDs), and `sha256` (64 lowercase hex characters). Public URLs, object keys,
base64 and unverifiable references are rejected. The run actor must currently
have company and channel/thread visibility; cross-company attachments are refused.
Missing, changed, unsupported or oversized images produce explicit `jev.asset_*`
failures. A retry verifies the same digest again before replay or a new provider call.

Local limits are 24 KiB per PNG/JPEG, dimensions 280–1024 each, at most 1,048,576
pixels, still images only. Bytes, MIME, decoded image and digest must agree.
The complete JSON request, including base64 framing, still fits within 64 KiB;
several individually valid images can therefore exceed the aggregate limit.
Image bytes exist only during an authorized provider request. Bundles, invocation
identity, traces and sample reports carry inert references and digests only.
Token admission conservatively adds one token per decoded pixel to the wire-byte
estimate; actual provider usage is retained on settlement.

For requests with assets, provider state is `{ "content": <original state>,
"assets": { "<logical name>": <server opaque placeholder> } }`. Refer to original
state through `content` and images through `assets.NAME`. No user text is replaced
by a short logical token. A `box` question has `type` and string `instructions`
only; instructions must name a declared image as a backtick path such as
`assets.photo`. It forbids `criteria`. Its answer has exactly `type: box`, `box`
with `left`, `right`, `top`, `bottom`, and `confidence`. Coordinates and confidence
are in [0,1], with `left < right` and `top < bottom`. Mixed text and Box answers
have static downstream paths, for example `/steps/start/output/answers/mug/box/left`.

The scripted image sample in `samples/jev-image.json` contains inert references and
supplied results. It never reads private storage or calls TypeSafe. Its format has
`source` (workflow definition) and `sample` (sample request); use them with the
ordinary compile/publication and sample APIs. The mixed scenario in
`samples/jev-triage.json` provides `sample` (automatic routing) and `uncertain_sample`
(a scripted human decision) against one source. Replace the illustrative reviewer
and attachment identifiers with authorized company resources before live use.
Jev sample usage comes from `output.usage.input_tokens` plus `output.usage.output_tokens`;
leave the generic script `tokens` at zero. Samples charge one simulated model call
and enforce their token allowance, without billing or external access.
Contracts checked against [TypeSafe Images](https://docs.typesafe.ai/images)
and [Box](https://docs.typesafe.ai/primitives/box).
