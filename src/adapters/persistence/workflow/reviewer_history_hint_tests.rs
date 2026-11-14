use super::*;
use crate::application::workflow::budget::*;
use crate::application::workflow::decisions::{
    AddDecisionComment, DecisionDisposition, DecisionSubmission,
};
use crate::domain::workflow::{BudgetCharge, BudgetResource};

#[path = "reviewer_terminal_hint_tests.rs"]
mod terminal_hints;

#[path = "reviewer_jev_hint_tests.rs"]
mod jev_hints;

#[path = "reviewer_control_hint_tests.rs"]
mod control_hints;

#[path = "reviewer_wait_hint_tests.rs"]
mod wait_hints;

#[path = "reviewer_completion_hint_tests.rs"]
mod completion_hints;

struct HistoricalReply {
    fixture: FinalFixture,
    retired: Uuid,
    command: SubmitDecision,
    comment: AddDecisionComment,
    submitted: DecisionSubmission,
}

enum BusinessContinuation {
    Finish,
    NextStep,
    NextIoStep,
}

struct PendingReply {
    fixture: FinalFixture,
    retired: Uuid,
    command: SubmitDecision,
    comment: AddDecisionComment,
}

#[derive(Clone, Copy)]
enum ReplyDeadline {
    Default,
    Short,
}

fn business_source(principal: Uuid, continuation: BusinessContinuation) -> Value {
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["reviewers"] =
        json!({"literal":[{"kind":"principal","id":principal}]});
    source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
    source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    if matches!(
        continuation,
        BusinessContinuation::NextStep | BusinessContinuation::NextIoStep
    ) {
        source["steps"]["start"]["routes"]["choices"]["continue"] = json!("next");
        source["steps"]["next"] = if matches!(continuation, BusinessContinuation::NextIoStep) {
            source["limits"]["root_budget"] =
                json!({"activations":10,"model_calls":10,"repetitions":10});
            let io: Value =
                serde_json::from_str(&registry::example("context.history").unwrap().source)
                    .unwrap();
            io["steps"]["start"].clone()
        } else {
            json!({"type":"data.map","with":{
                "value":{"literal":"next"},"output_schema":{"literal":true}},
                "routes":{"success":"$end"}})
        };
    }
    source
}

fn pending_reply(
    continuation: BusinessContinuation,
) -> impl std::future::Future<Output = PendingReply> {
    pending_reply_with_deadline(continuation, ReplyDeadline::Default)
}

async fn pending_reply_with_deadline(
    continuation: BusinessContinuation,
    deadline: ReplyDeadline,
) -> PendingReply {
    let s = Box::pin(Intents::new()).await;
    let retired = member(&s.fixture, "admin").await;
    let reviewer = s.fixture.principal(retired).await;
    let source = business_source(s.owner_principal(), continuation);
    if matches!(deadline, ReplyDeadline::Short) {
        // This fixture owns its database. Set the short lifetime at birth; all production
        // snapshot guards remain enabled and the sealed row is never rewritten.
        sqlx::raw_sql("CREATE FUNCTION short_reply_deadline_test() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.deadline := NEW.created_at + interval '2 seconds'; RETURN NEW; END $$; CREATE TRIGGER short_reply_deadline_test BEFORE INSERT ON workflow_runs FOR EACH ROW EXECUTE FUNCTION short_reply_deadline_test();")
            .execute(s.pool()).await.unwrap();
    }
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Complete"),
            policy: Some(ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap()),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    if matches!(deadline, ReplyDeadline::Short) {
        sqlx::raw_sql("DROP TRIGGER short_reply_deadline_test ON workflow_runs; DROP FUNCTION short_reply_deadline_test()")
            .execute(fixture.p.pool()).await.unwrap();
    }
    assert!(
        fixture
            .p
            .park_decision(fixture.scope, &approval_test_support::notices(&fixture.p))
            .await
            .unwrap()
            .is_some()
    );
    let decision=DecisionId(sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND subject_kind='business'")
        .bind(fixture.scope.company.as_uuid()).bind(fixture.scope.run.as_uuid()).fetch_one(fixture.p.pool()).await.unwrap());
    let view = fixture
        .p
        .read_decision(fixture.scope.company, fixture.s.owner(), decision)
        .await
        .unwrap();
    let comment = AddDecisionComment {
        company: fixture.scope.company,
        actor: fixture.s.owner(),
        decision,
        command_key: DecisionCommandKey::parse("before-policy-retirement").unwrap(),
        body: "Recorded before settlement".into(),
    };
    fixture
        .p
        .add_decision_comment(comment.clone())
        .await
        .unwrap();
    let command = SubmitDecision {
        company: fixture.scope.company,
        actor: fixture.s.owner(),
        decision,
        command_key: DecisionCommandKey::parse("recorded-business-vote").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: "continue".into(),
        data: json!(null),
        feedback: String::new(),
    };
    PendingReply {
        fixture,
        retired,
        command,
        comment,
    }
}

