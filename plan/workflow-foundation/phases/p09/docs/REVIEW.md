# P09 review record

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Workflow `implement-opus-reviewed-2_2`; phase `p09`; worktree `/Users/mac02/dev/openbot` (branch
`workflows`). Notes: `agent-review-notes.md`; findings: `agent-review-findings.md`; task:
`TASK.md`. Coordination pinned: `COORDINATION.md` r11, sha256 `3e8501b4…62f5`
(`logs/coordination-r11.md`) for R001–R007; r12, sha256 `05eb8e6e…2c69` (`logs/coordination-r12.md`;
only p09's WF-C003 response added) from R008. Applicable notices: WF-C002 r1, WF-C003 r1, WF-C004 r2, WF-C005 r1.

## Rounds

| Round | Date | Batch / stage | Artifact digest | Outcome | Findings |
| --- | --- | --- | --- | --- | --- |
| WF-P09-R001 | 2026-10-24 | B1a / batch review | `62a6c35e187548f9e6fbd444c9482f4b3e378764cef3dd2dc0a19f561cf84e50` (HEAD `da86871`) | **Changes required** | Actionable: F001 M, F002 M, F003 M, F004 L, F005 L, F006 L, F007 L, F008 L, F009 L. Optional: F010, F011. Tracked gap: F012 |
| WF-P09-R002 | 2026-10-24 | B1a / correction recheck | `b4dc745de5c69e461189ae0c64ad0d38ec3de67e30eeacc7550c9675e9e2de3c` (HEAD `da86871`) | **Changes required (Low only)** | Fixed: F001, F002, F004–F009; F010/F011 adopted. Open: F003 L (residual). New: F013 L, F014 L, F016 L; optional F015. Tracked: F012 |
| WF-P09-R003 | 2026-10-24 | B1a / correction recheck | `1484a5bc494ed3c0202e07678f66566fabe31f0f24ea0e5886620254cd6957db` (HEAD `da86871`) | **PASS** | F003, F013, F014, F016 fixed; F015 adopted. All B1a findings resolved except F012 (open by plan: not B1a-blocking, phase-blocking) |
| WF-P09-R004 | 2026-10-24 | B1b / architecture gate P09-B1b-D1 rev 1 | decision revision (no digest; tree in flux) | **PASS** | Optional: F017, F018 |
| WF-P09-R005 | 2026-10-24 | B1b / batch review + F012 browser pass | `293c80357296a898b19f1e83b6eed47cb45f5dc74fcc83d00840aab418878555` (HEAD `da86871`) | **Changes required** | F019 M, F020 M, F021 L; F017/F018 verified; F012 residual (keyboard) L |
| WF-P09-R006 | 2026-10-25 | B1b / correction recheck | `40320058996d47820d1c1517c683517973c5ac745a38eaf82aa77636fea1156e` (HEAD `da86871`) | **Changes required (Low only)** | F019, F020 fixed; F021 residual L (text inputs strip newlines); F012 residual L unchanged |
| WF-P09-R007 | 2026-10-25 | B1b / correction recheck | `89d23e56e74c70e6fcea1d60a9b9b87d4cc5c829af3afd34e8d8ab8147760eb9` (HEAD `da86871`) | **PASS** | F021 fixed; B1b has no open actionable finding; F012 keyboard residual L stays open (phase-blocking, not B1b-blocking) |
| WF-P09-R008 | 2026-10-25 | B2a / batch review (runs workspace, detail, controls, live updates) | `a62d5b7ea0affde68111a687eef03c60365a49c4427c669262fef739aa7e70a2` (HEAD `da86871`) | **Changes required** | F022 M, F023 M, F024 L, F025 L, F026 L, F027 L (verification), F028 L (records), F029 L; F012 residual L unchanged |

| WF-P09-R009 | 2026-10-25 | B2a / correction recheck (Astra 2.2) | `bb37b8a157f40f67a0ba9f478500d2dd755709ce25267ab1bd6a0642f3b6330e` (HEAD `da86871`) | **Changes required** | Fixed F022, F024–F026, F028–F029; residual F023 M, F027 L; new F030 L; separate F012 unchanged |

| WF-P09-R010 | 2026-10-25 | B2a / R009 correction recheck (Astra 2.2) | `96c6c401cffc64d5566a19b414a52fdcacb7d00a1e402ea3c139d506e43ffde6` (HEAD `da86871`) | **PASS** | F023, F027, F030 fixed; B2a has no open actionable findings; separate final-phase F012 unchanged |

| WF-P09-R011 | 2026-10-25 | B2b / whole-batch review | `0d695075870e4287db513ffcb96c9cb1b5d54ab66f0332e441f7759e281e83a4` (HEAD `da86871`) | **Changes required** | F031 L, F032 L, F033 L; separate F012 unchanged |

| WF-P09-R012 | 2026-10-25 | B2b / R011 correction recheck | `5578887c5eb2b7e4eb0e76b62bb02e1140e9fbd4c5bb8cd415530a368af6ca6d` (HEAD `da86871`) | **PASS** | F031–F033 fixed; B2b has no open actionable findings; separate F012 unchanged |

| WF-P09-R013 | 2026-10-25 | B4 / architecture gate P09-D1 r1 | decision SHA-256 `56dcd3ff0290aa3d04f093c963f57964324a69efaa0c148cc9049387a1bfd07f` | **PASS** | No findings; decision only, implementation unreviewed; B3 acceptance still prerequisite |

| WF-P09-R014 | 2026-10-25 | B3 / whole-batch review | `8cb2171b82b9e72e6d5a69813b81066dc7012e0fc9e3674815221f338e794b5b` (HEAD `da86871`) | **Changes required** | F034 M, F035 L; separate F012 unchanged |

| WF-P09-R015 | 2026-10-25 | B3 / R014 correction recheck | `f0ed97efaa607c1efb2cf5077f69fc40106355e9387ae7b9b100020921698785` (HEAD `da86871`) | **Changes required (Low only)** | F034/F035 fixed; F036 L; separate F012 unchanged |

| WF-P09-R016 | 2026-10-25 | B3 / F036 correction recheck | `44077e91138695e2b1d48eda0b50ae1617bf451078c4c6f79303b7f5bd9da575` (HEAD `da86871`) | **PASS** | F036 fixed; no open actionable B3 finding; separate F012 unchanged |

| WF-P09-R017 | 2026-10-25 | B4 / whole-batch review | `c09931634e15711cb7a01c585dd8dc0409b7d89c3ecc7556c96888c34e0bc7c4` (HEAD `da86871`) | **Changes required (Low only)** | F037 L, F038 L; F012 fixed |

| WF-P09-R018 | 2026-10-25 | B4 / F037-F038 correction recheck | `450480d565582b0b2e03a867ee624a2e5b5c3e017644951b558807482fd5a00b` (HEAD `da86871`) | **Changes required (Low only)** | F037 fixed; F038 residual L; F012 stays fixed |

| WF-P09-R019 | 2026-10-25 | B4 / F038 residual recheck | `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea` (HEAD `da86871`) | **PASS** | F038 fixed; no open actionable B4 finding; final integration pending |

| WF-P09-R020 | 2026-10-25 | Final combined p09 integration | `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea` (HEAD `da86871`) | **PASS** | No open actionable findings; full gates passed; documented CSS/ignored-test/SQLx-CLI limitations retained |

## Findings index (latest disposition)

| ID | Severity | Kind | Summary | Status |
| --- | --- | --- | --- | --- |
| WF-P09-F001 | Medium | defect | Parameter-selected agents' frozen tools/skills never shown in the preview | fixed (R002); residual → F016 |
| WF-P09-F002 | Medium | defect | Publication options cut company-wide at 200: needed tools vanish, false "not approved" | fixed (R002); new bound issue → F013 |
| WF-P09-F003 | Low (was Medium) | defect | Unsaved editor text discarded silently (incl. server-echoed unsaved text) | fixed (R003) |
| WF-P09-F004 | Low | defect | "Save draft" shows "Validating…"; reference keeps `aria-busy` / endless spinner | fixed (R002) |
| WF-P09-F005 | Low | defect | Textarea drops a leading newline (silent source change, Go-to-line off by one) | fixed (R002) |
| WF-P09-F006 | Low | verification | Own-company + foreign-id tests missing for writes and compare's version ids | fixed (R002) |
| WF-P09-F007 | Low | verification | MCP UI test passes on a refusal; happy path and argument diagnostic unproven | fixed (R002) |
| WF-P09-F008 | Low | defect | Validate-after-save `Conflict` becomes a 409 toast; publish conflicts mislabelled | fixed (R002) |
| WF-P09-F009 | Low | defect | Sidebar keeps old title/revision after save | fixed (R002) |
| WF-P09-F010 | Low | optional | Say when the published version differs from the preview; name the selected connection | adopted, verified (R002) |
| WF-P09-F011 | Low | optional | Newtypes / named rows for selection keys and operation rows | adopted, verified (R002) |
| WF-P09-F012 | Low | verification gap | Browser/themes/client behaviour closed by `logs/f012`; residual: keyboard walk beyond the tab strip, focus in light | fixed (R017; final full-suite gate remains) |
| WF-P09-F013 | Low | defect | More than 256 tool names of a kind: editor unrenderable, a committed save answers 400 | fixed (R003) |
| WF-P09-F014 | Low | defect | Cancelling the discard prompt from New leaves it disabled and spinning | fixed (R003) |
| WF-P09-F015 | Low | optional | Split `ui/authoring.rs` / `editor_body` / the 1573-line test file before B1b | adopted, verified (R003) |
| WF-P09-F016 | Low | defect | A `/params` agent step lists every frozen agent rather than its enum | fixed (R003) |
| WF-P09-F017 | Low | optional | Gate D1: specify a version-deduplicated closure walk for `frozen_agent_ids` | adopted, verified (R005) |
| WF-P09-F018 | Low | optional | Gate D1: read order/copy, agent-scope source, display-only invariant, element check, extra tests | adopted, verified (R005) |
| WF-P09-F019 | Medium | defect | A not-ready resource reads as "already created" / "someone changed" / "changed since shown" | fixed (R006) |
| WF-P09-F020 | Medium | defect | Revoke / re-point warnings omit that new runs stop and wrongly say parked calls are refused | fixed (R006) |
| WF-P09-F021 | Low | defect | Param fields drop undeclared keys / change numbers / strip newlines on save | fixed (R007) |
| WF-P09-F022 | Medium | defect | The 30 s `tick` reload of a run waiting on reconciliation wipes the typed note and closes the open form and step | fixed (R009) |
| WF-P09-F023 | Medium | defect | Body reloads (resync on every SSE reconnect, tick) replace command keys: the 503 "press it again" rule breaks; a pressed-again rerun starts a second run | fixed (R010) |
| WF-P09-F024 | Low | defect | A same-key edited note after a lost answer reads "Nothing was recorded: the run changed" while the first note was recorded | fixed (R009) |
| WF-P09-F025 | Low | defect | List wake-ups drop the open run's highlight (and clicks never move it) and collapse Load more on every run change | fixed (R009) |
| WF-P09-F026 | Low | defect | In-pane run links (lineage, children, waits, step children) compete for `#run-pane` without `hx-sync` | fixed (R009) |
| WF-P09-F027 | Low | verification | Tenancy tested only for pane/body/step/cancel (not rerun, retry, reconcile, list, stream; hidden-channel admin only on pane); lag arm untested | fixed (R010) |
| WF-P09-F028 | Low | records | `reconciliations.rs` (pre-existing p08 file) shown as created in `b2a.diff`; no base copy | fixed (R009) |
| WF-P09-F029 | Low | defect | Controls, reconciliations, receipts, trigger message and children reads have no run-leading index: working set is the company's history; no plan evidence | fixed (R009) |
| WF-P09-F030 | Low | defect | Waiting parents lose refresh of child state after tick suppression | fixed (R010) |
| WF-P09-F031 | Low | defect | Newest-200 attribution/eligibility window misses still-mounted and backlog messages | fixed (R012) |
| WF-P09-F032 | Low | defect | Every activity refresh loads all company channels for bounded run eligibility | fixed (R012) |
| WF-P09-F033 | Low | instructions | Newly added SQL omits explicit AS for table aliases | fixed (R012) |

| WF-P09-F034 | Medium | defect | Comment observing closed state discards unfinished submission data and feedback | fixed (R015) |
| WF-P09-F035 | Low | defect | Source-message fragment cannot load a source outside newest200 and needs target scrolling | fixed (R015) |

| WF-P09-F036 | Low | defect | Standalone source page inherits controls whose mailbox targets are absent | fixed (R016) |

| WF-P09-F037 | Low | defect | Sample textarea response drops initial newline from raw input | fixed (R018) |
| WF-P09-F038 | Low | verification | P09-D1 sample capability-boundary guard misses absolute qualified paths | fixed (R019) |

R009 uses `implement-astra-reviewed-2.2`; coordination r13 pinned in `logs/coordination-r13.md`. See the R009 note for exact snapshot exclusions, checks and retained evidence.


## Final integration acceptance — R020

Current workflow: `implement-astra-reviewed-2.2`; authoritative coordination r13 (immutable
`logs/coordination-r13.md`), notices WF-C002r1/C003r1/C004r2/C005r1 unchanged.
**PASS** for `76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea` at HEAD
`da868710d3b235d49856e3e481f8f3735f6b3e56`. All phase implementation findings are fixed/adopted;
F012 is closed. See R020 in agent-review-notes.md for acceptance mapping and full evidence.

Locked offline all-target compilation/Clippy/test compilation, network-isolated full suite
(2306 library +5 main), stock2MiB stack suite and130-test lease gate passed. Actual admin/operations
journeys and final decision browser passed. Retained limitations:3 intentional ignores; absent
SQLx CLI covered by actual migrator and direct-cache checks; pre-existing CSS regeneration drift
DV-P09-8. This review does not claim a clean whole-repository CSS regeneration or live-DNS tests.
