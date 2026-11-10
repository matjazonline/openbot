//! The supervised memory lifecycle reconciler (E2, E9). It drives each remote tenant and
//! namespace towards its durable desired state through the provisioner credential, one claimed
//! row at a time. Completion is fenced to the claim and its generation, so a stale worker or a
//! superseded intent never settles a row; only a definite answer from the service settles one.
use super::*;
use crate::app_error::AppResult;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// The claim lease. One pass (at most a few sequential requests) stays below it.
pub const LIFECYCLE_LEASE: Duration = Duration::from_secs(60);
/// The local deadline of one pass, below the lease, so a pass never outlives its ownership.
pub const LIFECYCLE_DEADLINE: Duration = Duration::from_secs(45);
pub const MAX_LIFECYCLE_ATTEMPTS: i32 = 8;
/// How long a row recorded under another provisioner credential waits before it is looked at
/// (and logged) again. Refusal consumes no attempt: it resumes once the ids match.
pub const PROVISIONER_MISMATCH_RETRY: Duration = Duration::from_secs(300);
/// Strays revoked per pass; more are left for the next pass, which is due at once and is charged
/// no attempt (it is progress, not failure).
const MAX_REVOCATIONS_PER_PASS: usize = 16;
/// The service lists at most this many credentials per tenant, oldest id first, and has no
/// pagination. A full listing may hide strays, so it never counts as revoked.
pub const SERVICE_CREDENTIAL_LIST_LIMIT: usize = 1_000;
/// Pages of one namespace's live credentials read per pass (p06 A3-6); more is
/// `credential_list_truncated`, never "all revoked".
pub const MAX_CREDENTIAL_PAGES: usize = 4;
/// How long a namespace waits for its tenant (not yet created, or disabled), without an attempt.
pub const TENANT_WAIT: Duration = Duration::from_secs(30);
/// Outage retries after which a row counts as stuck (review p7b r2 m2): about half an hour at
/// the five-minute backoff cap. Crossing it logs once at `error`; startup lists such rows.
pub const STUCK_TRANSIENT_RETRIES: i32 = 12;

/// Which budget a retry is charged to (review p7b r1 M2). A service outage, a revoked
/// provisioner or an interrupted pass is not poison: it retries without limit, backing off to
/// five minutes, and stays visible. Anything else spends one of the bounded attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryBudget {
    Outage,
    Bounded,
}

