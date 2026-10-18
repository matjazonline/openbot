//! The message publication transaction (04.11-D2 §2, D3 §A/§B, D4 F014.2).
//!
//! Order: `authorize_on` locks (company/actor, fence, run, current owners, intent), conflict
//! and replay checks, destination-owner locks in UUID order, the admission probe, then the
//! `message` marker as the first write, the action row, every thread/message/delivery and linkage row, the
//! atomic receipt (all in-app) or the effect park (a step with email). No provider I/O.
use super::super::action_dispatch::{AuthorizedAction, PostgresActionDispatch, SqlLocalAction};
use super::super::persistence_bound::{Attempt, End, Evidence, WriteBound};
use super::*;
use crate::adapters::persistence::delivery::enqueue::insert_delivery_on;
use crate::adapters::persistence::thread::insert_message_on;
use crate::application::transport::queue::DeliveryCreation;
use crate::application::workflow::actions::validate_action_result;

/// Who is asking for the stored state: a fresh observation or an ambiguous-COMMIT resolution
/// that may be finding its own park.
#[derive(Clone, Copy)]
enum Reader {
    Observe,
    Resolve,
}

#[derive(sqlx::FromRow)]
struct StoredAction {
    state: String,
    origin: String,
    wait_id: Option<Uuid>,
    result: Option<Value>,
}

async fn stored_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    action: &FrozenAction,
    reader: Reader,
) -> AppResult<MessagePublication> {
    let stored: Option<StoredAction> = sqlx::query_as(
        "SELECT action.state, action.origin, action.wait_id, receipt.result FROM workflow_message_actions AS action \
         LEFT JOIN workflow_action_receipts AS receipt ON receipt.company_id=action.company_id AND receipt.invocation_id=action.invocation_id \
         WHERE action.company_id=$1 AND action.invocation_id=$2 AND action.argument_digest=$3",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .bind(request.subject.argument_digest.as_str())
    .fetch_optional(&mut **tx)
    .await?;
    let Some(stored) = stored else {
        return Ok(MessagePublication::PossibleDispatchExists);
    };
    if let Some(result) = stored.result {
        return Ok(MessagePublication::Committed(ActionReceipt {
            subject: request.subject.clone(),
            result: validate_action_result(action, result)?,
        }));
    }
    let step = stored.origin == "step";
    Ok(
        match (stored.state.as_str(), step, reader, stored.wait_id) {
            ("pending", true, Reader::Resolve, Some(wait)) => {
                MessagePublication::Suspended(WaitId::new(wait))
            }
            (_, true, _, _) => MessagePublication::PossibleDispatchExists,
            ("pending", false, _, _) => MessagePublication::Pending,
            _ => MessagePublication::Uncertain,
        },
    )
}

/// Destination owners locked in one deterministic order, then compared with the plan.
async fn recheck_routes_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    plan: &MessagePlan,
) -> AppResult<()> {
    let mut checks: Vec<&RouteCheck> = plan.destinations.iter().map(|d| &d.check).collect();
    checks.sort_by_key(|check| match check {
        RouteCheck::Channel { channel } => (0, *channel),
        RouteCheck::Member { user, .. } => (1, *user),
        RouteCheck::Source { source, .. } => (2, source.as_uuid()),
    });
    for check in checks {
        let current = match check {
            RouteCheck::Channel { channel } => channel_exists_on(tx, company, *channel).await?,
            RouteCheck::Member {
                user,
                principal,
                email,
                sender,
            } => {
                channel_exists_on(tx, company, *sender).await?
                    && member_on(tx, company, *user, true)
                        .await?
                        .is_some_and(|member| {
                            member.principal == *principal && member.email == *email
                        })
            }
            RouteCheck::Source {
                source,
                thread,
                channel,
            } => sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM thread_messages WHERE company_id=$1 AND message_id=$2 \
                   AND thread_id=$3 AND channel_id=$4 FOR KEY SHARE)",
            )
            .bind(company.as_uuid())
            .bind(source.as_uuid())
            .bind(thread)
            .bind(channel)
            .fetch_one(&mut **tx)
            .await?,
        };
        if !current {
            return Err(AppError::BadRequest(
                "Workflow message destination changed before publication".into(),
            ));
        }
    }
    Ok(())
}

