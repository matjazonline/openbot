use super::*;
use crate::application::workflow::worker::WorkflowHandler;
use crate::application::workflow::{
    RetryCommand, RetryResult,
    lease::{LeaseReleaseCause, WorkflowFailure},
};
use crate::domain::workflow::{FailureClass, FailureCode, RetrySafety, StepFailure};

async fn populate(f: &HistoryThread, count: usize) -> ClaimedWorkflow {
    let owner = f.next().await;
    f.s.p()
        .prepare_agent(&owner, policy())
        .await
        .unwrap()
        .expect_ready();
    // A real local interruption returns the worker's lease without releasing its logical turn.
    assert!(
        f.s.p()
            .release_io(owner.fence, policy(), LeaseReleaseCause::LocalInterruption)
            .await
            .unwrap()
    );
    for _ in 1..count {
        let claim = f.next().await;
        parked(f, &claim).await;
    }
    owner
}

async fn counts(f: &HistoryThread) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('conversations',(SELECT count(*) FROM agent_history_conversations WHERE company_id=$1),'exchanges',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1),'last',(SELECT sum(last_exchange_order) FROM agent_history_conversations WHERE company_id=$1),'pending',(SELECT count(*) FROM agent_history_exchanges WHERE company_id=$1 AND state IN ('queued','active'))) ")
        .bind(f.s.fixture.target.company.as_uuid()).fetch_one(f.s.pool()).await.unwrap()
}

pub(super) async fn new_thread(f: &mut HistoryThread) {
    f.s.thread =
        f.s.p()
            .create_thread(
                f.s.channel,
                "Independent",
                &[EmailAddress::from("customer@partner.test")],
            )
            .await
            .unwrap()
            .id;
}

