//! Real publication/retirement claimants. C1 separates an inherited owner-tenant cleanup
//! failure from the principal-to-account lock protocol; the owner prefix case never commits it.
use super::*;
use crate::adapters::persistence::user_lifecycle::{finish_account_deletion_on, lock_account_on};
use crate::adapters::persistence::workflow::tests::message_tests::support::member;
use crate::application::ports::account_deletion::HostAccountDeletion;
use crate::application::workflow::message::MessagePlan;
use crate::entities::transport::PrincipalId;
use crate::use_cases::company::{CompanyPersistence, CompanyWrite};

enum ReviewerOwnership {
    RunCompany,
    OtherCompany,
}

#[tokio::test]
async fn workflow_reviewer_principal_final_publication_rechecks_approved_reviewer_privilege() {
    let PreparedPublication {
        fixture: f,
        claim,
        plan,
        retiring_user: reviewer,
        retiring_principal: principal,
    } = Box::pin(prepared(ReviewerOwnership::OtherCompany)).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(reviewer)
        .execute(f.p.pool())
        .await
        .unwrap();
    assert_send_and_clearance_remain_valid(&f, reviewer, principal).await;
    assert_eq!(outbound_count(&f).await, 0);
    assert_eq!(
        f.p.commit_reply_publication(claim, &plan).await.unwrap(),
        ReplyPublicationOutcome::Refused(ReplyPublicationFailure::AuthorityRevoked)
    );
    assert_eq!(outbound_count(&f).await, 0);
    assert_eq!(f.state().await["run"], "succeeded");
}

