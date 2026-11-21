# 06 UI, samples and documentation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A person exports, previews and applies a blueprint from the department page, starts from a
shipped sample, and reads documentation that says what a document carries and what it does not.
Continues in [07](07-integration-and-final-verification.md).

## Batch 06A Department page

1. The department page (`src/adapters/http/pages/company_departments.rs`) gains a Blueprint
   section, shown to people who manage the unit. A member who does not manage it sees no section,
   as they see no forms today.
2. **Export** shows the unit's document as YAML with copy and download, and the `omitted` list
   above it in plain words: what a setup recreated from this file will lack.
3. **Import** takes pasted or uploaded YAML or JSON, bounded at the document bound before it is
   parsed, and shows the preview of [02](02-plan-and-diff.md): operations grouped by unit, long
   texts as line diffs, every tool grant, budget and access change called out, the operations
   this person could not perform, and every refusal with its path in the document.
4. **Apply** posts the document, the previewed digest and a fresh key to the route of
   [03](03-apply.md). A stale answer re-renders the new preview and says what changed. Apply is
   offered only when the preview has no refusal.
5. **History** lists the unit's last ten applications: who applied, when, whether a person or
   which agent proposed it, and the operation counts, beside the ten administration events 06
   already shows.
6. An imported document is shown as text and data only. Nothing in it is rendered as markup,
   fetched or expanded, and aliases are refused at parse
   ([01](01-document-and-export.md), batch 01B item 4).
7. The page states the two things people get wrong: an export is the setup and not a backup of
   conversations, memory or knowledge; and applying a document from someone else gives its
   agents the prompts and tool grants written in it.

## Batch 06B Samples

1. Shipped sample documents live with the type in `src/application/blueprint/fixtures/`, as the
   workflow samples live in `src/application/workflow/fixtures/`. The first is the goals office
   of 08: a standing
   unit holding the Builder and a planning agent, with one example goal unit under it whose own
   desk binds `goal-plan-approve-dispatch`.
2. A sample names no person. Its `lead` and `sponsors` seats are unstated, and the page tells the
   importer which seats to fill, because a builder's approver and a goal's decider must be people
   (05, batch 05C item 2; 08 proposal 1).
3. Import offers the samples as starting points. They are documents like any other and go through
   the same preview and apply.
4. Every sample plans without a refusal against a freshly created company that has one ready
   model. A test holds that, so a format change cannot strand a sample.

## Batch 06C Documentation

1. A new `docs/blueprints.md`: the format, field by field; what is carried, referenced and left
   out; the meaning of stated and unstated fields; export, preview and apply; the Builder, its
   reach and its approvers; sharing a document and what to check before applying one.
2. Its field reference is checked against the type: a test compares the field names the document
   lists with the schema the parser uses, both ways. Documentation then cannot name a field the
   parser refuses or miss one it accepts, which the root `AGENTS.md` requires of anything
   documented.
3. `docs/departments.md` gains the Blueprint section and links it from "Archive" and "Delete".
   `docs/agent-authoring.md` says that a blueprint applies agent changes as a saved and published
   revision.
4. `docs/agent-builder-context.md` is rewritten for the Builder: its four tools, that it proposes
   and never writes, who approves, and that a design is live only after a success receipt.
5. Write these after the behavior exists. Do not document a route, a refusal code or a bound
   before the code enforces it.

## Focused checks

- Rendering tests: the section is absent for a non-manager; the preview renders operations,
  refusals and the authority marks; a stale apply re-renders; history shows person and agent
  origins; hostile document text is escaped.
- `blueprint_samples_`: each sample parses, plans without refusal on a fresh company, applies,
  and exports equal to itself apart from the seats a person filled.
- `blueprint_docs_fields_`: the consistency test of batch 06C item 2.
- `python3 scripts/check-integration-docs.py` and a link check of the new and changed documents.
- The browser checks the repository runs for department pages, extended to export, import and
  apply. Mark them unavailable, not passed, where the browser prerequisite is missing.