impl RetryBudget {
    pub fn of(outcome: &str) -> Self {
        match outcome {
            "service_unavailable"
            | "outcome_unknown"
            | "service_capacity"
            | "provisioner_unauthorized"
            | "interrupted"
            | "deadline_exceeded"
            | "store_unavailable" => Self::Outage,
            _ => Self::Bounded,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleResource {
    Tenant,
    Namespace(NamespaceExternalId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleDesired {
    Present,
    Disabled,
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleObserved {
    Present,
    Disabled,
    Absent,
}

impl LifecycleObserved {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Disabled => "disabled",
            Self::Absent => "absent",
        }
    }
}

/// One claimed row, as the store fenced it. The worker never edits it; every completion presents
/// it back whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleClaim {
    pub id: Uuid,
    pub tenant: TenantExternalId,
    pub resource: LifecycleResource,
    pub provisioner_credential_id: Uuid,
    pub desired: LifecycleDesired,
    pub generation: i64,
    pub fence: i64,
    /// Attempts already consumed, including this claim's if it reclaimed an expired lease.
    pub attempts: i32,
    /// Outage retries so far in this generation.
    pub transient_retries: i32,
}

/// What a fenced completion found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleCompletion {
    Applied,
    /// The desired state changed meanwhile: nothing was settled and the row is due again.
    Superseded,
    /// Another claim owns the row (or it is gone): nothing was written.
    Lost,
}

/// The durable side of the reconciler. Every write requires the claim's fence while the row is
/// still claimed; `settle` and `store_credential` also require its generation.
#[async_trait]
pub trait MemoryLifecycleStore: Send + Sync {
    /// Claim one due row (pending and due, or claimed with an expired lease, which consumes an
    /// attempt). A row whose owner is gone has its `present` turned into `absent` first.
    async fn claim_lifecycle(
        &self,
        worker: Uuid,
        lease: Duration,
    ) -> AppResult<Option<LifecycleClaim>>;
    /// The tenant of a namespace claim is created and enabled remotely.
    async fn tenant_ready(&self, claim: &LifecycleClaim) -> AppResult<bool>;
    /// The client credential id stored for the claimed namespace, if one is.
    async fn stored_credential(&self, claim: &LifecycleClaim) -> AppResult<Option<Uuid>>;
    /// The desired state holds remotely. A tenant records `generation_cap`; an absent namespace
    /// row is removed.
    async fn settle_lifecycle(
        &self,
        claim: &LifecycleClaim,
        observed: LifecycleObserved,
        generation_cap: Option<u64>,
    ) -> AppResult<LifecycleCompletion>;
    /// Seal and store a freshly issued client credential, marking the namespace ready and the
    /// lifecycle settled, all under the fence and generation. With `replaces`, the namespace
    /// must still hold that credential id: it is swapped in place, so a ready namespace stays
    /// ready throughout (p06 A4-8).
    async fn store_credential(
        &self,
        claim: &LifecycleClaim,
        credential: IssuedClientCredential,
        replaces: Option<Uuid>,
    ) -> AppResult<LifecycleCompletion>;
    /// Back off (2ⁿ s to 5 min, or `retry_after`). `Bounded` consumes an attempt and the last
    /// one fails; `Outage` counts a transient retry instead and never fails.
    async fn retry_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
        retry_after: Option<Duration>,
        budget: RetryBudget,
    ) -> AppResult<LifecycleCompletion>;
    /// Terminal: shown to operators, never retried.
    async fn fail_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
    ) -> AppResult<LifecycleCompletion>;
    /// Release without consuming an attempt (the provisioner credential mismatch).
    /// Re-record the claim's company (its tenant row and every lifecycle row) under the
    /// configured provisioner credential, after the service confirmed it manages the tenant
    /// (p06 A3-4).
    async fn adopt_provisioner(&self, claim: &LifecycleClaim, credential: Uuid) -> AppResult<()>;
    async fn postpone_lifecycle(
        &self,
        claim: &LifecycleClaim,
        outcome: &'static str,
        delay: Duration,
    ) -> AppResult<LifecycleCompletion>;
}

