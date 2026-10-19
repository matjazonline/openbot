//! Trusted SQL composition: authority owners and local effects share the writer transaction.
//! Hooks may only access this database; network/provider I/O is forbidden here.
use super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::*;
use crate::application::workflow::actions::*;
use crate::application::workflow::binding::ResourceStatus;
use crate::application::workflow::lease::LeaseWindow;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::Instant;

pub(super) struct AuthorizedAction {
    pub action: FrozenAction,
    pub current_policy: Value,
    pub approval_required: bool,
}

pub struct LockedActionAuthority {
    pub resource: ResourceStatus,
    pub policy: CurrentActionPolicy,
}

#[async_trait]
pub trait SqlActionAuthority: Send + Sync {
    /// Lock actual resource/policy owners against revocation through commit. Missing
    /// or unsupported owners must return an error, never cached directory observations.
    async fn lock_current(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        authority: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority>;
}

#[async_trait]
pub trait SqlLocalAction: Send + Sync {
    /// Registered same-database effect only. All writes must use the supplied transaction.
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: &FrozenAction,
    ) -> AppResult<Value>;
}

/// Production resource/policy owners are supplied by04.9/10. There is no fallback grant.
pub struct UnavailableActionAuthority;
#[async_trait]
impl SqlActionAuthority for UnavailableActionAuthority {
    async fn lock_current(
        &self,
        _: &mut Transaction<'_, Postgres>,
        _: &ActionRunAuthority,
    ) -> AppResult<LockedActionAuthority> {
        Err(missing())
    }
}

pub struct PostgresActionDispatch<A, L> {
    pub(super) persistence: PostgresPersistence,
    pub(super) authority: A,
    local: L,
    pub(super) decision_notices: Option<crate::application::workflow::decisions::DecisionNotices>,
}
impl<A, L> PostgresActionDispatch<A, L> {
    pub fn new(persistence: PostgresPersistence, authority: A, local: L) -> Self {
        Self {
            persistence,
            authority,
            local,
            decision_notices: None,
        }
    }
    pub fn with_decision_notices(
        mut self,
        notices: crate::application::workflow::decisions::DecisionNotices,
    ) -> Self {
        self.decision_notices = Some(notices);
        self
    }
}

