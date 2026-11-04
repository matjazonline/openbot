# Knowledge layer with access levels

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and status

Company members upload documents; agents answer from them; every piece of content carries an
access level from 0 to 9, and a reader sees only content at or below their own level. This is
proposed implementation work. Nothing described here exists yet.

```text
upload "0_sales.md"   level 0 (from the prefix)   overview, features, list prices
upload "5_sales.md"   level 5 (from the prefix)   discount floor, margins, competitor notes

customer (outside contact, level 0) asks "what is the lowest price?"
  -> search reads only level-0 chunks         -> agent: "I don't have that"
sales rep (member, level 5) asks the same
  -> search reads level-0 and level-5 chunks  -> agent quotes the discount floor
rep CCs the customer and asks again
  -> the reply reaches a level-0 reader       -> search reads level 0 only
```

## Decisions

### User decisions (2026-11-03)

| ID | Decision (user's words, lightly tidied) |
| --- | --- |
| D1 | Levels apply to both members and outside contacts. Outside contacts are always level 0, which is the public level. |
| D2 | The range is fixed: 0–9. |
| D3 | When the people receiving an answer have different levels, answer at the lowest level. |
| D4 | When content is withheld, the agent says "I don't have that". |
| D5 | Agent output that used restricted content is stamped. There is one level enum, used in the different parts of the app. |
| D6 | Revised 2026-11-04. The file name carries the level: it must start with a level digit 0–9 followed by `_` or `-` (confirmed 2026-11-04). `0_sales_discounts.md` is level 0, and `6_sales_discounts.md` and `6-sales_discounts.md` are both level 6. A name without that prefix is an error. Files whose names differ only in the prefix are layers of the same document, and uploading the same prefixed name again overrides it. One upload may contain several files. Each company member can upload documents only up to their own level. (This replaces the earlier rule of choosing a level per file at upload.) |

### Further decisions

Status: **Confirmed** means the user approved it (2026-11-03 or the date shown). **Proposed** means it still needs
confirmation before the group that uses it.

| ID | Status | Proposal | Practical effect |
| --- | --- | --- | --- |
| P1 | Confirmed: "owner or admin and admin not above his level" | The owner is always level 9. Invited members and admins start at 0. The owner and admins set levels; an admin can neither assign a level above their own nor change someone whose level is above theirs. | A level-3 admin cannot make anyone level 5, and cannot demote the level-7 head of sales. |
| P2 | Confirmed (2026-11-04) | Files whose names match after the level prefix form one logical document. A level-N reader sees every layer at or below N. Names compare case-insensitively after trimming. | `5_Sales.md` and `5-sales.md` both override `5_sales.md`, because the separator and case are not part of the document's identity. A level-5 reader's library shows "sales.md — levels 0, 5", listing `0_sales.md` and `5_sales.md`. |
| P3 | Confirmed (2026-11-04): multilingual | Embeddings are computed locally with `fastembed`, using `multilingual-e5-small` (384 dimensions). The model files are baked into the image and checked against a checksum; nothing is downloaded at runtime. | Works for every company whatever model provider it uses. The image grows by roughly 120–470 MB, depending on quantisation. Retrieved chunks still go to the agent's LLM provider. Content is multilingual, so the English-only `bge-small-en-v1.5` is ruled out. |
| P4 | Confirmed (2026-11-04) | Version 1 accepts `.txt` and `.md` (UTF-8). PDF and DOCX come later. | A sales PDF has to be exported to text or Markdown first. |
| P5 | Confirmed: "yes, if not set level 0" | HTTP connections and MCP connections carry an access level (default 0, set by admins up to their own level). A run that has read level-N content cannot send tool arguments to a destination below N. | The internal CRM connection is set to 9, so a level-5 run can still look up customers. A third-party SaaS stays at 0 and is refused once the run has read level-5 content. |
| P6 | Confirmed (2026-11-04): option A, withhold | If a reply would reach someone below the run's content level (for example, someone CC'd while the run was working), nothing from that run is sent. The run fails with `knowledge.audience_below_content` and shows up in attention without any content. Rejected alternatives: B, send only to the qualifying recipients (splits the thread, invites reply-all leaks, needs new partial-thread sending); C, regenerate at the lower level (duplicates the reply that the CC'd message's own run sends); D, let the sender approve anyway (makes levels advisory). | The rep sees "Reply withheld: a recipient on this thread is below the content level". Being level 5, the rep can read the withheld draft in the run view (05D). The customer is still answered, because the message that CC'd them starts its own run with ceiling 0. The customer never receives level-5 text. |
| P7 | Confirmed (2026-11-04): option A, placeholder | A teammate looking at a message above their level sees a "Restricted message" placeholder. A run's step data above their level is redacted. | A level-0 member can see that the thread exists and has a reply, but cannot read it. |
| P8 | Confirmed (2026-11-04) | Upload is synchronous and bounded (at most 10 files and 1 MiB of text each per request). The document and its chunks commit in one transaction; there is no indexing worker. | The upload page waits a few seconds for embedding, and a document is searchable as soon as the page returns. |
| P9 | Confirmed (2026-11-04): option B, both from the start | Search combines meaning (vectors) with exact-term matching (Postgres full-text, `simple` configuration, so it works in any language and keeps codes such as `AX-4410` intact). Both come from one query with one level filter, and the two rankings are merged. | "Price of AX-4410?" returns the AX-4410 passage, not the near-identical AX-4401 one. "How low can we go?" still finds "discount floor" by meaning. |

