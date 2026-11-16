use super::*;

#[tokio::test]
async fn workflow_reviewer_follow_owner_retirement_commits_with_a_live_followed_decision() {
    use crate::application::ports::account_deletion::HostAccountDeletion;

    let f = Box::pin(FollowFixture::new(1)).await;
    let owner = f.people.role.admission.binding.target.actor.user_id();
    let principal = PrincipalId::new(
        f.people
            .role
            .admission
            .binding
            .fixture
            .principal(owner)
            .await,
    );
    let departments =
        crate::adapters::persistence::test_support::department_use_cases(f.people.role.p());
    departments
        .add_member(
            owner,
            f.scope.company.as_uuid(),
            f.people.role.department,
            principal,
        )
        .await
        .unwrap();
    departments
        .assign_role(
            owner,
            f.scope.company.as_uuid(),
            f.people.role.department,
            DepartmentRole::Reviewer,
            principal,
        )
        .await
        .unwrap();
    let current = f.assignment().await;
    assert_eq!(current.reviewers.len(), 2);
    assert!(
        current
            .reviewers
            .iter()
            .any(|reviewer| reviewer.principal_id == principal)
    );
    let parked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_pending_follow_decisions \
         WHERE company_id=$1 AND decision_id=$2)",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.decision.0)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert!(
        parked,
        "the role holder owns a currently parked followed decision"
    );

    f.people.role.p().delete_user_account(owner).await.unwrap();

    let removed: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM users WHERE id=$1) \
           AND NOT EXISTS(SELECT 1 FROM companies WHERE id=$2) \
           AND NOT EXISTS(SELECT 1 FROM workflow_runs WHERE company_id=$2) \
           AND NOT EXISTS(SELECT 1 FROM workflow_follow_decisions WHERE company_id=$2) \
           AND NOT EXISTS(SELECT 1 FROM workflow_reviewer_assignments WHERE company_id=$2) \
           AND NOT EXISTS(SELECT 1 FROM workflow_reviewer_notices WHERE company_id=$2)",
    )
    .bind(owner)
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert!(
        removed,
        "owner retirement committed the complete followed-decision cascade"
    );
}

#[tokio::test]
async fn workflow_reviewer_follow_member_retirement_clears_audits_refreshes_and_cancels_in_one_commit()
 {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let f = Box::pin(FollowFixture::new(2)).await;
    let proposal = f.proposal().await;
    let removed_vote = f.vote(0).await;
    assert_eq!(f.notification_worker().drain_once().await.unwrap(), 2);
    f.people
        .role
        .p()
        .remove_member(f.scope.company.as_uuid(), f.people.users[0])
        .await
        .unwrap();
    let current = f.assignment().await;
    assert_eq!(current.reviewers.len(), 1);
    assert_eq!(current.reviewers[0].principal_id, f.people.principals[1]);
    assert_eq!(f.proposal().await, proposal);
    assert!(
        f.people
            .role
            .p()
            .submit_decision(removed_vote)
            .await
            .is_err()
    );
    let audits:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(event) FROM department_administration_events AS event WHERE company_id=$1 AND principal_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_all(f.people.role.p().pool()).await.unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["department_id"], json!(f.people.role.department));
    assert_eq!(
        audits[0]["department_label"],
        json!({"prefix":"Team","truncated":false})
    );
    assert_eq!(audits[0]["principal_kind"], "person");
    assert_eq!(audits[0]["transition_kind"], "demoted");
    assert_eq!(audits[0]["actor_kind"], "system");
    let leaked:i64=sqlx::query_scalar("SELECT count(*) FROM notifications AS notice JOIN message_deliveries AS delivery ON delivery.company_id=notice.company_id AND delivery.id=notice.email_delivery_id \
        WHERE notice.company_id=$1 AND notice.recipient_principal_id=$2 AND delivery.cancellation_requested_at IS NULL")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(leaked, 0);
    f.people
        .role
        .p()
        .submit_decision(f.vote(1).await)
        .await
        .unwrap();
}