impl<A: SqlActionAuthority, L: SqlLocalAction> PostgresActionDispatch<A, L> {
    pub(super) async fn authorize_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        server: Duration,
    ) -> AppResult<AuthorizedAction> {
        let actor = action_authority::prelock_on(tx, request.scope()).await?;
        self.authorize_prelocked_on(tx, request, server, actor)
            .await
    }

    pub(super) async fn authorize_prelocked_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        server: Duration,
        actor: Option<WorkflowActor>,
    ) -> AppResult<AuthorizedAction> {
        if !lease::lock_fence(tx, request.fence, server).await? {
            return Err(conflict());
        }
        // The existing attempt remains the sole ownership ledger.
        let attempt: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM task_attempts WHERE task_id=$1 AND attempt_number=$2 \
             AND execution_generation=$3 AND worker_id=$4 AND status='processing' FOR UPDATE",
        )
        .bind(request.fence.scope.job.0)
        .bind(request.fence.attempt.0)
        .bind(request.fence.generation.0)
        .bind(request.fence.worker.0)
        .fetch_optional(&mut **tx)
        .await?;
        if attempt.is_none()
            || lease::live_window(tx, request.fence, Instant::now())
                .await?
                .is_none()
        {
            return Err(conflict());
        }
        let authority = Box::pin(action_authority::load_on(
            tx,
            request.scope(),
            &request.subject,
            actor,
        ))
        .await?;
        validate_run_authority(request.scope(), &request.subject, &authority)?;
        let current = self.authority.lock_current(tx, &authority).await?;
        validate_resource(authority.action.request(), &current.resource)?;
        let decision = validate_current_authority(authority.action.request(), &current.policy)?;
        // Lock the exact immutable intent after all authoritative owners. No caller
        // observation or stored marker can substitute for these current checks.
        let intent: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 \
             AND execution_id=$3 AND id=$4 AND argument_digest=$5 FOR UPDATE",
        )
        .bind(request.scope().company.as_uuid())
        .bind(request.scope().run.as_uuid())
        .bind(request.scope().execution.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str())
        .fetch_optional(&mut **tx)
        .await?;
        if intent.is_none() {
            return Err(missing());
        }
        let mut policy = json!({"tool":current.policy.tool,"approval_required":current.policy.approval_required});
        let approved = decisions::approvals::approved(
            tx,
            request.fence.scope,
            &request.subject,
            authority.action.operation_digest().as_str(),
        )
        .await?;
        if let Some(id) = approved {
            policy["approval_required"] = json!(true);
            policy["approval_decision"] = json!(id);
        }
        Ok(AuthorizedAction {
            action: authority.action,
            current_policy: policy,
            approval_required: decision == ActionAccessDecision::ApprovalRequired
                && approved.is_none(),
        })
    }

    pub(super) async fn existing(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
    ) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND invocation_id=$2)",
        ).bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
            .fetch_one(&mut **tx).await?)
    }

    pub(super) async fn insert(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        kind: &str,
        policy: Value,
    ) -> AppResult<()> {
        Self::insert_subject(tx, request, marker, kind, policy, None).await
    }

    pub(super) async fn insert_subject(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        marker: Uuid,
        kind: &str,
        policy: Value,
        subject: Option<Vec<u8>>,
    ) -> AppResult<()> {
        sqlx::query("INSERT INTO workflow_action_dispatches (company_id,run_id,execution_id,invocation_id,argument_digest,id,job_id,attempt_number,execution_generation,worker_id,effect_kind,current_policy,replay_subject) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
            .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str()).bind(marker)
            .bind(request.fence.scope.job.0).bind(request.fence.attempt.0)
            .bind(request.fence.generation.0).bind(request.fence.worker.0)
            .bind(kind).bind(policy).bind(subject).execute(&mut **tx).await?;
        Ok(())
    }
}

