# Plan04 p01 independent review

## 04RU-P01-D1-R1 — Root membership architecture: CHANGES REQUIRED

[04RU-P01-D1-R1/N1](agent-review-notes.md), retained independent B, reviews [D1/r1](logs/D1-r1.md), SHA256 **421d64ed6135db91db556d18c6e744e4de75f498fb80b251d444c525cd01c42f**, frozen source **4d0aeba871fb23224b32d167e1a2461e7128084a12f548bb3d631627537b3110**, HEAD/review base8fdb892b6e279ed0c09150e2336bbdc7462f877d; coordination1 and exact two exclusions pinned.

- **F1 medium:** replacement membership lacks the distinct incarnation needed by immutable direct-notification event pins across removal/rejoin.
- **F2 high:** generic department manager authorization would bypass owner-only company Admin/membership mutation.
- **F3 medium:** proposed role deletion before principal demotion loses authenticated retirement audit facts.
- **F4 medium:** teardown must reconcile child deletion order with retained history that permits deletion only after its tenant disappears.

[Evidence and required corrections](agent-review-findings.md). No runtime execution or implementation acceptance. Return one corrected decision revision before dependent edits; p02/final Plan04 gates remain outstanding. Prior Plan03 publication and explicit Doop deferral are unchanged.


## 04RU-P01-D1-R2 — Corrected root membership architecture: PASS (decision only)

[04RU-P01-D1-R2/N1](agent-review-notes.md), retained independent B, **PASS** for immutable [D1/r2](logs/D1-r2.md), SHA256 **50a70e404c18b6bdf7af95740bd1bfb8e2c19f94e106b94d2e5d079039c011f2**, unchanged frozen source **4d0aeba871fb23224b32d167e1a2461e7128084a12f548bb3d631627537b3110**, HEAD8fdb892b6e279ed0c09150e2336bbdc7462f877d; coordination1 and two exclusions unchanged.

F1–F4 resolved at decision level: distinct membership incarnation, owner-scoped equivalent mutations, principal-boundary retirement audit, and parent-absent ordered statement-trigger teardown with explicit FK deferrals. Author's rolled-back PostgreSQL18 probe supports the mechanism only. No new findings; no reviewer runtime. Root may release implementation after identity checks; actual schema/history/concurrency/authorization and final Plan04 gates remain outstanding.


## 04RU-P01-D1-R3 — Native peer-FK timing: CHANGES REQUIRED

[04RU-P01-D1-R3/N1](agent-review-notes.md), retained independent B, immutable [D1/r3](logs/D1-r3.md) SHAef1485f2349888ae207f92b127dab461375715a8246aca0627f6029f5d33f559; source686872f7713869716cf5e387417dd23d7ad643011dd869689ee3583860becf24, HEAD8fdb892b, coordination1/two exclusions pinned. **F1 medium:** conversation sessions cross the direct threads→companies cascade; the decision incorrectly places all thread deletion after machine/session cleanup. Revise that edge's disposition and both-origin company/account proof, including explicit workflow-call peer analysis. Three proposed deferrals/tombstone preservation otherwise retain commit-time integrity. No reviewer runtime or implementation acceptance; no release of dependent native schema edits.


## 04RU-P01-D1-R4 — Native/Builder corrections: CHANGES REQUIRED for remaining teardown failure

[04RU-P01-D1-R4/N1](agent-review-notes.md), immutable D1/r4 SHA229dd3398b6c5a73550ac1d1bb7a2da7b5d809e75e6762ffa9ea39e29274bb97; unchanged source686872f7…cf24/HEAD8fdb892b/coordination1. R3/F1 resolved at design level; bounded non-root implicit Builder inference supported. **New R4/F1 medium:** actual owner-account deletion fails message_participants_identity_fk; trace and correct its teardown ordering while preserving standalone identity/history protection. No overall decision PASS, runtime execution by B or B1 acceptance. [Finding](agent-review-findings.md).


## 04RU-P01-D1-R5 — Identity-FK correction: PASS (decision only)

[04RU-P01-D1-R5/N1](agent-review-notes.md), retained B, **PASS** immutable [D1/r5](logs/D1-r5.md), SHA256 **d8be7fd9748e0db617ea9f8f23f86daf5a68cf42e22811a5e952e7068c0bc93d**, unchanged source686872f7713869716cf5e387417dd23d7ad643011dd869689ee3583860becf24/HEAD8fdb892b/coordination1. R4/F1 resolved at decision level by exact deferred NO ACTION identity edge with unchanged owners/history protections and real positive/negative regression mapping. Native-origin/tombstone and Builder decisions retained. No new findings or reviewer runtime. Root may release corrections after pin checks; production proof and full B1 implementation review remain outstanding.


