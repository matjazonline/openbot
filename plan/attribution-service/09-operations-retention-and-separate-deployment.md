# 09 — Operations, retention and separate deployment

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [08](08-analytics-and-attribution-dashboard.md). Attribution has bounded operational
behavior and can be deployed on another server, or copied into another repository, while keeping
the same API and identity contracts.

## Batch 09A — Make failure, retention and recovery explicit

1. Instrument redirect latency/storage failure, ingestion acceptance/rejection, merchant delivery
   lag, duplicate/conflicting events, unattributed reasons, pending reconciliation age, report
   latency, rate limits, identity failures and lifecycle freshness. Carry safe correlation IDs;
   never log tokens, full request bodies, authorization headers or PII-bearing URLs.
2. Enforce the consent/retention policy from phase 01. Define separate lifetimes for raw clicks,
   observations, order/refund financial facts, event deduplication records, audit records, and
   tombstones. Retain the evidence needed for the attribution window and merchant retry/refund
   horizon; deleting deduplication rows must not make old deliveries count again.
3. Freeze reporting dimensions needed by retained orders before pruning raw click data. Never
   silently change existing attribution because a click was deleted. Tenant deletion must cover
   credentials, installations, observations, orders, exports and caches under the agreed policy,
   and retain only the minimum tombstone needed to prevent delayed recreation.
4. Run cleanup/reconciliation in bounded batches under owned shutdown/recovery. Test competing
   cleanup and ingestion/settlement, and ensure one poison tenant cannot monopolize the worker.
   Expose unresolved jobs and retry state with appropriate operator permissions.
5. Back up the attribution database and required encryption material independently. Restore into
   a fresh isolated database and verify orders, refunds, keys, pending events and tenant boundaries.
   Document recovery objectives from measured results. A backup of one application is not a backup
   of the other. An incompatible schema follows the reset policy; never invent rollback compatibility.

Focused checks: `attribution_retention_`, `attribution_recovery_` and `attribution_limits_` with real
database races, expiry boundaries, replay after pruning, process interruption and backup restore.
Use representative load to measure the agreed targets. A raised resource limit must have a
documented CI tripwire before the new limit is reached; do not weaken existing stack/lease budgets.

## Batch 09B — Prove the deployment and source boundaries

1. Ship the service's standalone non-root image, example deployment, secret provisioning, health
   probes, TLS/proxy settings and exact configuration reference. Specify trusted proxies, public
   tracking origin, app origin/callback URLs, Openbot authority URL and timeouts using keys that
   the implementation actually reads. Keep the initial public deployment origin stable when
   moving compute so existing tracking links and merchant callbacks continue to resolve.
2. Exercise two deployment topologies with identical application artifacts: separate containers
   on one host; attribution on another host/network with Openbot reachable only over HTTPS. Keep
   the database private with its own role. Moving the database, if needed, is a separately tested
   backup/restore and cutover operation, not a cross-database dependency.
3. In the separate deployment test, deny attribution all access to Openbot's database, internal
   filesystem and private service ports. Allow only the documented identity/company API and SSO
   endpoints. Likewise deny Openbot direct attribution-database access. Verify a complete journey.
4. Copy `services/attribution/` into a clean directory/repository and build, migrate, serve assets,
   and test it without parent files. Verify Openbot still builds independently. The necessary
   integration contracts and deployment docs must travel with the extracted service.
5. Demonstrate Openbot outage behavior: new logins/current user-authorized actions fail as specified;
   existing merchant/public traffic runs only within the configured company-status freshness bound;
   expiration stops tenant operations, and reconnection reconciles status before access resumes.
   Do not claim account-service independence merely because the server is remote.

Focused checks: `attribution_deployment_`, standalone-copy/container tests, blocked cross-database
connections, browser SSO across hosts, key verification on the remote instance, lifecycle expiry,
restart/retry recovery and restoration. Record commands, topology and measured results here.

Completion: service relocation requires no source extraction, UI rewrite, shared database access
or new user credentials. Final integrated verification and pilot acceptance are in
[10](10-integration-verification-and-pilot.md).