#[async_trait]
impl<A: SqlActionAuthority, L: SqlLocalAction> ActionDispatch for PostgresActionDispatch<A, L> {
    async fn local(&self, request: &ActionDispatchRequest) -> AppResult<LocalDispatchResult> {
        let bound = WriteBound::start("action.local", request.lease);
        let attempt = bound.attempt(Box::pin(async {
            let started = bound.started();
            let mut conn = bound.acquire(self.persistence.pool()).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let dispatched: AppResult<End<_>> = async {
                let AuthorizedAction { action, current_policy: policy, approval_required: protected } = self.authorize_on(&mut tx, request, bound.server_timeout()).await?;
                token_budget::require_progression(&mut tx, request.fence.scope).await?;
                if protected {
                    return Ok(match decisions::approvals::park(&mut tx, request, &action, self.decision_notices.as_ref()).await? {
                        Some(wait) => End::Commit(LocalDispatchResult::ApprovalParked(wait)),
                        None => End::Rollback(LocalDispatchResult::ApprovalRequired),
                    });
                }
                if action_uncertainty::conflicted_on(&mut tx, request.scope()).await? { return Err(conflict()); }
                if Self::existing(&mut tx, request).await? {
                    let saved: Option<Value> = saved_receipt(&mut tx, request).await?;
                    return Ok(End::Rollback(match saved {
                        Some(result) => LocalDispatchResult::Committed(ActionReceipt { subject: request.subject.clone(), result: validate_action_result(&action, result)? }),
                        None => LocalDispatchResult::PossibleDispatchExists,
                    }));
                }
                let marker = Uuid::new_v4();
                Self::insert(&mut tx, request, marker, "local", policy).await?;
                // Box the local adapter seam to keep debug/test stacks bounded.
                let result = validate_action_result(&action, Box::pin(self.local.apply(&mut tx, &action)).await?)?;
                sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,'local',$7)")
                    .bind(request.scope().company.as_uuid()).bind(request.scope().run.as_uuid())
                    .bind(request.scope().execution.as_uuid()).bind(request.subject.invocation.as_uuid())
                    .bind(request.subject.argument_digest.as_str()).bind(marker).bind(&result).execute(&mut *tx).await?;
                lease::live_window(&mut tx, request.fence, started).await?.ok_or_else(conflict)?;
                Ok(End::Commit(LocalDispatchResult::Committed(ActionReceipt { subject: request.subject.clone(), result })))
            }
            .await;
            bound.end(tx, dispatched).await
        })).await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // The effect, its marker and its receipt share one transaction: the receipt
            // row is the whole evidence, and the effect ran at most once.
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(async {
                        let mut conn = bound.acquire(self.persistence.pool()).await?;
                        let mut tx = bound.begin(&mut conn).await?;
                        let saved = async {
                            lease::lock_scope(&mut tx, request.fence.scope, bound.server_timeout())
                                .await?;
                            if let Some(wait) =
                                decisions::approvals::parked(&mut tx, request).await?
                            {
                                return Ok(Some(LocalDispatchResult::ApprovalParked(wait)));
                            }
                            Ok(saved_receipt(&mut tx, request).await?.map(|result| {
                                LocalDispatchResult::Committed(ActionReceipt {
                                    subject: request.subject.clone(),
                                    result,
                                })
                            }))
                        }
                        .await;
                        let saved = bound.roll_back(tx, saved).await?;
                        // Stored only after validate_action_result, so it is already canonical.
                        Ok(saved.map_or(Evidence::NotApplied, Evidence::Applied))
                    }))
                    .await
            }
        }
    }

    async fn remote(
        &self,
        request: &ActionDispatchRequest,
        provider: &dyn RemoteAction,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> AppResult<RemoteDispatchObservation> {
        supervise_remote(self, request, provider, cancel).await
    }
}

/// Who a remote entry was reserved for: its marker and the fence it was reserved under.
#[derive(sqlx::FromRow)]
struct EntryOwner {
    dispatch_id: Uuid,
    invocation_id: Uuid,
    job_id: Uuid,
    attempt_number: i32,
    execution_generation: Uuid,
    worker_id: Uuid,
}
impl EntryOwner {
    /// Exactly the entry this call reserved, under its marker and fence.
    fn is(&self, request: &ActionDispatchRequest, marker: Option<Uuid>) -> bool {
        let fence = request.fence;
        marker == Some(self.dispatch_id)
            && self.invocation_id == request.subject.invocation.as_uuid()
            && self.job_id == fence.scope.job.0
            && self.attempt_number == fence.attempt.0
            && self.execution_generation == fence.generation.0
            && self.worker_id == fence.worker.0
    }
}

/// The reservation this call built, held while its COMMIT may be in flight. Exactly one of
/// the acknowledged COMMIT or the resolution takes it, so a call yields at most one.
type PendingReservation = Mutex<Option<RemoteReservation>>;

fn take(pending: &PendingReservation) -> Option<RemoteReservation> {
    pending.lock().ok().and_then(|mut pending| pending.take())
}

/// Adopts our reservation with the earlier of the window read before its COMMIT and the one
/// read under the resolution's locks, and only while that window is still open.
fn adopt((mut reservation, fresh): (RemoteReservation, LeaseWindow)) -> Option<RemoteReservation> {
    let window = LeaseWindow {
        expires: reservation.window.expires.min(fresh.expires),
        run_deadline: reservation.window.run_deadline.min(fresh.run_deadline),
    };
    (Instant::now() < window.expires.min(window.run_deadline)).then(|| {
        reservation.window = window;
        reservation
    })
}

