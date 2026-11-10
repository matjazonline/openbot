# Doop contract captures

Captured from a disposable local deployment of the modified Doop source identified
by the base revision and exact source fingerprint in `provenance.json`, using
ordinary signup/workspace/key APIs, configured company bindings and synthetic data.
The source is https://github.com/kgoedecke/doop (AGPL-3.0); these are observed MCP
catalog/result fixtures, not a vendored implementation or an assertion of production support.
Known synthetic identities and origin are normalized, and credentials are excluded.

Regenerate with `scripts/doop-test.py` as described in `docs/doop-testing.md`, review the
captured schemas/results, and update provenance hashes with the reviewed output. The
configured local isolation suite passes; the historical unmodified-upstream failures
remain in the phase evidence. The pinned source/image validation record is in integrations/doop/README.md. Screenshot/PDF fixtures prove
transport shape, not model visual review or confidential media access.

The full static `get_guide` and 48-tool catalog were refreshed from the actual private
production-image run identified in `provenance.json`. Its complete 29,947-byte guide
is also split at section boundaries into three ordered company Skills. Document
workflow/agent tests exercise the full text within existing Skill, context and provider
budgets. No live credential is included.

Private export captures (`private-*.json`) come from the file-access phase04 actual
production-image/native PostgreSQL/private Garage/local TLS run. Their separate
`private-exports-provenance.json` pins source-file and result hashes and exact call
arguments. `private-exports-tools.json` is the exact two-tool subset of that run's
catalog; `tools.json` is the matching complete release capture. Disposable local
identities and URLs are retained unchanged; no credential is included.

These dense image results exercise real bounded JPEG previews. The large PDF
result contains an authenticated two-page download location; the small PDF contains
the complete bounded file. The actual-image run checked private font embedding,
rendered pixels/glyphs, normal-session downloads and denied signed-out/foreign/bearer
access. The UI also saved the selected private PDF through its normal export control.
Guarded-transport and workflow-registry tests consume all five actual shapes while
leaving URLs inert and retaining the existing 16,384-byte field rejection.
`private-acceptance-report.json` is the actual sanitized report used to challenge the
runner's required-case and request-budget gate. Full service and independent
acceptance remain separate from replay; no production or model visual-review claim.