## Invariants

1. Content at level L never reaches a reader below L, whether through search, thread history,
   remembered agent exchanges, a reply, tool arguments, the UI, a notification or run views.
2. Level filtering happens in SQL before ranking or reading. The model never chooses a level;
   the search tool has no level parameter.
3. A missing or unresolved identity is level 0. A level is never widened because something is
   unknown. This mirrors `plan/hydradb/01-preserve-memory-scope-isolation.md`.
4. A run's content level only rises. A step's output (or a tool observation) and the content-level
   rise it caused commit in one transaction.
5. One `AccessLevel` type in Rust and one `access_level` SQL domain carry every level.

## Implementation groups

| Group | Deliverable | Prerequisites |
| --- | --- | --- |
| [01 — Access levels](01-access-levels-and-member-levels.md) | `AccessLevel` type and SQL domain, member and invite levels, reader-level resolution, level management | — |
| [02 — Library and indexing](02-knowledge-library-and-indexing.md) | Documents, upload/override/delete with level rules, chunking, embedding port and adapter | 01 |
| [03 — Run levels and egress](03-run-levels-history-and-egress.md) | Run knowledge ceiling and content level, history filtering, message stamps, the release check on every egress | 01 |
| [04 — Search tool](04-knowledge-search-tool.md) | Built-in `search_knowledge` agent tool bounded by the run ceiling | 02, 03, 05 |
| [05 — Read paths](05-read-paths-ui-and-notifications.md) | Viewer-level filtering in the mailbox, previews, attachments, notifications and run views | 03 |
| [06 — Verification](06-integration-and-verification.md) | End-to-end level journeys, races, and the final full-suite gate | all |

### Implementation order

Implement the groups in the order **01 → 03 → 02 → 05 → 04 → 06**.

- 01 and 03 are the foundation that the other plans build on.
- The search tool (04) is the only source of content above level 0. Human-written messages are
  level 0. So 04 ships last, once everything that stores or shows such content can filter it:
  stamps and egress (03), library filtering (02), and the UI and notifications (05).
- Agents never get a knowledge read without its release check.

### Order across plans

The numeric prefix of each plan directory is its implementation order:

1. **01-knowledge-access-levels** (this plan), in the group order above. It goes first because
   the release check (03D) must exist before the later plans add egress paths and rewire tool
   dispatch.
2. **[02-autonomous-agent](../02-autonomous-agent/01-capability-contracts-and-storage.md).** It
   rewires MCP dispatch, which must keep the P5 connection levels and the `search_knowledge`
   built-in.
3. **[03-channel-handles-send](../03-channel-handles-send/01-contracts-and-response-ownership.md).**
   Its automatic replies and human-input prompts call `may_release` from the start.
4. **[04-autonomous-agent-history](../04-autonomous-agent-history/01-contracts-and-conversation-identity.md).**
   It goes last because it integrates with both 02 (agent revisions as exchange provenance) and
   03 (publication and human continuation). Landing it earlier would leave those cases as
   unexecuted dependencies that neither of the other plans owns.

02 and 03 do not depend on each other and may be swapped.

### Related plans

All are proposed; none is implemented yet.

| Plan | Relationship |
| --- | --- |
| [04-autonomous-agent-history](../04-autonomous-agent-history/01-contracts-and-conversation-identity.md) | Requires 01 and 03. It stamps each remembered exchange with the run's content level and selects only exchanges at or below the ceiling. |
| [03-channel-handles-send](../03-channel-handles-send/01-contracts-and-response-ownership.md) | Adds automatic reply publication and human-input prompts. Both are egress paths and must use the release check from 03D. |
| [02-autonomous-agent](../02-autonomous-agent/01-capability-contracts-and-storage.md) | Binds MCP tools directly to company connections. Those connections carry the P5 level, and the `search_knowledge` built-in (04) must follow whichever tool-dispatch shape is current. |

## Out of scope

- Compartments: different teams at the same level seeing different documents.
- Per-contact levels for outside contacts (D1 fixes them at 0).
- pgvector or any approximate index, until scanning a company's chunks is a measured bottleneck.
- Re-indexing when the embedding model changes. Changing the model means resetting the database.
- Classifying human-written message text. Inbound human messages are level 0.