impl<A: SqlActionAuthority, L: SqlLocalAction> PostgresActionDispatch<A, L> {
    /// The ambiguous COMMIT of a reservation. Under the original's own locks, our entry's
    /// absence proves the reservation aborted: a definite, retry-safe NotApplied. Its presence
    /// proves our COMMIT, so `stashed` is the same permit an acknowledged COMMIT returns,
    /// adopted while its fence still holds; enter_remote re-checks that fence before any
    /// provider. Otherwise it stays Unknown and the next owner sees PossibleDispatchExists.
    async fn resolve_reservation(
        &self,
        bound: &WriteBound,
        request: &ActionDispatchRequest,
        entry: Uuid,
        stashed: Option<RemoteReservation>,
    ) -> AppResult<RemoteReservationResult> {
        let marker = stashed.as_ref().map(|reservation| reservation.marker);
        let resolution = self.reserved_on(bound, request, entry, marker);
        match bound.resolve_retryable(Box::pin(resolution)).await? {
            None => Err(bound.not_applied()),
            Some(ReservationEvidence::Approval(wait)) => {
                Ok(RemoteReservationResult::ApprovalParked(wait))
            }
            Some(ReservationEvidence::Entry(fresh)) => match stashed.zip(fresh).and_then(adopt) {
                Some(reservation) => {
                    bound.adopted();
                    Ok(RemoteReservationResult::Reserved(Box::new(reservation)))
                }
                None => Err(bound.lapsed()),
            },
        }
    }

    /// Resolution of an ambiguous reservation, read-only: the original's run, execution and
    /// job locks and the fence predicate first, then the remote entry with the id minted
    /// before its timer, then our window. Absent ⇒ `NotApplied`; exactly ours under a fence
    /// that still holds ⇒ `Applied(Some(window))`; otherwise `Applied(None)`.
    async fn reserved_on(
        &self,
        bound: &WriteBound,
        request: &ActionDispatchRequest,
        entry: Uuid,
        marker: Option<Uuid>,
    ) -> AppResult<Evidence<ReservationEvidence>> {
        let observed = Instant::now();
        let mut conn = bound.acquire(self.persistence.pool()).await?;
        let mut tx = bound.begin(&mut conn).await?;
        let evidence = async {
            let fenced =
                lease::lock_fence(&mut tx, request.fence, bound.server_timeout()).await?;
            if let Some(wait) = decisions::approvals::parked(&mut tx, request).await? {
                return Ok(Evidence::Applied(ReservationEvidence::Approval(wait)));
            }
            let owner: Option<EntryOwner> = sqlx::query_as(
                "SELECT dispatch_id, invocation_id, job_id, attempt_number, execution_generation, worker_id \
                 FROM workflow_action_remote_entries WHERE company_id=$1 AND id=$2",
            )
            .bind(request.scope().company.as_uuid())
            .bind(entry)
            .fetch_optional(&mut *tx)
            .await?;
            Ok(match owner {
                None => Evidence::NotApplied,
                Some(owner) if fenced && owner.is(request, marker) => {
                    Evidence::Applied(ReservationEvidence::Entry(lease::live_window(&mut tx, request.fence, observed).await?))
                }
                Some(_) => Evidence::Applied(ReservationEvidence::Entry(None)),
            })
        }
        .await;
        bound.roll_back(tx, evidence).await
    }
}

/// How a reservation attempt ended its transaction, before the result is built from it.
enum ReservationEvidence {
    Approval(crate::domain::workflow::WaitId),
    Entry(Option<LeaseWindow>),
}

enum Reserving {
    Approval(crate::domain::workflow::WaitId),
    /// A receipt already exists: rolled back, then checked against the window read before.
    Receipt(ActionReceipt, LeaseWindow),
    /// Rolled back with nothing written.
    Refused(RemoteReservationResult),
    /// COMMIT sent: the reservation waits in `pending`.
    Reserved,
}

