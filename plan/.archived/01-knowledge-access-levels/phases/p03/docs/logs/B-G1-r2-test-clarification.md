# G1-R2 author clarification retained by B

Received from root during independent gate review, 2026-11-05. No source edits accompanied it.

The replacement-claim test will retire/release a live old claim, make backoff due and obtain a real new claim while the old cached window is still future, so it exercises actual fence mismatch rather than only monotonic deadline expiry. Separate SQL-shortened expiry tests leave claim.window future and cross actual DB lease/deadline before call, during a statement and during COMMIT.

B interprets this as a test-mechanics clarification within D3's proposed ownership/commit guard, not a change to its contract. The decision file SHA remains `da4f77abd6bf3f520050cd599c1ff2e7ff91ac0fa9eedbc7204720f1cee38dcb`.
