# CH-P01-AG1-R2 identity and delta inspection

2026-11-10. Decision file `CH-P01-M1-r2.md` and identity JSON agree on SHA256 `aabc37ecdcb5960c066b65c58b001f3020c9852ff4f8ebb189d45194eaa61853`; identity decision is CH-P01-M1/r2. Its initial Markdown heading still says r1; the filename, identity JSON, revision-2 dispositions and this exact hash unambiguously identify the reviewed r2 contract.

Diff against immutable r1 changes the JSON boundary and library-copy identity policy plus revision dispositions only. No implementation edits exist. GNU-coreutils snapshot helper returns `bf9044fe37778c1b4985e3234f5635a0c8ce175bce2e7216a6068d3807f16210`, base/HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`, excluding exactly `plan/cloudroom-harness/COORDINATION.md` and `plan/cloudroom-harness/phases/p01/docs`. Coordination matches the retained revision-1 copy byte-for-byte. No active notices.

No behavioral tests executed. Prior source/caller inspection remains applicable because source and HEAD are unchanged. Proposed collision, largest-valid, 75%-headroom and 413 tests are accepted obligations, not execution evidence.
