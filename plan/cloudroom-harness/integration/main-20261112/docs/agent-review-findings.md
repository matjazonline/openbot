

## CH-MAIN-B1-F1 — create form promises no hosted model but creation pins one

2026-11-12; implement-astra-reviewed-2-2; actionable product-contract defect, **P2 / medium; OPEN**. Combined integration round CH-MAIN-B1-R1 / note CH-MAIN-B1-N1. Independent B `/root/cloudroom_b3_reviewer`, requested Astra/high (runtime settings not independently exposed). Worktree `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`, HEAD23eb638d609614cafe958bd6a5d17a4e9bef6463, merge input103004a8ac2fd643cf14bde7e09022ae79f72053, artifact **84f0e56a7169710052dfb5ea0470d8c3f8dae644c9826f842b718d5145a58220**; exclusions integration docs and cloudroom coordination only.

`src/adapters/http/pages/agent_settings.rs:886` labels both blank choices “No hosted model”; line892 says to leave both blank for native profiles. The Advanced create form passes `agent_id: None` into the same agent_fields/company_model_selection path (lines568–574). However, actual CompanyAgents creation calls resolve_model_on before insertion/publication/desk provisioning (`src/adapters/persistence/company_agent.rs:83`): blank provider/model must resolve to the company's usable hosted default (lines145–159), or creation is refused. It then creates the active hosted answering desk. Thus the create UI offers an execution choice that this creation operation does not implement.

Required behavior derives from both accepted inputs: departments require atomic hosted default-desk creation, while cloudroom allows existing definitions to remove hosted selection for native publication. The merge must describe the actual operation at each surface. Make create guidance/choices accurate for the retained hosted-default prerequisite and preserve the supported native-only edit/publication behavior. Do not infer that native-only automatic desk creation is required. Verify create-with-default, create-without-usable-default and existing-agent blank edit wording/behavior through appropriate affected checks. [Exact source excerpts](logs/CH-MAIN-B1-R1-reviewer-findings-evidence.txt).

Observable implication: following the create form's native-only instructions either produces an explicitly hosted agent/default desk or fails for a missing hosted connection. This is an integration mismatch between individually reviewed UI wording and new creation behavior, not a request to replace either accepted execution contract. Existing producer/consumer guidance applies; no instruction change proposed.

## CH-MAIN-B1-F2 — documented company-agent update omits a required wire field

2026-11-12; implement-astra-reviewed-2-2; actionable documentation/API correctness defect, **P2 / medium; OPEN**. Same round/note/reviewer/worktree/input/digest as F1.

The complete update example in `docs/agent-authoring.md:106` omits `agent.native_permissions`. The actual `CompanyAgentJsonUpdate` embeds `AgentJsonPayload`, whose nonoptional `native_permissions` at `src/adapters/http/routes/agent.rs:192` has no serde default. The documented request therefore fails JSON deserialization before any revision/tool update. This is not an obsolete compatibility request: the accepted no-compatibility contract may require the field, but shipped examples must satisfy that contract.

Update the example with a supported explicit value and accurately describe the field's native-execution role; retain the existing strict wire contract. Check the resulting documented payload against the actual decoder rather than merely asserting that the example contains the field. [Exact excerpt/decoder evidence](logs/CH-MAIN-B1-R1-reviewer-findings-evidence.txt). No production request was sent by B; this conclusion follows directly from the required nonoptional typed field and the example's JSON.

The omission is already present in the accepted cloudroom input's guide and was missed in N1–N5; it is exposed here while checking combined API/documentation surfaces. That earlier pass does not excuse the mismatch. Per integration record ownership, prior phase history is not edited; this entry explicitly links the missed inherited contract to CH-P01-B3-N5. Existing documentation/consumer verification guidance suffices; no new instruction is supported.

All concrete findings for this round are F1/F2. No optional suggestions. Combined full verification remains outstanding separately; earlier phase failures are not presumed unrelated to this combined artifact without appropriate evidence.

## CH-MAIN-B1-R3 final correction dispositions — 2026-11-12

Independent B `/root/cloudroom_b3_reviewer`, requested Astra/high (actual runtime settings not independently exposed), implement-astra-reviewed-2-2; note CH-MAIN-B1-N3, final review CH-MAIN-B1-R3-final. Same worktree/branch/input commits and exact exclusions as N1; reviewed entry/return artifact **a0aa542552ba808a17447d5c219eb6777ec00c9c0801550da8dc8329219b2554**. Cloudroom authorityrev4 and departments authorityrev19/notices unchanged.

- **CH-MAIN-B1-F1 — FIXED.** The shared model picker derives creation/editing from existing AgentFields.agent_id. Creation says Company default and explains that the new answering desk requires a keyed model connection; editing retains the native-only blank-model option. No new creation path, hosted-execution exception or weakened desk prerequisite was introduced. Actual rendered create/edit tests pass, as do model-less create refusal, atomic default-desk creation/competing creators and native-only definition checks in the27-test correction run and complete merged suites.
- **CH-MAIN-B1-F2 — FIXED.** The guide's complete JSON update now includes native_permissionsfull and explains its required presence/supported native modes. The new regression extracts that exact guide example and deserializes the fully qualified production CompanyAgentJsonUpdate, asserting the resulting native policy. It passed in the focused run and both full modes. R2's compilation failure remains retained; R2 received no acceptance.

No new actionable or optional findings. All required affected checks are exercised successfully; the complete repository command remains nonzero for separately confirmed unchanged issues:

- CSP scanner: exact sample.rs153 hostile literal failure matches accepted feature normal/stock evidence; the triggering whole sample file is byte-identical to both immutable inputs.
- MCP dropped-call fixture: exact3of4versusOk4 failure matches accepted feature normal/stock evidence; the entire test file is identical to both inputs.
- Raw patch whitespace: all331reported files are retained docs/logs evidence, each independently checked byte-for-byte against its immutable input. No historical log was rewritten. The complete target/product diff excluding only retained plan log paths passes whitespace checks.

These are confirmed unrelated failures, not waived affected checks. Transport boundary now passes in the merged artifact; the input feature's prior transport failure is not carried forward as a current failure. Normal and stock2MiB each4216PASS/2FAIL/26ignored; no overflow. [Independent final evidence/identities](logs/CH-MAIN-B1-R3-reviewer-final-checks.json), [validated final manifest](logs/CH-MAIN-B1-R3-final.evidence.json). Root acceptance and main publication remain separate.