#[tokio::test]
async fn agent_history_ordering_conversation_limit_refuses_atomically_then_admits_after_cancel() {
    let f = Box::pin(HistoryThread::new()).await;
    let owner = populate(&f, 31).await;
    let a = f.next().await;
    let b = f.next().await;
    let (left, right) = tokio::join!(
        f.s.p().prepare_agent(&a, policy()),
        f.s.p().prepare_agent(&b, policy())
    );
    let extra = match (left.unwrap(), right.unwrap()) {
        (AgentPreparationResult::Parked(_), AgentPreparationResult::AtCapacity) => b,
        (AgentPreparationResult::AtCapacity, AgentPreparationResult::Parked(_)) => a,
        _ => panic!("exactly one competing conversation member may take the final slot"),
    };
    let before = counts(&f).await;
    assert_eq!(
        before,
        json!({"conversations":1,"exchanges":32,"last":32,"pending":32})
    );
    assert!(matches!(
        f.s.p().prepare_agent(&extra, policy()).await.unwrap(),
        AgentPreparationResult::AtCapacity
    ));
    assert_eq!(counts(&f).await, before);
    assert_eq!(
        pointer(&f).await,
        Some(owner.fence.scope.execution.as_uuid())
    );
    let frozen: bool = sqlx::query_scalar(
        "SELECT frozen_inputs ? 'context' FROM workflow_executions WHERE company_id=$1 AND id=$2",
    )
    .bind(extra.fence.scope.company.as_uuid())
    .bind(extra.fence.scope.execution.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert!(frozen);
    cancel(&f, &owner, "capacity-cancel-head").await;
    parked(&f, &extra).await;
    assert_eq!(
        counts(&f).await,
        json!({"conversations":1,"exchanges":33,"last":33,"pending":32})
    );
}

#[tokio::test]
async fn agent_history_ordering_company_limit_spans_streams_and_rolls_back_new_identity() {
    let mut f = Box::pin(HistoryThread::new()).await;
    for stream in 0..8 {
        if stream > 0 {
            new_thread(&mut f).await;
        }
        populate(&f, if stream == 7 { 31 } else { 32 }).await;
    }
    assert_eq!(
        counts(&f).await,
        json!({"conversations":8,"exchanges":255,"last":255,"pending":255})
    );
    new_thread(&mut f).await;
    let a = f.next().await;
    new_thread(&mut f).await;
    let b = f.next().await;
    let (left, right) = tokio::join!(
        f.s.p().prepare_agent(&a, policy()),
        f.s.p().prepare_agent(&b, policy())
    );
    let extra = match (left.unwrap(), right.unwrap()) {
        (AgentPreparationResult::Ready(_), AgentPreparationResult::AtCapacity) => b,
        (AgentPreparationResult::AtCapacity, AgentPreparationResult::Ready(_)) => a,
        _ => panic!("exactly one different-stream claimant may take the final tenant slot"),
    };
    assert_eq!(
        counts(&f).await,
        json!({"conversations":9,"exchanges":256,"last":256,"pending":256})
    );
    assert!(matches!(
        f.s.p().prepare_agent(&extra, policy()).await.unwrap(),
        AgentPreparationResult::AtCapacity
    ));
    assert_eq!(
        counts(&f).await,
        json!({"conversations":9,"exchanges":256,"last":256,"pending":256})
    );
    let outcome = Box::pin(runtime(&f).execute(
        &crate::application::workflow::polling::WorkflowStepKind("agent.run".into()),
        &extra,
    ))
    .await;
    use crate::adapters::persistence::workflow::tests::action_outcome_support::CompletedOutcome;
    let error = outcome.completed_result().unwrap_err();
    assert_eq!(
        error.failure.code(),
        &FailureCode::parse("agent.history_queue_full").unwrap()
    );
    let calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM task_harness_runs WHERE company_id=$1")
            .bind(extra.fence.scope.company.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    assert_eq!(calls, 0);
    native_refusal(&f, &extra).await;
}

async fn retry(f: &HistoryThread, claim: &ClaimedWorkflow, key: &str) -> AppResult<RetryResult> {
    let scope = claim.fence.scope;
    let head = f.s.p().head(scope.company, scope.run).await?.unwrap();
    f.s.p()
        .retry(RetryCommand {
            company_id: scope.company,
            run_id: scope.run,
            actor: f.s.owner(),
            command_key: IdempotencyKey::parse(key).unwrap(),
            expected_revision: head.revision,
        })
        .await
}

#[tokio::test]
async fn agent_history_ordering_safe_retry_at_capacity_is_atomic_and_old_position_cannot_steal_new_owner()
 {
    let f = Box::pin(HistoryThread::new()).await;
    let old = Box::pin(failed_with_checkpoint(&f)).await;
    let newer = populate(&f, 32).await;
    let before = counts(&f).await;
    let saved:Value=sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(old.fence.scope.company.as_uuid()).bind(old.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert!(matches!(
        retry(&f, &old, "retry-full").await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(counts(&f).await, before);
    let unchanged:Value=sqlx::query_scalar("SELECT to_jsonb(exchange) FROM agent_history_exchanges AS exchange WHERE company_id=$1 AND id=$2")
        .bind(old.fence.scope.company.as_uuid()).bind(old.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(unchanged, saved);
    // Remove another queued member, retaining the newer logical owner.
    let queued:Uuid=sqlx::query_scalar("SELECT run_id FROM agent_history_exchanges WHERE company_id=$1 AND state='queued' AND id<>$2 ORDER BY position DESC LIMIT 1")
        .bind(old.fence.scope.company.as_uuid()).bind(newer.fence.scope.execution.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    let head =
        f.s.p()
            .head(old.fence.scope.company, RunId::new(queued))
            .await
            .unwrap()
            .unwrap();
    assert!(matches!(
        f.s.p()
            .cancel(CancelCommand {
                company_id: old.fence.scope.company,
                run_id: RunId::new(queued),
                actor: f.s.owner(),
                command_key: IdempotencyKey::parse("cancel-capacity-member").unwrap(),
                expected_revision: head.revision,
            })
            .await
            .unwrap(),
        CancelResult::Applied { .. }
    ));
    assert!(matches!(
        retry(&f, &old, "retry-now-available").await.unwrap(),
        RetryResult::Applied { .. }
    ));
    assert_eq!(
        pointer(&f).await,
        Some(newer.fence.scope.execution.as_uuid())
    );
    let resumed =
        f.s.p()
            .claim_io(old.fence.scope, WorkflowWorkerId(Uuid::new_v4()), policy())
            .await
            .unwrap()
            .unwrap();
    parked(&f, &resumed).await;
    assert_eq!(
        pointer(&f).await,
        Some(newer.fence.scope.execution.as_uuid())
    );
    cancel(&f, &newer, "cancel-newer-owner").await;
    assert!(matches!(
        f.s.p().resume_wait(old.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(pointer(&f).await, Some(old.fence.scope.execution.as_uuid()));
}

async fn failed_with_checkpoint(f: &HistoryThread) -> ClaimedWorkflow {
    use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
    let old = f.next().await;
    let launch =
        f.s.p()
            .prepare_agent(&old, policy())
            .await
            .unwrap()
            .expect_ready();
    f.s.p()
        .open_workflow_checkpoint(
            old.fence,
            &launch.prepared.spec(old.fence).unwrap(),
            policy(),
        )
        .await
        .unwrap();
    let failure = WorkflowFailure {
        failure: StepFailure::new(
            FailureClass::Terminal,
            FailureCode::parse("provider.rejected").unwrap(),
            None,
        )
        .unwrap(),
        safety: RetrySafety::SafeToRetry,
    };
    assert!(
        f.s.p()
            .release_io(old.fence, policy(), LeaseReleaseCause::Classified(failure))
            .await
            .unwrap()
    );
    old
}

/// Skip the adapter's cap precheck using genuine native source/manifest/input facts.
/// The INSERT trigger must independently reject the full company before advancing order.
async fn native_refusal(f: &HistoryThread, claim: &ClaimedWorkflow) {
    use crate::application::workflow::agent_history::{
        AgentHistoryIdentity, ResolveAgentHistoryIdentity,
    };
    let identity =
        f.s.p()
            .resolve_agent_history_identity(ResolveAgentHistoryIdentity {
                fence: claim.fence,
                lease_policy: policy(),
            })
            .await
            .unwrap()
            .unwrap();
    let error=sqlx::query(
        "INSERT INTO agent_history_exchanges \
         (company_id,id,conversation_id,agent_id,revision_id,manifest_id,run_id,execution_id,job_id,position,input_digest,provenance,access_level,content_exact,state) \
         SELECT execution.company_id,execution.id,conversation.id,template.agent_id,selected.revision_id,ownership.manifest_id, \
             execution.run_id,execution.id,job.id,conversation.last_exchange_order+1, \
             sha256(convert_to((execution.frozen_inputs->'context')::text,'UTF8')),template.provenance,run.content_level,run.content_exact,'queued' \
         FROM workflow_executions AS execution \
         JOIN workflow_runs AS run ON run.company_id=execution.company_id AND run.id=execution.run_id \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         JOIN run_dependency_manifests AS ownership ON ownership.company_id=run.company_id AND ownership.run_id=run.id \
         JOIN agent_history_conversations AS conversation ON conversation.company_id=run.company_id AND conversation.id=$3 \
         JOIN LATERAL(SELECT agent_id,provenance FROM agent_history_exchanges \
             WHERE company_id=execution.company_id AND agent_id=$4 ORDER BY position LIMIT 1) AS template ON true \
         JOIN agent_manifest_revisions AS selected ON selected.company_id=ownership.company_id \
             AND selected.manifest_id=ownership.manifest_id AND selected.agent_id=template.agent_id \
         WHERE execution.company_id=$1 AND execution.id=$2",
    ).bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
        .bind(identity.conversation.as_uuid()).bind(f.agent).execute(f.s.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("agent_history_capacity")
    );
    let last: i64 = sqlx::query_scalar(
        "SELECT last_exchange_order FROM agent_history_conversations WHERE company_id=$1 AND id=$2",
    )
    .bind(identity.company.as_uuid())
    .bind(identity.conversation.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert_eq!(last, 0, "native refusal must not allocate a position");
}