async fn channel_exists_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    channel: Uuid,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM channels WHERE company_id=$1 AND id=$2 FOR KEY SHARE",
    )
    .bind(company.as_uuid())
    .bind(channel)
    .fetch_optional(&mut **tx)
    .await?
    .is_some())
}

async fn admission_full_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
) -> AppResult<bool> {
    let held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT 1 FROM workflow_action_dispatches WHERE company_id=$1 AND execution_id=$2 LIMIT 128) AS bounded",
    )
    .bind(request.scope().company.as_uuid())
    .bind(request.scope().execution.as_uuid())
    .fetch_one(&mut **tx)
    .await?;
    Ok(usize::try_from(held).map_err(|_| invalid())? >= MAX_MESSAGE_ACTIONS_PER_EXECUTION)
}

/// Every thread, canonical message and delivery of the plan, in destination order.
async fn write_destinations_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    plan: &MessagePlan,
) -> AppResult<()> {
    let scope = request.scope();
    for (ordinal, destination) in plan.destinations.iter().enumerate() {
        if let Some(thread) = &destination.new_thread {
            sqlx::query(
                "INSERT INTO threads (id, company_id, channel_id, subject) VALUES ($1,$2,$3,$4)",
            )
            .bind(thread.id)
            .bind(scope.company.as_uuid())
            .bind(thread.channel_id)
            .bind(&thread.subject)
            .execute(&mut **tx)
            .await?;
        }
        let inserted = insert_message_on(tx, &destination.message).await?;
        if inserted.canonical_id != destination.message.id {
            return Err(AppError::Conflict(
                "Workflow message identity already belongs to another message".into(),
            ));
        }
        let delivery = match &destination.delivery {
            Some(delivery) => match insert_delivery_on(tx, delivery).await? {
                DeliveryCreation::Created(id) if id == delivery.id => Some(id.as_uuid()),
                _ => {
                    return Err(AppError::Conflict(
                        "Workflow message delivery key already belongs to another delivery".into(),
                    ));
                }
            },
            None => None,
        };
        sqlx::query(
            "INSERT INTO workflow_message_destinations (company_id,run_id,invocation_id,ordinal,kind,destination_id,thread_id,message_id,delivery_id,principal_id,recipient_address) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .bind(request.subject.invocation.as_uuid())
        .bind(i16::try_from(ordinal).map_err(|_| invalid())?)
        .bind(destination.kind.as_str())
        .bind(destination.destination_id)
        .bind(destination.thread_id)
        .bind(destination.message.id.as_uuid())
        .bind(delivery)
        .bind(destination.principal_id)
        .bind(destination.recipient.as_ref().map(|address| address.as_str().to_string()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_action_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    marker: Uuid,
    origin: MessageOrigin,
    wait: Option<Uuid>,
    destinations: usize,
) -> AppResult<()> {
    let scope = request.scope();
    sqlx::query(
        "INSERT INTO workflow_message_actions (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,origin,wait_id,destination_count) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(scope.execution.as_uuid())
    .bind(request.subject.invocation.as_uuid())
    .bind(request.subject.argument_digest.as_str())
    .bind(marker)
    .bind(origin.as_str())
    .bind(wait)
    .bind(i16::try_from(destinations).map_err(|_| invalid())?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The all-in-app receipt, accepted at this commit; SQL recomputes and compares it.
async fn accept_in_app_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    marker: Uuid,
    result: &Value,
) -> AppResult<()> {
    let scope = request.scope();
    sqlx::query("INSERT INTO workflow_action_receipts (company_id,run_id,execution_id,invocation_id,argument_digest,dispatch_id,effect_kind,result) VALUES ($1,$2,$3,$4,$5,$6,'message',$7)")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid())
        .bind(request.subject.invocation.as_uuid()).bind(request.subject.argument_digest.as_str())
        .bind(marker).bind(result).execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_message_actions SET state='accepted',updated_at=clock_timestamp() WHERE company_id=$1 AND invocation_id=$2")
        .bind(scope.company.as_uuid()).bind(request.subject.invocation.as_uuid()).execute(&mut **tx).await?;
    Ok(())
}

/// Park the step in its effect wait and close the exact fenced attempt and job (EffectParked).
async fn park_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &ActionDispatchRequest,
    wait: Uuid,
) -> AppResult<()> {
    let fence = request.fence;
    let scope = fence.scope;
    sqlx::query("INSERT INTO workflow_waits (company_id,run_id,execution_id,id,reason,deadline,created_at) SELECT company_id,id,$3,$4,'effect',deadline,clock_timestamp() FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(wait)
        .execute(&mut **tx).await?;
    let closed = sqlx::query("UPDATE task_attempts SET status='completed',stop_reason='effect_parked',finished_at=clock_timestamp() WHERE task_id=$1 AND attempt_number=$2 AND worker_id=$3 AND execution_generation=$4 AND status='processing'")
        .bind(scope.job.0).bind(fence.attempt.0).bind(fence.worker.0).bind(fence.generation.0)
        .execute(&mut **tx).await?;
    let job = sqlx::query("UPDATE background_tasks SET status='completed',updated_at=clock_timestamp(),worker_id=NULL,execution_generation=NULL,locked_at=NULL,lock_expires_at=NULL WHERE company_id=$1 AND id=$2 AND workflow_execution_id=$3 AND queue_kind='workflow' AND status='processing' AND worker_id=$4 AND execution_generation=$5 AND retry_count+1=$6 AND lock_expires_at>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid())
        .bind(fence.worker.0).bind(fence.generation.0).bind(fence.attempt.0)
        .execute(&mut **tx).await?;
    if closed.rows_affected() != 1 || job.rows_affected() != 1 {
        return Err(super::super::persistence_bound::refused("message park"));
    }
    sqlx::query("UPDATE workflow_runs SET state='waiting',waiting_reason='effect' WHERE company_id=$1 AND id=$2 AND state IN ('queued','running')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).execute(&mut **tx).await?;
    super::super::waits::audit(tx, scope, "wait_parked").await?;
    Ok(())
}

impl<L: SqlLocalAction> PostgresActionDispatch<MessageActionAuthority, L> {
    async fn publish_on(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
        plan: &MessagePlan,
        bound: &WriteBound,
    ) -> AppResult<End<MessagePublication>> {
        let started = bound.started();
        let AuthorizedAction {
            action,
            current_policy,
            approval_required,
        } = match self.authorize_on(tx, request, bound.server_timeout()).await {
            Ok(authorized) => authorized,
            Err(error) => {
                return Self::rejected_on(tx, request, error)
                    .await
                    .map(End::Rollback);
            }
        };
        if approval_required {
            return Ok(End::Rollback(MessagePublication::ApprovalRequired));
        }
        // Remote dispatch refuses new work in a conflicted execution; messaging matches (F014.2).
        if super::super::action_uncertainty::conflicted_on(tx, request.scope()).await? {
            return Err(conflict());
        }
        if Self::existing(tx, request).await? {
            return Ok(End::Rollback(
                stored_on(tx, request, &action, Reader::Observe).await?,
            ));
        }
        if let Err(error) = recheck_routes_on(tx, request.scope().company, plan).await {
            return Self::rejected_on(tx, request, error)
                .await
                .map(End::Rollback);
        }
        super::super::persistence_bound::locks_held();
        if admission_full_on(tx, request).await? {
            return Ok(End::Rollback(MessagePublication::ActionLimit));
        }
        let marker = Uuid::new_v4();
        // The probe above holds the attempt lock, so the SQL admission backstop cannot fire here;
        // it exists for SQL-only writers.
        Self::insert(tx, request, marker, "message", current_policy).await?;
        let wait = (origin == MessageOrigin::Step && plan.has_delivery()).then(Uuid::new_v4);
        insert_action_on(tx, request, marker, origin, wait, plan.destinations.len()).await?;
        write_destinations_on(tx, request, plan).await?;
        lease::live_window(tx, request.fence, started)
            .await?
            .ok_or_else(conflict)?;
        if !plan.has_delivery() {
            let result = validate_action_result(&action, plan.accepted_result())?;
            accept_in_app_on(tx, request, marker, &result).await?;
            return Ok(End::Commit(MessagePublication::Committed(ActionReceipt {
                subject: request.subject.clone(),
                result,
            })));
        }
        if let Some(wait) = wait {
            park_on(tx, request, wait).await?;
            return Ok(End::Commit(MessagePublication::Suspended(WaitId::new(
                wait,
            ))));
        }
        Ok(End::Commit(MessagePublication::Pending))
    }

    /// D2 §5.1 row 2: a routing or authority refusal while this invocation has no marker is
    /// Terminal `message.rejected` (the transaction rolls back, nothing was written). With a
    /// marker, or for any other error, the error stays a retryable dispatch failure.
    async fn rejected_on(
        tx: &mut Transaction<'_, Postgres>,
        request: &ActionDispatchRequest,
        error: AppError,
    ) -> AppResult<MessagePublication> {
        let refusal = matches!(
            error,
            AppError::NotFound(_) | AppError::BadRequest(_) | AppError::InvalidCredentials
        );
        if refusal && !Self::existing(tx, request).await? {
            return Ok(MessagePublication::Rejected(error));
        }
        Err(error)
    }

    async fn resolve_on(
        &self,
        request: &ActionDispatchRequest,
        bound: &WriteBound,
    ) -> AppResult<Evidence<MessagePublication>> {
        let mut conn = bound.acquire(self.persistence.pool()).await?;
        let mut tx = bound.begin(&mut conn).await?;
        let stored = async {
            lease::lock_scope(&mut tx, request.fence.scope, bound.server_timeout()).await?;
            if !Self::existing(&mut tx, request).await? {
                return Ok(None);
            }
            // Read-only evidence of this operation: the frozen intent itself, whatever state the
            // park left the run in (a resolution never re-authorizes or writes).
            let operation: Value = sqlx::query_scalar(
                "SELECT operation FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2 \
                   AND execution_id=$3 AND id=$4 AND argument_digest=$5",
            )
            .bind(request.scope().company.as_uuid())
            .bind(request.scope().run.as_uuid())
            .bind(request.scope().execution.as_uuid())
            .bind(request.subject.invocation.as_uuid())
            .bind(request.subject.argument_digest.as_str())
            .fetch_one(&mut *tx)
            .await?;
            let action = FrozenAction::restore(operation)?;
            Ok(Some(
                stored_on(&mut tx, request, &action, Reader::Resolve).await?,
            ))
        }
        .await;
        let stored = bound.roll_back(tx, stored).await?;
        Ok(stored.map_or(Evidence::NotApplied, Evidence::Applied))
    }
}

#[async_trait]
impl<L: SqlLocalAction> MessageDispatch for PostgresActionDispatch<MessageActionAuthority, L> {
    async fn observe(
        &self,
        request: &ActionDispatchRequest,
        _origin: MessageOrigin,
    ) -> AppResult<Option<MessagePublication>> {
        let bound = WriteBound::read_only("message.observe", request.lease);
        bound
            .read(Box::pin(async {
                let mut conn = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let observed = async {
                    let authorized = match self
                        .authorize_on(&mut tx, request, bound.server_timeout())
                        .await
                    {
                        Ok(authorized) => authorized,
                        Err(error) => {
                            return Self::rejected_on(&mut tx, request, error).await.map(Some);
                        }
                    };
                    if super::super::action_uncertainty::conflicted_on(&mut tx, request.scope())
                        .await?
                    {
                        return Err(conflict());
                    }
                    if !Self::existing(&mut tx, request).await? {
                        return Ok(None);
                    }
                    if authorized.approval_required {
                        return Ok(Some(MessagePublication::ApprovalRequired));
                    }
                    Ok(Some(
                        stored_on(&mut tx, request, &authorized.action, Reader::Observe).await?,
                    ))
                }
                .await;
                bound.roll_back(tx, observed).await
            }))
            .await
    }

    async fn publish(
        &self,
        request: &ActionDispatchRequest,
        origin: MessageOrigin,
        plan: &MessagePlan,
    ) -> AppResult<MessagePublication> {
        let bound = WriteBound::start("message.publish", request.lease);
        let attempt = bound
            .attempt(Box::pin(async {
                let mut conn = bound.acquire(self.persistence.pool()).await?;
                let mut tx = bound.begin(&mut conn).await?;
                let published =
                    Box::pin(self.publish_on(&mut tx, request, origin, plan, &bound)).await;
                bound.end(tx, published).await
            }))
            .await;
        match attempt {
            Attempt::Done(publication) => Ok(publication),
            Attempt::Failed(error) => Err(error),
            // Marker, linkage and park share one transaction: the marker is the whole evidence.
            Attempt::Ambiguous => {
                bound
                    .resolve(Box::pin(self.resolve_on(request, &bound)))
                    .await
            }
        }
    }
}
