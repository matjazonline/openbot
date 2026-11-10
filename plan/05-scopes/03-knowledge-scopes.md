# 03 Knowledge scopes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every knowledge document belongs to a department; a run searches the documents of the units in its
run scopes at or below its level; people upload into departments they belong to or lead one step
above. Continues in [04](04-integration-and-final-verification.md).

## Batch 03A Schema and search

1. `knowledge_documents` (`init_schema.sql:12019`) gains `unit_id uuid NOT NULL` with a composite
   foreign key to `departments (company_id, id) ON DELETE RESTRICT`, and `knowledge_chunks`
   (`:11983`) carries it denormalized the way it carries `access_level`, so the search predicate
   needs no join. Uniqueness becomes `(company_id, unit_id, name_key, access_level)` and
   `(company_id, id, access_level)` stays: the level layers of one document live in one unit.
2. The search statement (`visible`, `src/adapters/persistence/knowledge/search.rs:46`) adds
   `AND chunk.unit_id = ANY($units)` beside the level filter, where `$units` is the run's `read`
   rows from `workflow_run_scopes` ([02](02-scope-resolution-and-tools.md)). The filter is in SQL
   before ranking, as the level is. A run with no scope rows searches nothing and the tool says so
   (`knowledge.scope_unavailable`), never the whole company.
3. Manual search from the library page uses the viewer's own position: the units where they are a
   member or hold a role, their ancestors, and one step down for a lead, computed by the same
   `run_scopes` function with the person as principal. Owner and admins see every unit.
4. `workflow_knowledge_consumptions` (`:12004`) records the document's unit with its level, so a
   proof says which department's knowledge a reply used. Egress and consumption rules are unchanged.

## Batch 03B Upload, move and library

1. Upload (`src/adapters/http/routes/knowledge/upload.rs`, `replace`,
   `src/adapters/persistence/knowledge/writes.rs:38`) takes a target unit. Authority: the uploader
   is a member or role holder of that unit, holds `lead` in its parent, or is an owner or admin, and
   the file's level prefix is at most their level, as today. The form defaults to the uploader's
   single department and asks when there are several; a person in no department uploads to the
   root only if they are an owner or admin.
2. A document is moved, not copied. "Move to" lists the units where the mover has upload authority
   and moves every level layer together, in one transaction. Moving up to the common ancestor is
   how two sibling departments come to share a document; there is no sideways grant.
3. Deleting a department that owns documents is refused by the foreign key, like desks; the
   Departments pane says what to move first. Re-parenting moves nothing and changes only readers.
4. The library (`knowledge_library`, `src/adapters/http/pages/knowledge.rs:7`) groups documents by
   department, root first, and the Departments pane lists each department's documents with a
   link into the library filtered to that unit.
5. The agent system context for a run on a department desk names the units it can search, in the
   same sentence that names its memory unit ([02](02-scope-resolution-and-tools.md), Batch 02B
   item 4).

## Focused checks

- `knowledge_scope_schema_`: the unit foreign key, denormalized chunk unit, layer uniqueness per
  unit, and department delete refusal.
- `knowledge_scope_search_`: with the scripted-model `Deployment` harness, the same question on two
  department desks finds two different documents; a sub-department run finds root documents; a
  root run does not find a sub-department's; a lead finds one step down; the level filter still
  applies inside a readable unit; no scope rows search nothing.
- `knowledge_scope_upload_`: authority for member, role holder, parent lead, admin, outsider and
  over-level; move takes all layers and refuses a target without authority; the library groups and
  filters correctly; consumption proofs record the unit.