#[tokio::test]
async fn workflow_reviewer_follow_account_retirement_clears_all_roles_and_preserves_snapshot_history()
 {
    use crate::application::ports::account_deletion::HostAccountDeletion;
    let f = Box::pin(FollowFixture::new(1)).await;
    let proposal = f.proposal().await;
    f.people
        .role
        .p()
        .delete_user_account(f.people.users[0])
        .await
        .unwrap();
    let empty = f.assignment().await;
    assert!(empty.reviewers.is_empty());
    assert!(empty.refusal.is_some());
    assert_eq!(f.proposal().await, proposal);
    let audits:i64=sqlx::query_scalar("SELECT count(*) FROM department_administration_events WHERE company_id=$1 AND principal_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(audits, 1);
    let roles: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM department_roles WHERE company_id=$1 AND principal_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.people.principals[0].as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(roles, 0);
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(f.people.users[0])
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap();
    assert!(absent, "the actual account deletion committed");
    let history: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_reviewer_assignments WHERE company_id=$1 AND decision_id=$2",
    )
    .bind(f.scope.company.as_uuid())
    .bind(f.decision.0)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(history, 2);
}

#[tokio::test]
async fn workflow_reviewer_follow_failed_final_statement_rolls_back_actual_account_retirement_and_notices()
 {
    let f = Box::pin(FollowFixture::new(1)).await;
    assert_eq!(f.notification_worker().drain_once().await.unwrap(), 1);
    let before = f.assignment().await;
    let proposal = f.proposal().await;
    let before_notice:Value=sqlx::query_scalar("SELECT jsonb_build_object('notification',notification.state,'delivery',delivery.status,'cancelled',delivery.cancellation_requested_at IS NOT NULL) \
        FROM notifications AS notification JOIN message_deliveries AS delivery ON delivery.company_id=notification.company_id AND delivery.id=notification.email_delivery_id \
        WHERE notification.company_id=$1 AND notification.recipient_principal_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    let mut tx = f.people.role.p().pool().begin().await.unwrap();
    let locks =
        crate::adapters::persistence::user_lifecycle::lock_account_on(&mut tx, f.people.users[0])
            .await
            .unwrap();
    Box::pin(
        crate::adapters::persistence::user_lifecycle::finish_account_deletion_on(
            &mut tx,
            f.people.users[0],
            locks,
        ),
    )
    .await
    .unwrap();
    let audit:i64=sqlx::query_scalar("SELECT count(*) FROM department_administration_events WHERE company_id=$1 AND principal_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        audit, 1,
        "actual retirement created the audit before rollback"
    );
    let revision:i64=sqlx::query_scalar("SELECT assignment_revision FROM workflow_follow_decisions WHERE company_id=$1 AND decision_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.decision.0).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        revision, 2,
        "actual retirement refreshed reviewers before rollback"
    );
    let cancelled:bool=sqlx::query_scalar("SELECT bool_and(cancellation_requested_at IS NOT NULL) FROM message_deliveries WHERE company_id=$1 AND purpose='notification'")
        .bind(f.scope.company.as_uuid()).fetch_one(&mut *tx).await.unwrap();
    assert!(
        cancelled,
        "account retirement must cancel email before deleting in-app projections"
    );
    let failed = sqlx::query("SELECT 1/0")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        failed
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("22012")
    );
    tx.rollback().await.unwrap();
    assert_eq!(f.assignment().await, before);
    assert_eq!(f.proposal().await, proposal);
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM department_administration_events WHERE company_id=$1",
    )
    .bind(f.scope.company.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(audit, 0);
    let after_notice:Value=sqlx::query_scalar("SELECT jsonb_build_object('notification',notification.state,'delivery',delivery.status,'cancelled',delivery.cancellation_requested_at IS NOT NULL) \
        FROM notifications AS notification JOIN message_deliveries AS delivery ON delivery.company_id=notification.company_id AND delivery.id=notification.email_delivery_id \
        WHERE notification.company_id=$1 AND notification.recipient_principal_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.people.principals[0].as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(after_notice, before_notice);
}
