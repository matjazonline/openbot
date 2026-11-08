# VG-P06-A2-R1 — CHANGES REQUIRED

The revised architecture **VG-P06-D1/r2** is not accepted. SHA256 `9e83a43dc5bfc89490521d30793a8eba51ed9aa81d66eb33c12f6491884c0ef6`.

**VG-P06-A2-F1 (high):** retaining rmcp1.8.0 WorkerTransport for cancel-and-close keeps handshake channels open while its worker waits without observing cancellation. The proposed cleanup transfer can therefore hang; timeout-and-drop loses the join handle. Pinned-source inspection and an independent actual-API probe confirm this edge. The decision must cover handshake channel waits as well as in-flight HTTP and retain awaited completion ownership.

Other r2 authority/request-boundary and F1 mapping decisions were reviewed without a further finding. Proposed C005/r2 remains unapproved; B1-F1/F2/F3 stay open until corrected code review.

Base/HEAD `ace526fd8259ec53c5199ffa7555a1a99dcba58e`; unchanged source digest `a781a861b6d39b7abccfdc0706619a7cfcc5573a440f5026ae0dbb492218c658`; coordination19. Exact exclusions remain COORDINATION.md and this phase's docs.

[Full findings](agent-review-findings.md#vg-p06-a2-f1--retained-workertransport-cannot-finish-cancelled-handshake-channel-waits), [notes](agent-review-notes.md#vg-p06-a2-r1), [probe](logs/reviewer-A2-worker-close-run.log), [entry](logs/reviewer-A2-R1-entry.txt), [exit](logs/reviewer-A2-R1-exit.txt). Prior B1 review retained in [immutable copy](logs/reviewer-B1-R1-REVIEW-retained.md). No project source changes;07 remains blocked.
