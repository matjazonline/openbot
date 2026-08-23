These rules apply across the repository. More specific guides under `src/` add source-level and
subsystem rules.

# Keep builds and deployment secure and reproducible

Run the production image as a non-root user unless a narrowly documented capability is required
for a privileged port. Prefer capability-based port binding or an outer proxy over running the
whole application as root.

Pin git dependencies by revision or tag in addition to committing the lockfile. Production
documentation and examples must name configuration keys the application actually reads and must
not promise validation that startup does not perform.

CI must exercise formatting, offline compilation, migrations, and the database-backed test suite.
When adding or changing a concurrency protocol, include a test with competing claimants rather
than relying only on sequential mocks.
