# Plan05 p03 independent findings

## D3 revision 1

2026-11-17; `05SC-P03-D3-R1`, note
[05SC-P03-D3-R1-N1](agent-review-notes.md#05sc-p03-d3-r1-n1).
Independent B `/root/p05_reviewer`; requested Astra/high; workflow
implement-astra-reviewed-2-3; phase p03 in
`/Users/mac02/orca/workspaces/openbot/plan03-04`, feature/plan05-scopes.
Start/review-base/HEAD `5fd8865225125771205f4e0a7a8a59f595e19905`, unchanged source
digest `5e52c2b5725bf4e40e736e6b40de0f15be7157d3926d154562783f4a29e02ac2`.
[Immutable decisions](logs/D3-R1-TASK.md), [REVIEW](REVIEW.md), coordination13
C001–C004/r1/C005/r2 and proposed C006/r1.

No actionable defects or optional suggestions identified at this architecture gate.
Implementation and the explicit proof obligations remain pending. This entry is
not a no-defects claim about source that has not yet been implemented.

## B1 source round 1

Round 05SC-P03-B1-R1, 2026-11-18. All entries below belong to independent B /root/p05_reviewer_62, assigned Astra 6.2/high, implement-astra-reviewed-2-3. Phase p03, feature/plan05-scopes, /Users/mac02/orca/workspaces/openbot/plan03-04. Start/base/HEAD 5fd8865225125771205f4e0a7a8a59f595e19905, digest bd0bc89e1a9374f2c259145d6f9859a8af92f93e4bf49070da17d794f7b2828a. [Review note](agent-review-notes.md#05sc-p03-b1-r1-n1) · [Pinned criteria](logs/B1-R1-TASK.md) · [Source review](REVIEW.md)

### 05SC-P03-B1-R1-F1 — Hidden destination filename disclosed in move response

**High, actionable, classification/privacy. Open.** A member who can write in two departments may move a known public document between them. When the destination has only an above-ceiling layer with the same filename, the target lookup considers all levels and the response expressly confirms that a document with this name already exists there. The native form renders this text, and the API returns it through the common mutation error handler. The library and manual search hide that filename at the same clearance. D3/D calls for generic refusal without higher-layer metadata. Keep the atomic rejection of the collision. Return the explanatory conflict only when a colliding layer is visible, and give hidden-only collisions the existing generic whole-document refusal. Exercise both a hidden-only and a visible collision through the HTTP surface with a lower-clearance mover, including unchanged rows/revisions. Existing tests exercise an owner-visible collision and a hidden *source* layer in separate cases. Source inspection, no active probing. Relevant spans are src/adapters/persistence/knowledge/moves.rs:34–57, src/adapters/http/routes/knowledge/mod.rs:172–181 and src/adapters/http/routes/knowledge/scope_tests.rs:350–370.

### 05SC-P03-B1-R1-F2 — Filtered manual search embeds before authorizing the requested unit

**Medium, actionable, authorization ordering and verification. Open.** The application calls company-only access and immediately enters the shared query embedder. The first authorization of an explicit unit occurs inside the later candidate read. A valid company member's foreign or unreadable unit therefore consumes model capacity and may return a model error instead of the scoped refusal when the model is unavailable. D3/B promises that valid foreign unit IDs refuse without embedding. The existing foreign-ID HTTP case asserts the query counter after the foreign *company*, then checks only the status for the valid company plus foreign *unit*. Admit the requested read scope before embedding and preserve fresh SQL-time reauthorization against concurrent changes. Assert zero additional query calls for foreign and unreadable filtered units, alongside the existing mid-embedding revocation case. Source inspection. Relevant spans are src/application/knowledge/service.rs:98–115, src/adapters/persistence/knowledge/search.rs:116–125 and src/adapters/http/routes/knowledge/scope_tests.rs:223–247.

### 05SC-P03-B1-R1-F3 — Move form cannot reach writable destinations past the first 256

**Medium, actionable, UI correctness. Open.** For an owner/admin with 257 or more departments, the picker fetches only the first 256 plus the selected/default unit. On the move page, the selected filter also restricts the source anchor lookup. Selecting the omitted destination by opening its department library removes the source document from this lookup. The truncation notice contains no navigation to another page of destinations, so the native move cannot target an authorized department beyond the bound. Direct API submission still works. Original 03B/2 requires the mover to be able to choose an upload-authorized unit. Preserve bounded rendering and add a user-reachable way to select the omitted target while retaining the source. Verify a native route flow reaches and moves to the 257th destination and retains the bound. The 4096-document HTML test creates exactly 256 choices. Relevant spans are src/adapters/persistence/knowledge/permissions.rs:268–281, src/adapters/http/routes/knowledge/mod.rs:282–305, src/adapters/http/pages/knowledge.rs:144–162 and 183–191, and src/adapters/http/pages/knowledge_tests.rs:92–125. This is an implementation gap in the gated bounded-picker decision.

### 05SC-P03-B1-R1-F4 — A knowledge read-set limit suppresses the whole department detail

**Medium, actionable, existing caller regression. Open.** An ordinary member leading a department with 257 direct children may view that department's detail, roster and desks under the existing department policy. The newly mounted card unconditionally propagates the whole-library projection's scope-limit error, discarding that already-authorized department detail. Both the standalone Department view and the Companies department pane use this function. D3 intentionally bounds ordinary knowledge projections, while 03B/4 adds a card to the existing department view. Keep the knowledge refusal and preserve the independently authorized surrounding view, with a clear card-level unavailable state or a correctly scoped bounded department lookup. Exercise a member whose knowledge projection exceeds the bound and assert that department details still render and the refused card cannot expose documents. Relevant spans are src/application/use_cases/department.rs:309–340 and 409–433, src/adapters/http/routes/ui_departments.rs:117–151, src/adapters/persistence/knowledge/permissions.rs:130–138. The current card test covers classification in a small hierarchy. Source inspection.

The review found these gaps in implementation and fixture coverage. D3 permits bounded rendering and fail-closed projections. The findings require no increase of either bound. Candidate lesson for future bounded UI reviews is to exercise the first item beyond a page limit and its independently authorized surrounding view.

### B1-R2 resolution history for F1–F4 (2026-11-18)

Independent B /root/p05_reviewer_62, assigned Astra 6.2/high. Frozen artifact 2a1e2771ddb751adc80b3a6b20a8bbd404a9b71fa7e1bd69604e212b288b19d8, base/HEAD 5fd8865225125771205f4e0a7a8a59f595e19905, same worktree and exclusions. [05SC-P03-B1-R2-N1](agent-review-notes.md#05sc-p03-b1-r2-n1) · [Focused execution](logs/B1-R2-focused-r1.txt)

- **F1 fixed.** The company-serialized move checks for a destination layer visible at the current ceiling. Hidden-only collisions receive the same generic refusal as a hidden source layer. The API/native test first establishes that the target list is empty for the mover, then checks generic 403 for hidden-only and descriptive 409 after adding a visible layer. Owner-visible state matches the pre-request snapshot after each refusal.
- **F2 fixed.** Both manual endpoints call library search, which calls required per-unit preflight before acquiring the embedder. Both preflight and the subsequent SQL candidate read use the current actor/unit projection. Empty and occupied slot tests exercise valid foreign and unreadable targets with the expected 404 and model-call counts. A separate successful read followed by actual grant revocation during a second embedding returns no passages.
- **F3 fixed.** Production next links carry the source anchor and a validated writable cursor. SQL keyset selection follows the same root/name/UUID ordering as the page, limiting each query to 256 plus one for overflow detection. Selecting an omitted destination returns to the source form and explicitly includes the writable choice. The actual router test follows generated links to department 257, checks the bounded picker, submits the native POST and verifies the original layer at revision 2 in the target. Genuine foreign source/filter-result selection and cursor cases return 404. The final-source boxed helper retains those assertions.
- **F4 fixed.** The knowledge adapter translates the shared resolver limit into its own stable error. The department section handles just that error in the knowledge card. All other read errors keep their normal handling. The actual department route test exercises both the resolver's 255-child plus two-ancestor count and the SQL 257-child limit. Both responses retain the department/roster/desks, show the refusal, omit the document canary and leave the owner's stored layer intact.

These are source-finding dispositions. Phase acceptance awaits the complete retained final evidence.

R2 final evidence confirmation, 2026-11-18. All four dispositions remain fixed at staged digest 62d667d9e56395de26652accbbab2ee1eb9f854969e308607cae2aa003cb5907. B independently reconstructed the original reviewed source identity. All four targeted regression tests passed in the corrected-source full stock-stack run. [05SC-P03-B1-R2-N2](agent-review-notes.md#05sc-p03-b1-r2-n2) records the full-run and upstream-download failure dispositions.