impl<A: SqlActionAuthority, L: SqlLocalAction> PostgresActionDispatch<A, L> {
    async fn reserve_action(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
        bound: &WriteBound,
        entry: Uuid,
        pending: &PendingReservation,
    ) -> AppResult<End<Reserving>> {
        let AuthorizedAction {
            action,
            current_policy: mut policy,
            approval_required: protected,
        } = self
            .authorize_on(tx, request, bound.server_timeout())
            .await?;
        token_budget::require_progression(tx, request.fence.scope).await?;
        if protected {
            return Ok(
                match decisions::approvals::park(
                    tx,
                    request,
                    &action,
                    self.decision_notices.as_ref(),
                )
                .await?
                {
                    Some(wait) => End::Commit(Reserving::Approval(wait)),
                    None => End::Rollback(Reserving::Refused(
                        RemoteReservationResult::ApprovalRequired,
                    )),
                },
            );
        }
        if action_uncertainty::conflicted_on(tx, request.scope()).await? {
            return Err(conflict());
        }
        if let Some(receipt) = self.receipt_on(tx, request, &action).await? {
            let window = lease::live_window(tx, request.fence, bound.started())
                .await?
                .ok_or_else(conflict)?;
            // This branch wrote nothing: ROLLBACK, so a stall cannot leave it
            // ambiguous (and fail closed for no reason).
            return Ok(End::Rollback(Reserving::Receipt(receipt, window)));
        }
        if decisions::approvals::consumed(tx, request).await? {
            return Ok(End::Rollback(Reserving::Refused(
                RemoteReservationResult::ApprovalRequired,
            )));
        }
        let window = lease::live_window(tx, request.fence, bound.started())
            .await?
            .ok_or_else(conflict)?;
        let provider = self.provider_on(tx, request, &action, contract).await?;
        policy["provider_replay"] = provider.proof()?;
        let Some(marker) = self
            .marker_on(tx, request, &action, policy, &provider)
            .await?
        else {
            return Ok(End::Rollback(Reserving::Refused(
                RemoteReservationResult::PossibleDispatchExists,
            )));
        };
        if !Self::reserve_entry_on(tx, request, marker, entry).await? {
            return Ok(End::Rollback(Reserving::Refused(
                RemoteReservationResult::PossibleDispatchExists,
            )));
        }
        // Held before COMMIT is sent, so an ambiguous acknowledgement can still hand
        // back this same reservation once the resolution proves it committed.
        *pending.lock().map_err(|_| conflict())? = Some(RemoteReservation {
            marker,
            entry,
            action,
            request: request.clone(),
            window,
            provider,
        });
        Ok(End::Commit(Reserving::Reserved))
    }
}

