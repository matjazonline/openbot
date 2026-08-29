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

# A raised limit must leave something that fails when it is approached again

Raising a bound to clear a failure also removes the signal that the bound was being approached, and
the next report of it is a crash rather than a red build. So when you raise one, say what now
fails early instead, and keep that in CI.

The worked example is stack size. `.cargo/config.toml` gives test threads 16 MiB so the agent chain
cannot overflow them — which on its own would let that chain grow twelvefold unnoticed, and it had
already grown silently once. `scripts/stack-budget.sh` re-runs the suite at the stock 2 MiB and is
the thing that actually catches it. When it fails, shrink the chain before raising
`STACK_BUDGET_KIB`, and record the reason either way. Its threshold is calibrated per platform, so
treat a first failure on new hardware as calibration, not regression.

Settings that bound the same resource for different threads move together:
`RUNTIME_THREAD_STACK_BYTES` covers the threads the process spawns, `RUST_MIN_STACK` covers the
ones libtest spawns, and neither covers the other.
