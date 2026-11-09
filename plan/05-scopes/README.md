# 05 Scopes: memory and knowledge follow the organization tree

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09; knowledge folded in the same day. Fifth of the organization plans;
depends on
[01-departments](../01-departments/README.md) through [04-root-unit](../04-root-unit/README.md)
and on the memory service integration (`plan/memory-service`, batch 05A), which is implemented on
the memory-service branch of the working checkout and not yet on `main`. File anchors below that
name memory modules refer to that branch.

## Intended behavior

Memory and knowledge answer two separate questions about a fact: who may know it, which is the
access level and does not change here, and whose it is, which today is "the company" for
knowledge and one of three ad hoc kinds for memory. After the organization plans both answer the
second question the same way: a fact belongs to a unit in the tree or to a principal, and a run
reads what its position in the tree allows. One resolver computes that read set at admission and
both consumers use it.

Today the memory client knows three scope kinds, `company`, `agent` and `user`
(`NamespaceOwner`, `src/application/memory/settings.rs:164`; `memory_namespaces.scope_kind`), each
provisioned lazily when a run first wants it. After the organization plans there is one tree of
departments with the company as its root, and principals attached to it. Memory follows that tree:

- a **unit namespace** per department, the root included. It holds what the department knows:
  customers, procedures learned, decisions taken on its desks. The root unit's namespace is what
  the company namespace was.
- a **principal namespace** per agent and per person. It holds what one actor knows for itself.

Knowledge documents belong to a unit as well, the root by default, which is what the single
company library means today. There is no principal-owned knowledge: a document written for one
agent is a department document or one of that agent's skills.

A run reads its agent's own memory namespace, and the memory and knowledge of the unit chain from
its desk's department up to the root; it writes memory to its desk's unit by default. Knowledge
flows down the tree, never sideways, and a lead change moves nothing, because roles own neither.

## Decisions (user, 2026-11-09)

1. **Two namespace kinds, derived, not configured.** `unit` and `principal` replace `company`,
   `agent` and `user`. A namespace is provisioned in the transaction that creates its department or
   principal and retired in the one that deletes it, through the lifecycle machinery that exists
   (`retire_namespaces_on`, called from `src/adapters/persistence/agent.rs:472` and
   `src/adapters/persistence/user_lifecycle.rs:88`). No operator grant list names namespaces.
2. **Desks and roles own no memory.** A desk's memory is its thread history. A role is a seat; if
   it owned memory, a lead change would carry the department's knowledge away with the outgoing
   agent. Desk identity stays provenance on a record, never a scope.
3. **Memory about a person is held by whoever may know it.** A customer is an external principal
   but gets no namespace. A record about them lives in the unit namespace of the desk that learned
   it, with the customer as `subject` in provenance, so purging everything about one person is one
   query across units.
4. **Reads walk up, one step down for a parent lead.** Read set = own principal namespace, plus the
   desk's unit and every ancestor to the root. A holder of a department's `lead` role also reads
   its direct children's units, the same one-step rule as desk grants
   ([01-departments](../01-departments/README.md), decision 7). Siblings never share.
5. **Writes stay local.** Default write target is the desk's unit at the consumed level the memory
   plan already derives. A save marked `personal` goes to the agent's own namespace. A run never
   writes an ancestor; root desks write the root. An explicit "share up" save is deferred and
   recorded as an open question in [02](02-scope-resolution-and-tools.md).
6. **Direct messages resolve like role bindings.** A run with no desk
   ([03-principal-addresses](../03-principal-addresses/README.md)) uses the agent's single
   department if it has exactly one; otherwise it reads its own namespace and the root only, and
   writes only its own namespace.
7. **Levels stay orthogonal.** Access levels remain a classification inside a namespace, the read
   ceiling still comes from root membership and the run's authorized audience, and the consumed
   watermark rules in `plan/memory-service/05-openbot-integration.md` are unchanged. Department
   membership never changes a level.
8. **The service does not learn about departments.** It still sees opaque tenant and namespace ids,
   a read set and a ceiling. OpenBot computes the read set from the tree.
9. **Knowledge uses the same read set, with no principal axis.** A document belongs to a unit;
   search filters on the run's unit set beside the level. Upload authority follows the one-step
   rule:
   a member or role holder of the unit, the lead of its parent, or an owner or admin, each up to
   their own level. Sharing sideways is moving the document to the common ancestor.
10. **One resolver, one snapshot.** `run_scopes` is computed once at admission and frozen into
    `workflow_run_scopes`; memory tools and knowledge search both read it. Neither recomputes
    access from the live tree mid-run.

## What does not change

- The service contract, its access-level contract, credentials and the lifecycle reconciler.
- `memory.load` and `memory.save` as workflow steps and the agent memory tools, except for the
  scope vocabulary in [02](02-scope-resolution-and-tools.md).
- Knowledge levels, the lowest-reader rule, withheld replies, consumption proofs, embeddings and the
  level-prefixed upload names (`plan/.archived/01-knowledge-access-levels/README.md`).
- The rule that a missing or foreign identity refuses a scope and never widens it
  (`memory_scope_lazy_provisioning_maps_scopes_and_refuses_missing_identities`,
  `src/adapters/persistence/memory/tests.rs:336`).

## Documents

1. [Unit and principal namespaces](01-unit-and-principal-namespaces.md).
2. [Scope resolution and tools](02-scope-resolution-and-tools.md).
3. [Knowledge scopes](03-knowledge-scopes.md).
4. [Integration and final verification](04-integration-and-final-verification.md).