async fn historical_reply() -> HistoricalReply {
    let PendingReply {
        fixture,
        retired,
        command,
        comment,
    } = pending_reply(BusinessContinuation::Finish).await;
    let submitted = fixture.p.submit_decision(command.clone()).await.unwrap();
    HistoricalReply {
        fixture,
        retired,
        command,
        comment,
        submitted,
    }
}

struct ReplyIo {
    pending: PendingReply,
    claim: ClaimedWorkflow,
    policy: LeasePolicy,
}

enum ReviewerRetirement {
    BeforeClaim,
    AfterClaim,
}

fn reply_io(retirement: ReviewerRetirement) -> impl std::future::Future<Output = ReplyIo> {
    reply_io_with_deadline(retirement, ReplyDeadline::Default)
}

async fn reply_io_with_deadline(
    retirement: ReviewerRetirement,
    deadline: ReplyDeadline,
) -> ReplyIo {
    let pending = Box::pin(pending_reply_with_deadline(
        BusinessContinuation::NextIoStep,
        deadline,
    ))
    .await;
    let f = &pending.fixture;
    let next =
        f.p.submit_decision(pending.command.clone())
            .await
            .unwrap()
            .successor
            .unwrap();
    let policy = LeasePolicy::new(std::time::Duration::from_secs(60)).unwrap();
    if matches!(retirement, ReviewerRetirement::BeforeClaim) {
        f.p.delete_user_account(pending.retired).await.unwrap();
    }
    let claim =
        f.p.claim_io(next, WorkflowWorkerId(Uuid::new_v4()), policy)
            .await
            .unwrap()
            .unwrap();
    if matches!(retirement, ReviewerRetirement::AfterClaim) {
        f.p.delete_user_account(pending.retired).await.unwrap();
    }
    ReplyIo {
        pending,
        claim,
        policy,
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_lease_renewal_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(reply_io(ReviewerRetirement::AfterClaim)).await;
    let f = &io.pending.fixture;
    assert!(
        f.p.renew_io(io.claim.fence, io.policy)
            .await
            .unwrap()
            .is_some()
    );
    assert!(f.p.validate_io(io.claim.fence, io.policy).await.unwrap());
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(state, "running");
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_budget_reservation_and_replay_ignore_unused_retired_final_reviewers()
 {
    let io = Box::pin(reply_io(ReviewerRetirement::AfterClaim)).await;
    let f = &io.pending.fixture;
    let request = BudgetReservation::new(
        io.claim.fence,
        BudgetReservationKey::parse("unused-final-policy").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.p.reserve_budget(request.clone(), io.policy)
            .await
            .unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    assert_eq!(
        f.p.reserve_budget(request, io.policy).await.unwrap(),
        BudgetReservationResult::Replayed(BudgetDisposition::Granted)
    );
    let receipt: Value = sqlx::query_scalar("SELECT jsonb_build_object('calls',usage.model_calls, \
        'receipts',(SELECT count(*) FROM workflow_budget_receipts WHERE company_id=usage.company_id AND reservation_key='unused-final-policy')) \
        FROM workflow_root_budget_usage AS usage WHERE company_id=$1 AND root_run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(receipt, json!({"calls":1,"receipts":1}));
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_claim_ignores_unused_retired_final_reviewers() {
    let io = Box::pin(reply_io(ReviewerRetirement::BeforeClaim)).await;
    let f = &io.pending.fixture;
    assert!(f.p.validate_io(io.claim.fence, io.policy).await.unwrap());
    let attempt: i32 = sqlx::query_scalar(
        "SELECT attempt_ordinal FROM background_tasks WHERE company_id=$1 AND id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(io.claim.fence.scope.job.0)
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(attempt, 1);
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_budget_company_precedes_run_during_actual_retirement() {
    use crate::adapters::persistence::user_lifecycle::{
        finish_account_deletion_on, lock_account_on,
    };
    let io = Box::pin(reply_io(ReviewerRetirement::AfterClaim)).await;
    let f = &io.pending.fixture;
    let user = member(&f.s.fixture, "admin").await;
    let mut retirement = f.p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, user).await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let request = BudgetReservation::new(
        io.claim.fence,
        BudgetReservationKey::parse("retirement-prefix").unwrap(),
        BudgetCharge::new(BudgetResource::ModelCall, 1).unwrap(),
    )
    .unwrap();
    let reserve = f.p.reserve_budget(request, io.policy);
    tokio::pin!(reserve);
    tokio::select! {
        _=blocked_by(f.p.pool(),blocker,"%companies%FOR NO KEY UPDATE%")=>{},
        result=reserve.as_mut()=>panic!("budget owner must first wait for retirement's company: {result:?}"),
    }
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE NOWAIT")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *retirement)
        .await
        .expect("budget company ownership must precede run ownership");
    finish_account_deletion_on(&mut retirement, user, locks)
        .await
        .unwrap();
    retirement.commit().await.unwrap();
    assert_eq!(
        reserve.await.unwrap(),
        BudgetReservationResult::Recorded(BudgetDisposition::Granted)
    );
    let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(user)
        .fetch_one(f.p.pool())
        .await
        .unwrap();
    assert!(
        !present,
        "actual retirement and actual budget owner both commit"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_nonterminal_vote_ignores_unused_retired_reply_policy_accounts()
 {
    let pending = Box::pin(pending_reply(BusinessContinuation::NextStep)).await;
    let f = &pending.fixture;
    f.p.delete_user_account(pending.retired).await.unwrap();
    let result = f.p.submit_decision(pending.command.clone()).await.unwrap();
    assert_eq!(result.disposition, DecisionDisposition::Applied);
    assert!(result.successor.is_some());
    assert_eq!(
        f.p.submit_decision(pending.command.clone())
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Replayed
    );
    let state: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',run.state, \
        'finals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=run.company_id AND run_id=run.id), \
        'submitted',(SELECT count(*) FROM workflow_decisions WHERE company_id=run.company_id AND run_id=run.id AND submitted_at IS NOT NULL)) \
        FROM workflow_runs AS run WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(state, json!({"state":"running","finals":0,"submitted":1}));
}

#[tokio::test]
async fn workflow_reviewer_follow_fresh_final_vote_requires_current_reply_policy_accounts() {
    let pending = Box::pin(pending_reply(BusinessContinuation::Finish)).await;
    let f = &pending.fixture;
    f.p.delete_user_account(pending.retired).await.unwrap();
    let before =
        f.p.read_decision(f.scope.company, f.s.owner(), pending.command.decision)
            .await
            .unwrap();
    assert!(matches!(
        f.p.submit_decision(pending.command.clone()).await,
        Err(AppError::NotFound(_))
    ));
    let after =
        f.p.read_decision(f.scope.company, f.s.owner(), pending.command.decision)
            .await
            .unwrap();
    assert_eq!(
        before, after,
        "unavailable final authority leaves the real business decision open"
    );
    let finals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM channel_reply_proposals WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.scope.run.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert_eq!(finals, 0);
}

#[tokio::test]
async fn workflow_reviewer_follow_history_and_replays_do_not_require_unused_retired_reply_policy_accounts()
 {
    let HistoricalReply {
        fixture: f,
        retired,
        command,
        comment,
        submitted,
    } = Box::pin(historical_reply()).await;
    let before =
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap();
    f.p.delete_user_account(retired).await.unwrap();
    let mut check = f.p.pool().begin().await.unwrap();
    let missing = crate::adapters::persistence::workflow::action_authority::prelock_reply_on(
        &mut check,
        crate::application::workflow::actions::ActionScope {
            company: f.scope.company,
            run: f.scope.run,
            execution: f.scope.execution,
        },
        Some(f.s.owner()),
    )
    .await;
    assert!(
        matches!(missing, Err(AppError::NotFound(_))),
        "the retired policy mapping really is unusable for a fresh final response: {missing:?}"
    );
    check.rollback().await.unwrap();
    let after =
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap();
    assert_eq!(
        before, after,
        "historical view must not gain a retired-policy prerequisite"
    );
    let replay = f.p.submit_decision(command.clone()).await.unwrap();
    assert_eq!(replay.disposition, DecisionDisposition::Replayed);
    assert_eq!(replay.revision, submitted.revision);
    assert_eq!(replay.successor, submitted.successor);
    let replayed = f.p.add_decision_comment(comment.clone()).await.unwrap();
    assert_eq!(replayed.disposition, DecisionDisposition::Replayed);
    let changed = AddDecisionComment {
        body: "Changed replay".into(),
        ..comment.clone()
    };
    assert!(matches!(
        f.p.add_decision_comment(changed).await,
        Err(AppError::Conflict(_))
    ));
    let fresh = AddDecisionComment {
        command_key: DecisionCommandKey::parse("new-closed-comment").unwrap(),
        ..comment
    };
    assert!(
        matches!(
            f.p.add_decision_comment(fresh).await,
            Err(AppError::Conflict(_))
        ),
        "a fresh closed comment retains its existing policy"
    );
    assert_eq!(
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap(),
        after
    );
    let old_actor = WorkflowActor::authenticated(retired).unwrap();
    assert!(
        matches!(
            f.p.read_decision(f.scope.company, old_actor, command.decision)
                .await,
            Err(AppError::NotFound(_))
        ),
        "retired caller authority is still denied"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_ordinary_effect_hints_ignore_unused_retired_reply_policy_accounts()
 {
    let historical = Box::pin(historical_reply()).await;
    let f = &historical.fixture;
    f.p.delete_user_account(historical.retired).await.unwrap();
    let mut tx = f.p.pool().begin().await.unwrap();
    let actor = crate::adapters::persistence::workflow::action_authority::prelock_accounts_on(
        &mut tx,
        crate::application::workflow::actions::ActionScope {
            company: f.scope.company,
            run: f.scope.run,
            execution: f.scope.execution,
        },
        Some(f.s.owner()),
    )
    .await
    .unwrap();
    assert!(
        actor.is_some_and(|actor| actor.user_id() == f.s.owner().user_id()),
        "an ordinary effect hint requires its real actor without resolving unused final-policy recipients"
    );
    tx.rollback().await.unwrap();
}