#[async_trait]
impl<A: SqlActionAuthority, L: SqlLocalAction> RemoteDispatch for PostgresActionDispatch<A, L> {
    async fn reserve_remote(
        &self,
        request: &ActionDispatchRequest,
        contract: Option<&ProviderReplayContract>,
    ) -> AppResult<RemoteReservationResult> {
        // Minted before the timer: the only COMMIT this operation sends inserts the remote
        // entry with exactly this id, which is what an ambiguous outcome is resolved by.
        let entry = Uuid::new_v4();
        let pending = PendingReservation::default();
        let bound = WriteBound::start("action.reserve_remote", request.lease);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                // Box the SQL phase so approval composition does not enlarge the supervisor frame.
                let reserving = Box::pin(
                    self.reserve_action(&mut tx, request, contract, &bound, entry, &pending),
                )
                .await;
                match bound.end(tx, reserving).await? {
                    Reserving::Receipt(receipt, window) => {
                        if Instant::now() >= window.expires.min(window.run_deadline) {
                            return Err(conflict());
                        }
                        Ok(RemoteReservationResult::Committed(receipt))
                    }
                    Reserving::Refused(result) => Ok(result),
                    Reserving::Approval(wait) => Ok(RemoteReservationResult::ApprovalParked(wait)),
                    // Unreachable when empty: only the resolution also takes it, and it runs
                    // only when this COMMIT's outcome is ambiguous.
                    Reserving::Reserved => {
                        let reservation = take(&pending).ok_or_else(|| bound.unresolved())?;
                        Ok(RemoteReservationResult::Reserved(Box::new(reservation)))
                    }
                }
            }))
            .await;
        match attempt {
            Attempt::Done(result) => Ok(result),
            Attempt::Failed(error) => Err(error),
            // Boxed: the resolution's state stays off this frame (the dispatch chain runs at
            // stock 2 MiB in stack-budget.sh).
            Attempt::Ambiguous => {
                Box::pin(self.resolve_reservation(&bound, request, entry, take(&pending))).await
            }
        }
    }

    async fn enter_remote(&self, reservation: RemoteReservation) -> AppResult<RemoteEntry> {
        let request = &reservation.request;
        let bound = WriteBound::read_only("action.enter_remote", request.lease);
        let window = bound.read(Box::pin(async {
            let started = bound.started();
            let mut conn = bound.acquire(self.persistence.pool()).await?;
            let mut tx = bound.begin(&mut conn).await?;
            let window = async {
                let AuthorizedAction {
                    approval_required: protected,
                    ..
                } = self.authorize_on(&mut tx, request, bound.server_timeout()).await?;
                token_budget::require_progression(&mut tx, request.fence.scope).await?;
                if protected {
                    return Err(missing());
                }
                Self::verify_entry_on(
                    &mut tx,
                    request,
                    reservation.marker,
                    reservation.entry,
                    &reservation.action,
                    &reservation.provider,
                )
                .await?;
                let allowed: bool = sqlx::query_scalar("SELECT NOT workflow_action_has_evidence_conflict($1,$2) AND NOT EXISTS(SELECT 1 FROM workflow_action_evidence WHERE company_id=$1 AND invocation_id=$3 AND disposition='applied') AND (NOT EXISTS(SELECT 1 FROM workflow_action_evidence_consumptions WHERE company_id=$1 AND remote_entry_id=$4) OR workflow_action_not_applied_available($1,$3,$4) IS NOT NULL)")
                    .bind(request.scope().company.as_uuid()).bind(request.scope().execution.as_uuid())
                    .bind(request.subject.invocation.as_uuid()).bind(reservation.entry).fetch_one(&mut *tx).await?;
                if !allowed { return Err(conflict()); }
                lease::live_window(&mut tx, request.fence, started)
                    .await?
                    .ok_or_else(conflict)
            }
            .await;
            // Read-only: ROLLBACK needs no synchronous flush and leaves nothing ambiguous.
            bound.roll_back(tx, window).await
        }))
        .await?;
        Ok(RemoteEntry {
            marker: reservation.marker,
            entry: reservation.entry,
            request: reservation.request,
            action: reservation.action,
            provider: reservation.provider,
            deadline: window
                .expires
                .min(window.run_deadline)
                .min(reservation.window.expires)
                .min(reservation.window.run_deadline),
        })
    }

    async fn finish_remote(
        &self,
        entry: RemoteEntry,
        result: Value,
    ) -> AppResult<RemoteDispatchObservation> {
        self.commit_remote(entry, result).await
    }

    async fn owns_remote(&self, request: &ActionDispatchRequest) -> AppResult<bool> {
        let bound = WriteBound::read_only("action.owns_remote", request.lease);
        bound
            .read(Box::pin(async {
                let mut conn = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let live = async {
                    Ok(
                        lease::lock_fence(&mut tx, request.fence, bound.server_timeout()).await?
                            && !action_uncertainty::conflicted_on(&mut tx, request.scope()).await?
                            && lease::live_window(&mut tx, request.fence, Instant::now())
                                .await?
                                .is_some(),
                    )
                }
                .await;
                bound.roll_back(tx, live).await
            }))
            .await
    }
}

async fn saved_receipt(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
) -> AppResult<Option<Value>> {
    Ok(sqlx::query_scalar("SELECT result FROM workflow_action_receipts WHERE company_id=$1 AND invocation_id=$2 AND argument_digest=$3")
        .bind(request.scope().company.as_uuid()).bind(request.subject.invocation.as_uuid())
        .bind(request.subject.argument_digest.as_str()).fetch_optional(&mut **tx).await?)
}