async fn assert_send_and_clearance_remain_valid(
    f: &FinalFixture,
    reviewer: Uuid,
    principal: PrincipalId,
) {
    let role: String =
        sqlx::query_scalar("SELECT role FROM company_members WHERE company_id=$1 AND user_id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(reviewer)
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(role, "member");
    let mut tx = f.p.pool().begin().await.unwrap();
    let actor = crate::adapters::persistence::workflow::action_authority::prelock_on(
        &mut tx,
        crate::application::workflow::actions::ActionScope {
            company: f.scope.company,
            run: f.scope.run,
            execution: f.scope.execution,
        },
    )
    .await
    .unwrap()
    .expect("the run actor retains send authority");
    assert_eq!(actor, f.s.owner());
    assert_ne!(actor.user_id(), reviewer);
    crate::adapters::persistence::workflow::channel_reply_approvals::require_principal_clearance_on(
        &mut tx, f.scope, principal.as_uuid(),
    ).await.unwrap();
    tx.rollback().await.unwrap();
}
struct ReviewActors {
    reviewer: Uuid,
    actor: Uuid,
}
struct PreparedPublication {
    fixture: FinalFixture,
    claim: ReplyPublicationClaim,
    plan: MessagePlan,
    retiring_user: Uuid,
    retiring_principal: PrincipalId,
}

async fn prepared(ownership: ReviewerOwnership) -> PreparedPublication {
    let s = Box::pin(Intents::new()).await;
    let admin = member(&s.fixture, "admin").await;
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(admin)
        .execute(s.pool())
        .await
        .unwrap();
    let ReviewActors { reviewer, actor } = match ownership {
        ReviewerOwnership::RunCompany => ReviewActors {
            reviewer: s.owner().user_id(),
            actor: admin,
        },
        ReviewerOwnership::OtherCompany => {
            let company = CompanyPersistence::create(
                s.p(),
                admin,
                CompanyWrite {
                    name: "Reviewer owns this other company".into(),
                    slug: format!("reviewer-{}", Uuid::new_v4()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            assert_ne!(company.id, s.fixture.target.company.as_uuid());
            ReviewActors {
                reviewer: admin,
                actor: s.owner().user_id(),
            }
        }
    };
    let reviewer_principal = s.fixture.principal(reviewer).await;
    assert_ne!(reviewer_principal, reviewer);
    let policy = ReplyApprovalPolicy::new(vec![reviewer_principal], 60).unwrap();
    let f = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source: final_source("data.map"),
            contract: literal_contract("Approved"),
            policy: Some(policy),
            run_as: Some(actor),
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    let selected_actor: Uuid =
        sqlx::query_scalar("SELECT actor_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.scope.company.as_uuid())
            .bind(f.scope.run.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(selected_actor, actor);
    assert_ne!(selected_actor, reviewer);
    f.advance().await;
    let mut command = f.command("approve").await;
    command.actor = WorkflowActor::authenticated(reviewer).unwrap();
    f.p.submit_decision(command).await.unwrap();
    f.advance().await;
    let claim = claim(&f).await;
    let plan = super::authority::plan(&f, claim).await;
    PreparedPublication {
        fixture: f,
        claim,
        plan,
        retiring_user: reviewer,
        retiring_principal: PrincipalId::new(reviewer_principal),
    }
}

async fn blocked_by(pool: &sqlx::PgPool, blocker: i32, pattern: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let pid:Option<i32>=sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)) AND query LIKE $2 ORDER BY pid LIMIT 1")
                .bind(blocker).bind(pattern).fetch_optional(pool).await.unwrap();
            if let Some(pid)=pid {return pid}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("production claimant must block at the intended account/run lock")
}

#[tokio::test]
async fn workflow_reviewer_principal_retirement_first_publication_refuses_without_deadlock() {
    let PreparedPublication {
        fixture: f,
        claim,
        plan,
        retiring_user,
        ..
    } = Box::pin(prepared(ReviewerOwnership::OtherCompany)).await;
    let mut retirement = f.p.pool().begin().await.unwrap();
    let locks = lock_account_on(&mut retirement, retiring_user)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let finish = async {
        blocked_by(f.p.pool(), blocker, "%FROM users WHERE id=ANY%FOR SHARE%").await;
        finish_account_deletion_on(&mut retirement, retiring_user, locks)
            .await
            .unwrap();
        retirement.commit().await.unwrap();
    };
    let (publication, ()) = tokio::join!(
        Box::pin(f.p.commit_reply_publication(claim, &plan)),
        Box::pin(finish)
    );
    assert!(
        matches!(publication, Err(AppError::NotFound(_))),
        "expected current-identity refusal, got {publication:?}"
    );
    assert!(matches!(
        f.p.commit_reply_publication(claim, &plan).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(outbound_count(&f).await, 0);
}

#[tokio::test]
async fn workflow_reviewer_principal_publication_first_retirement_waits_for_owner_account_lock() {
    let PreparedPublication {
        fixture: f,
        claim,
        plan,
        retiring_user,
        retiring_principal,
    } = Box::pin(prepared(ReviewerOwnership::OtherCompany)).await;
    let mut run_lock = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.run.as_uuid())
        .execute(&mut *run_lock)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *run_lock)
        .await
        .unwrap();
    let retirement = async {
        let publisher = blocked_by(f.p.pool(), blocker, "%FROM workflow_runs%FOR SHARE%").await;
        let deletion = f.p.delete_user_account(retiring_user);
        tokio::pin!(deletion);
        tokio::select! {
            result=&mut deletion=>panic!("retirement bypassed the reviewer account SHARE lock: {result:?}"),
            _=blocked_by(f.p.pool(),publisher,"%FROM users WHERE id=$1 FOR UPDATE%")=>{run_lock.commit().await.unwrap();deletion.await.unwrap();}
        }
    };
    let (publication, ()) = tokio::join!(
        Box::pin(f.p.commit_reply_publication(claim, &plan)),
        Box::pin(retirement)
    );
    let published = publication.unwrap();
    assert!(matches!(published, ReplyPublicationOutcome::Published(_)));
    assert_eq!(
        f.p.commit_reply_publication(claim, &plan).await.unwrap(),
        published
    );
    assert_eq!(
        outbound_count(&f).await,
        1,
        "replay after retirement creates no duplicate"
    );
    let cancelled:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers AS reviewer JOIN message_deliveries AS delivery ON delivery.id=reviewer.notice_delivery_id WHERE reviewer.company_id=$1 AND reviewer.principal_id=$2 AND delivery.cancellation_reason='recipient_revoked'")
        .bind(f.scope.company.as_uuid()).bind(retiring_principal.as_uuid()).fetch_one(f.p.pool()).await.unwrap();
    assert_eq!(cancelled, 1);
}

#[tokio::test]
async fn workflow_reviewer_principal_run_company_owner_lock_prefix_precedes_publication_company_gate()
 {
    let PreparedPublication {
        fixture: f,
        claim,
        plan,
        retiring_user,
        ..
    } = Box::pin(prepared(ReviewerOwnership::RunCompany)).await;
    let mut retirement = f.p.pool().begin().await.unwrap();
    let _locks = lock_account_on(&mut retirement, retiring_user)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *retirement)
        .await
        .unwrap();
    let release = async {
        blocked_by(f.p.pool(), blocker, "%FROM users WHERE id=ANY%FOR SHARE%").await;
        retirement.rollback().await.unwrap(); // Prefix-only: C1 does not claim owner teardown works.
    };
    let (publication, ()) = tokio::join!(
        Box::pin(f.p.commit_reply_publication(claim, &plan)),
        Box::pin(release)
    );
    assert!(matches!(
        publication.unwrap(),
        ReplyPublicationOutcome::Published(_)
    ));
    assert_eq!(outbound_count(&f).await, 1);
}

#[path = "reviewer_history_hint_tests.rs"]
mod history_hints;