/// The decision a pass ends in.
enum Step {
    Settle(LifecycleObserved, Option<u64>),
    /// Store a freshly issued credential, replacing the stored one named (p06 A4-8) or none.
    Store(IssuedClientCredential, Option<Uuid>),
    Retry(&'static str, Option<Duration>),
    /// Not a failure: due again after the delay, without an attempt.
    Wait(&'static str, Duration),
    Fail(&'static str),
}

/// The generic retry classification of a failed request.
fn retry(error: &MemoryServiceError) -> Step {
    match error {
        MemoryServiceError::Unavailable => Step::Retry("service_unavailable", None),
        MemoryServiceError::Ambiguous => Step::Retry("outcome_unknown", None),
        MemoryServiceError::Capacity { retry_after_secs } => Step::Retry(
            "service_capacity",
            retry_after_secs.map(Duration::from_secs),
        ),
        // A revoked provisioner: client credentials keep serving; the operator acts (E1).
        MemoryServiceError::Unauthorized => Step::Retry("provisioner_unauthorized", None),
        MemoryServiceError::Forbidden { code } if code == "tenant_disabled" => {
            Step::Retry("tenant_disabled", None)
        }
        MemoryServiceError::Forbidden { .. } => Step::Retry("forbidden", None),
        MemoryServiceError::NotFound => Step::Retry("not_found", None),
        MemoryServiceError::Conflict { .. } => Step::Retry("conflict", None),
        MemoryServiceError::Expired
        | MemoryServiceError::Refused { .. }
        | MemoryServiceError::Protocol => Step::Retry("protocol_error", None),
    }
}

pub struct MemoryLifecycleWorker {
    store: Arc<dyn MemoryLifecycleStore>,
    provisioner: Arc<dyn MemoryProvisioner>,
    /// The configured provisioner credential id; rows recorded under another one are refused.
    credential_id: Uuid,
    worker: Uuid,
    poll: Duration,
}

impl MemoryLifecycleWorker {
    pub fn new(
        store: Arc<dyn MemoryLifecycleStore>,
        provisioner: Arc<dyn MemoryProvisioner>,
        credential_id: Uuid,
    ) -> Self {
        Self {
            store,
            provisioner,
            credential_id,
            worker: Uuid::new_v4(),
            poll: Duration::from_secs(2),
        }
    }

    /// Until `stop`: work while rows are due, poll when none are, back off after a store error.
    pub async fn run(&self, stop: &CancellationToken) {
        loop {
            let delay = match self.pass(stop).await {
                Ok(true) => Duration::ZERO,
                Ok(false) => self.poll,
                Err(error) => {
                    tracing::warn!(error = %error, "Memory lifecycle pass failed; retrying");
                    Duration::from_secs(5)
                }
            };
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }

    /// One claimed row, reconciled and completed. `false` when nothing was due. A full pass is
    /// evidence of progress only because every completion moves the row out of the due set.
    pub async fn pass(&self, stop: &CancellationToken) -> AppResult<bool> {
        let Some(claim) = self
            .store
            .claim_lifecycle(self.worker, LIFECYCLE_LEASE)
            .await?
        else {
            return Ok(false);
        };
        if claim.provisioner_credential_id != self.credential_id {
            // p06 A3-4: after an operator reassigned the tenant to the configured credential,
            // the company's rows adopt it; nothing moves to an id the service has not confirmed.
            match self.manages_tenant(claim.tenant).await {
                Ok(true) => {
                    self.store
                        .adopt_provisioner(&claim, self.credential_id)
                        .await?;
                    tracing::info!(
                        lifecycle_id = %claim.id,
                        recorded_provisioner = %claim.provisioner_credential_id,
                        configured_provisioner = %self.credential_id,
                        "Memory lifecycle adopted the configured provisioner credential"
                    );
                }
                Ok(false) => {
                    tracing::warn!(
                        lifecycle_id = %claim.id,
                        recorded_provisioner = %claim.provisioner_credential_id,
                        configured_provisioner = %self.credential_id,
                        "Memory lifecycle refused: recorded under another provisioner credential"
                    );
                    self.store
                        .postpone_lifecycle(
                            &claim,
                            "provisioner_mismatch",
                            PROVISIONER_MISMATCH_RETRY,
                        )
                        .await?;
                    return Ok(true);
                }
                Err(step) => {
                    self.complete(&claim, step).await?;
                    return Ok(true);
                }
            }
        }
        let step = tokio::select! {
            biased;
            _ = stop.cancelled() => Step::Retry("interrupted", None),
            step = tokio::time::timeout(LIFECYCLE_DEADLINE, self.step(&claim)) => {
                step.unwrap_or(Step::Retry("deadline_exceeded", None))
            }
        };
        let completion = self.complete(&claim, step).await?;
        tracing::info!(
            lifecycle_id = %claim.id,
            generation = claim.generation,
            completion = ?completion,
            "Memory lifecycle pass"
        );
        Ok(true)
    }

    async fn complete(&self, claim: &LifecycleClaim, step: Step) -> AppResult<LifecycleCompletion> {
        match step {
            Step::Settle(observed, cap) => self.store.settle_lifecycle(claim, observed, cap).await,
            Step::Store(credential, replaces) => {
                self.store
                    .store_credential(claim, credential, replaces)
                    .await
            }
            Step::Retry(outcome, after) => {
                let budget = RetryBudget::of(outcome);
                tracing::warn!(lifecycle_id = %claim.id, outcome, budget = ?budget,
                    "Memory lifecycle deferred");
                if budget == RetryBudget::Outage
                    && claim.transient_retries + 1 == STUCK_TRANSIENT_RETRIES
                {
                    tracing::error!(lifecycle_id = %claim.id, outcome,
                        transient_retries = STUCK_TRANSIENT_RETRIES,
                        "Memory lifecycle stuck: retrying an outage without limit; see \
                         docs/deploy.md, Memory service");
                }
                self.store
                    .retry_lifecycle(claim, outcome, after, budget)
                    .await
            }
            Step::Wait(outcome, delay) => {
                self.store.postpone_lifecycle(claim, outcome, delay).await
            }
            Step::Fail(outcome) => {
                tracing::error!(lifecycle_id = %claim.id, outcome, "Memory lifecycle failed");
                self.store.fail_lifecycle(claim, outcome).await
            }
        }
    }

    async fn step(&self, claim: &LifecycleClaim) -> Step {
        let tenant = claim.tenant;
        match (claim.resource, claim.desired) {
            (LifecycleResource::Tenant, LifecycleDesired::Present) => {
                self.tenant_present(tenant).await
            }
            (LifecycleResource::Tenant, LifecycleDesired::Disabled) => {
                self.tenant_disabled(tenant).await
            }
            (LifecycleResource::Tenant, LifecycleDesired::Absent) => {
                self.tenant_absent(tenant).await
            }
            (LifecycleResource::Namespace(namespace), LifecycleDesired::Present) => {
                match self.store.tenant_ready(claim).await {
                    Ok(true) => self.namespace_present(claim, namespace).await,
                    Ok(false) => Step::Wait("tenant_not_ready", TENANT_WAIT),
                    Err(_) => Step::Retry("store_unavailable", None),
                }
            }
            (LifecycleResource::Namespace(namespace), LifecycleDesired::Absent) => {
                self.namespace_absent(tenant, namespace).await
            }
            (LifecycleResource::Namespace(_), LifecycleDesired::Disabled) => {
                Step::Fail("invalid_lifecycle")
            }
        }
    }

    /// Create is idempotent for us; a tombstone or a foreign tenant is a uniform 409.
    /// Whether the configured credential manages `tenant`, in any state (p06 A3 F5). A listing
    /// answers for every tenant its manager manages, tombstones included; only its 404 falls
    /// back to a create, whose 200 also proves management (of a tenant the probe may itself have
    /// created, which is then ours).
    async fn manages_tenant(&self, tenant: TenantExternalId) -> Result<bool, Step> {
        let probe = CredentialQuery {
            label: None,
            active_only: true,
            after: None,
            limit: 1,
        };
        match self
            .provisioner
            .list_client_credentials(tenant, probe)
            .await
        {
            Ok(_) => return Ok(true),
            Err(MemoryServiceError::NotFound) => {}
            Err(error) => return Err(retry(&error)),
        }
        match self.provisioner.create_tenant(tenant).await {
            Ok(_) => Ok(true),
            Err(MemoryServiceError::Conflict { .. } | MemoryServiceError::NotFound) => Ok(false),
            Err(error) => Err(retry(&error)),
        }
    }

    /// After a "not ours" answer: a manager the configured credential is not (yet). Rotation
    /// passes through this until the operator's reassignment lands (p06 A3 F2), so it spends no
    /// attempt; settings show the mismatch and A3-3 gauges the postponed rows.
    async fn not_ours(&self, tenant: TenantExternalId) -> Step {
        match self.manages_tenant(tenant).await {
            Ok(true) => Step::Wait("tenant_recheck", TENANT_WAIT),
            Ok(false) => Step::Wait("provisioner_mismatch", PROVISIONER_MISMATCH_RETRY),
            Err(step) => step,
        }
    }

    async fn create_tenant(&self, tenant: TenantExternalId) -> Result<RemoteTenant, Step> {
        match self.provisioner.create_tenant(tenant).await {
            Ok(view) => Ok(view),
            Err(MemoryServiceError::Conflict { .. }) => Err(Step::Fail("tenant_conflict")),
            Err(error) => Err(retry(&error)),
        }
    }

    async fn tenant_present(&self, tenant: TenantExternalId) -> Step {
        let created = match self.create_tenant(tenant).await {
            Ok(created) => created,
            Err(step) => return step,
        };
        let view = match created.state {
            RemoteTenantState::Active => created,
            RemoteTenantState::Disabled => match self.provisioner.enable_tenant(tenant).await {
                Ok(view) => view,
                Err(MemoryServiceError::NotFound) => return Step::Fail("tenant_unmanageable"),
                Err(error) => return retry(&error),
            },
            RemoteTenantState::Deleted => return Step::Fail("tenant_conflict"),
        };
        Step::Settle(
            LifecycleObserved::Present,
            Some(view.generation_daily_token_cap),
        )
    }

    async fn tenant_disabled(&self, tenant: TenantExternalId) -> Step {
        if let Err(step) = self.create_tenant(tenant).await {
            return step;
        }
        match self.provisioner.disable_tenant(tenant).await {
            Ok(view) => Step::Settle(
                LifecycleObserved::Disabled,
                Some(view.generation_daily_token_cap),
            ),
            Err(MemoryServiceError::NotFound) => Step::Fail("tenant_unmanageable"),
            Err(error) => retry(&error),
        }
    }

    /// Create first, whatever it answers, so an earlier ambiguous create is definite before the
    /// delete; then only a 200 completes. A 404 means the tenant is not ours: never done.
    async fn tenant_absent(&self, tenant: TenantExternalId) -> Step {
        let created = match self.provisioner.create_tenant(tenant).await {
            Ok(_) => true,
            Err(MemoryServiceError::Conflict { .. } | MemoryServiceError::NotFound) => false,
            Err(error) => return retry(&error),
        };
        match self.provisioner.delete_tenant(tenant).await {
            Ok(_) => Step::Settle(LifecycleObserved::Absent, None),
            // Definite only after a create that succeeded in this step: the tenant was ours a
            // moment ago (p06 A3 revision 2, review B1 F1). After a refused create it may be
            // another provisioner's (a reassignment), so the row waits instead.
            Err(MemoryServiceError::NotFound) if created => Step::Fail("tenant_unmanageable"),
            Err(MemoryServiceError::NotFound) => self.not_ours(tenant).await,
            Err(error) => retry(&error),
        }
    }

    /// Create, revoke strays (labelled for this namespace but not the stored credential), then
    /// issue and store under the fence. An ambiguous issue leaves a stray the next pass revokes.
    async fn namespace_present(
        &self,
        claim: &LifecycleClaim,
        namespace: NamespaceExternalId,
    ) -> Step {
        let tenant = claim.tenant;
        match self.provisioner.create_namespace(tenant, namespace).await {
            Ok(RemoteNamespaceState::Active) => {}
            Err(MemoryServiceError::Forbidden { code }) if code == "tenant_disabled" => {
                return Step::Wait("tenant_disabled", TENANT_WAIT);
            }
            // Only `namespace_deleted` is our namespace's tombstone (replaced by a new id at its
            // next use). Any other 409 is the tenant's (deleted, or not ours): definite, never
            // replaced, and shown at tenant level in settings (review p7b r3 m1).
            Ok(RemoteNamespaceState::Deleted) => return Step::Fail("namespace_conflict"),
            Err(MemoryServiceError::Conflict { code }) if code == "namespace_deleted" => {
                return Step::Fail("namespace_conflict");
            }
            Err(MemoryServiceError::Conflict { .. }) => return Step::Fail("tenant_conflict"),
            Err(MemoryServiceError::NotFound) => return self.not_ours(tenant).await,
            Err(error) => return retry(&error),
        }
        let stored = match self.store.stored_credential(claim).await {
            Ok(stored) => stored,
            Err(_) => return Step::Retry("store_unavailable", None),
        };
        let listed = match self.revoke_labelled(tenant, namespace, stored, false).await {
            Ok(listed) => listed,
            Err(step) => return step,
        };
        if stored.is_some() && listed {
            return Step::Settle(LifecycleObserved::Present, None);
        }
        // None stored yet, or the stored one is not among the namespace's live credentials in
        // a complete listing: a restored service never had it, or it was revoked by hand. Issue
        // a new one first and swap it in, so the namespace never leaves `ready` (p06 A4-8).
        let label = client_credential_label(namespace);
        match self
            .provisioner
            .issue_client_credential(tenant, namespace, &label)
            .await
        {
            Ok(credential) => Step::Store(credential, stored),
            Err(error) => retry(&error),
        }
    }

    /// Create first, as for tenants: the service answers 404 for a namespace whose create never
    /// landed and keeps no tombstone, so a delayed create could still land after a bare delete.
    async fn namespace_absent(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Step {
        let disabled = match self.provisioner.create_namespace(tenant, namespace).await {
            Ok(_) => false,
            // Our tombstone, or our tenant's (a tenant we do not manage answers 404, never 409):
            // nothing of this namespace can serve or land.
            Err(MemoryServiceError::Conflict { .. }) => {
                return self.revoked_absent(tenant, namespace).await;
            }
            // p06 A3-5: a 404 means "not ours" or "never created"; it never completes. The probe
            // creates a never-created tenant, after which the next create is definite.
            Err(MemoryServiceError::NotFound) => return self.not_ours(tenant).await,
            Err(MemoryServiceError::Forbidden { code }) if code == "tenant_disabled" => true,
            Err(error) => return retry(&error),
        };
        match self.provisioner.delete_namespace(tenant, namespace).await {
            Ok(_) => self.revoked_absent(tenant, namespace).await,
            // A disabled tenant: a 404 cannot be told apart from a reassignment or a create still
            // in flight without the quiescence deadline (persistence AGENTS), so wait until the
            // tenant is enabled (create + delete is then definite) or deleted (its tombstone
            // answers 409). A disabled tenant serves no client request meanwhile.
            Err(MemoryServiceError::NotFound) if disabled => {
                Step::Wait("tenant_disabled", TENANT_WAIT)
            }
            Err(error) => retry(&error),
        }
    }

    async fn revoked_absent(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
    ) -> Step {
        match self.revoke_labelled(tenant, namespace, None, true).await {
            Ok(_) => Step::Settle(LifecycleObserved::Absent, None),
            Err(step) => step,
        }
    }

    /// Revoke this namespace's live credentials except `keep`. A tenant that is gone or not ours
    /// has none of ours left to revoke.
    /// `absent`: called only after a definite tombstone (create 409 or delete 200), where the
    /// namespace's grants are gone, so a listing that answers "not ours" leaves nothing of it
    /// able to serve. On the present path the tenant was ours a moment ago, so the same answer
    /// postpones instead of settling `present` with strays unseen (p06 A3-5).
    /// Returns whether `keep` is among the live credentials; `Ok` always means the listing was
    /// complete (a truncated one is a retry).
    async fn revoke_labelled(
        &self,
        tenant: TenantExternalId,
        namespace: NamespaceExternalId,
        keep: Option<Uuid>,
        absent: bool,
    ) -> Result<bool, Step> {
        let label = client_credential_label(namespace);
        // The namespace's live credentials only, page by page (p06 A3-6).
        let mut strays: Vec<Uuid> = Vec::new();
        let mut kept = false;
        let mut after = None;
        let mut truncated = false;
        for page in 1..=MAX_CREDENTIAL_PAGES {
            let query = CredentialQuery {
                label: Some(&label),
                active_only: true,
                after,
                limit: SERVICE_CREDENTIAL_LIST_LIMIT as u32,
            };
            let listed = match self
                .provisioner
                .list_client_credentials(tenant, query)
                .await
            {
                Ok(listed) => listed,
                Err(MemoryServiceError::NotFound | MemoryServiceError::Conflict { .. })
                    if absent =>
                {
                    return Ok(false);
                }
                Err(MemoryServiceError::NotFound | MemoryServiceError::Conflict { .. }) => {
                    return Err(self.not_ours(tenant).await);
                }
                Err(error) => return Err(retry(&error)),
            };
            kept |= listed
                .credentials
                .iter()
                .any(|credential| Some(credential.credential_id) == keep && !credential.revoked);
            strays.extend(
                listed
                    .credentials
                    .into_iter()
                    .filter(|credential| {
                        credential.label == label
                            && !credential.revoked
                            && Some(credential.credential_id) != keep
                    })
                    .map(|credential| credential.credential_id),
            );
            match listed.next_after {
                Some(next) if page < MAX_CREDENTIAL_PAGES => after = Some(next),
                Some(_) => truncated = true,
                None => break,
            }
        }
        let mut revoked = 0;
        for stray in strays.iter().take(MAX_REVOCATIONS_PER_PASS) {
            revoked += usize::from(self.revoke(tenant, *stray).await?);
        }
        if strays.len() > MAX_REVOCATIONS_PER_PASS {
            // Free and immediate only while the batch revoked something: a listing that keeps
            // showing credentials the service answers 404 for is not progress (r2 n2).
            return Err(if revoked > 0 {
                Step::Wait("revoking_strays", Duration::ZERO)
            } else {
                Step::Retry("stray_revocation_stuck", None)
            });
        }
        if truncated {
            // Strays past the listing's end cannot be seen, so this is never "all revoked".
            return Err(Step::Retry("credential_list_truncated", None));
        }
        Ok(kept)
    }

    /// `true` when this call revoked it; `false` when the service no longer knows it.
    async fn revoke(&self, tenant: TenantExternalId, credential: Uuid) -> Result<bool, Step> {
        match self
            .provisioner
            .revoke_client_credential(tenant, credential)
            .await
        {
            Ok(()) => Ok(true),
            Err(MemoryServiceError::NotFound) => Ok(false),
            Err(error) => Err(retry(&error)),
        }
    }
}