## 04RU-P01-D1-R6 — Local-call dispatch FK correction: PASS (decision only)

[04RU-P01-D1-R6/N1](agent-review-notes.md), retained B, **PASS** immutable [D1/r6](logs/D1-r6.md), SHA256 **49bbf4167eb083a476c2d556c205e17b7710452cb02ee50d2e36ffc69f6cde9b**, frozen source **4ac1d9d0fcc475a8acff950a3d2a6f8c6558fcbf6c66b18cdeee45937f3cec4e**, HEAD8fdb892b/coordination1. Actual workflow teardown failure justifies precisely the seven-column local-call→dispatch deferred NO ACTION edge; other owners/peers/guards remain unchanged. Proposed probes distinguish wrong-run scope from foreign-tenant owner-guard rejection. Followed-role fixture correction matches existing fail-closed production behavior. No new findings or reviewer runtime. Root may release this correction after pin checks; real fresh-schema proofs, full B1 review and final Plan04 verification remain outstanding.


## 04RU-P01-D1-R7 — Remaining local-call native peers: PASS (decision only)

[04RU-P01-D1-R7/N1](agent-review-notes.md), retained independent B, **PASS** immutable [D1/r7](logs/D1-r7.md), SHA256 **8ec33f0a479882b3b3c67dc8ed8dde80be34228a68a27548246bf611254bb1a9**, frozen source **d31771d65f656072835280d4c7612bc1cf21df692abc92207719a58af5f0b947**, HEAD8fdb892b/coordination1. The observed profile failure and independent ownership graph support exactly four deferred native peer FKs, preserving owners, committed scope/origin integrity and guards. No new findings. Preserve the inherited origin-FK regression at its new enforcement boundary. Fresh-schema both-origin company/account proofs, full B1 review and final Plan04 verification remain required; no reviewer runtime or implementation acceptance.


## 04RU-P01-B1-R1 — Whole-B1 implementation/proof: CHANGES REQUIRED

[04RU-P01-B1-R1/N1](agent-review-notes.md), retained independent B, reviews frozen artifact **e7f60808ad0ddd95d12c8e85cc5accb306030037e80dfd8b754425b793654247**, HEAD/start/review base **8fdb892b6e279ed0c09150e2336bbdc7462f877d**, accepted D1/r7 SHA8ec33f0a…1a9, coordination1 and exact two exclusions pinned. **F1 medium:** unchanged browser escalation rendering/parsing omits valid Owner/Admin values, so editing a stored policy can clear it; explicit submission is rejected. Complete the actual saved-form round trip and accepted root escalation proof without weakening availability. [Finding and decisive checks](agent-review-findings.md).

Whole production/schema/test-helper and fixture delta reviewed. Completed retained evidence validates; fresh-schema focused44PASS, affected652PASS/0FAIL/2ignored and requiredHTTP21PASS do not cover the omitted new-role escalation consumer. No reviewer runtime or source mutation; architecture remains accepted. One corrective batch and re-review required before implementation acceptance; p02/p03/final repository gates, external journeys and explicit Plan03 Doop deferral remain outstanding.


## 04RU-P01-B1-R2 — Escalation correction recheck: PASS

[04RU-P01-B1-R2/N1](agent-review-notes.md), retained B, **PASS** for frozen artifact **2a5714f52927de4b4b7d261ba970d95d2ed526d768f80ced4009c8b2dd7ab793**, HEAD/start/review base8fdb892b6e279ed0c09150e2336bbdc7462f877d, coordination1 and two exclusions unchanged. **B1-R1/F1 resolved:** canonical browser rendering/parsing preserves every stored role/hop value; actual create→GET/render→POST/edit→read-back, invalid no-write, Rust/SQL and real Owner/Admin worker selection proofs pass. Retained whole-B1 R1 coverage applies to unchanged paths.

Completed evidence validates. Four decisive F1 checks and all21HTTP checks PASS; broader176 run174PASS/1old-assertionFAIL/1manual-browserIGNORED is retained, with exact corrected retirement test subsequently1PASS. The correction preserves lifetime identity/history and proves no live authority. Current all-target SQLx/reachability checks pass; no removed tests or new findings. No reviewer runtime or source edits. Root may accept B1 after matching pins; p02/p03/integration/final full-suite gates and explicitly unexercised/deferred external journeys remain outstanding.
