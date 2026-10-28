# 04 — Image assets and Box questions

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Extend the same `jev.evaluate` step with image assets and the `box` question variant. TypeSafe
currently labels images and Box experimental and requires `TypeSafe-Danger-Zone-Images: true`.
Box requires a compatible versioned model (currently Jev 1.14). Document that status in the
authoring UI; do not silently turn on image requests for text-only steps.

## Batches and checks

1. Define `with.assets` as a bounded map of logical names to immutable, company-owned private
   file references. Add an application-owned read port implemented by the storage/persistence
   adapters. Before reading, verify company, actor, and file visibility; reject public arbitrary
   URLs, cross-company files, mutable/unverifiable objects, unsupported MIME types, and files
   beyond a deliberately lower byte/pixel limit than the upstream maximum. Capture the
   content digest in the frozen request identity so a changed object cannot silently alter a
   retry. Never put base64 image bytes into a published bundle, run view, trace, or sample report.
2. Add the `box` tag to the literal question union. It needs instructions identifying a declared
   image asset and forbids `criteria`. Use server-generated opaque asset placeholders when
   constructing TypeSafe's request, rather than a short user-chosen token that could match
   unrelated text. Send the image opt-in header only when assets or Box are present. Validate
   `box.left < box.right`, `box.top < box.bottom`, all four coordinates in [0, 1], and
   confidence in [0, 1]. Derive static downstream output paths for mixed Box and text questions.
3. Add a scripted image sample and controlled loopback integration fixture. Samples use
   supplied scripted results and inert image metadata, never fetch private files or call TypeSafe.
   Surface a clear error if an image is missing, unauthorized, changed, or unsupported.

Focused checks after batch 1: `cargo test --locked --offline --lib jev_assets_` and relevant
attachment/storage authorization tests. After batch 2:
`cargo test --locked --offline --lib jev_box_` and
`cargo test --locked --offline --lib jev_contract_`. After batch 3 run the sample-boundary
checks (`python3 scripts/tests/workflow-sample-boundary.py` and
`python3 scripts/workflow-sample-boundary.py`), plus the relevant sample and runtime tests.
List filters first; record commands, results, and test counts. Include an actual cross-company
negative database test and a changed-object retry test.

Gate: one request can contain Box plus text questions, but no image bytes or unauthorized asset
can escape through publication, logs, samples, or stale retries.
