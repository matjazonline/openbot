//! Actual department and account writers, with observable PostgreSQL lock queues.
use super::*;
use crate::adapters::persistence::channel_gate::{
    ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on,
};
use crate::adapters::persistence::test_support::minted_agent_notice_key;
use crate::application::notification::{
    ClaimedNotificationEvent, NotificationPersistence, NotificationProjectionCommit,
    NotificationProjectionCommitOutcome,
};
use crate::application::use_cases::user::{ProfileUpdate, UserPersistence};
use crate::entities::value_objects::EmailAddress;
use crate::transport::WorkerId;
use std::time::Duration;

async fn claim_notice(f: &AgentCase) -> ClaimedNotificationEvent {
    let mut claimed = f
        .people
        .role
        .p()
        .claim_notification_events(WorkerId::random(), Duration::from_secs(30), 1)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    claimed.pop().unwrap()
}

async fn waiter(p: &PostgresPersistence, blocker: i32, mode: Option<&str>) -> i32 {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT activity.pid FROM pg_stat_activity AS activity \
                 WHERE activity.datname=current_database() AND $1=ANY(pg_blocking_pids(activity.pid)) \
                   AND ($2::text IS NULL OR EXISTS(SELECT 1 FROM pg_locks AS waiting \
                     WHERE waiting.pid=activity.pid AND waiting.locktype='advisory' \
                       AND NOT waiting.granted AND waiting.mode=$2)) ORDER BY activity.pid LIMIT 1",
            ).bind(blocker).bind(mode).fetch_optional(p.pool()).await.unwrap();
            if let Some(pid) = pid { return pid; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("the real writer must reach its asserted lock boundary")
}

async fn edit_role(f: &AgentCase) {
    crate::adapters::persistence::test_support::department_use_cases(f.people.role.p())
        .assign_role(
            f.people.users[0],
            f.people.role.company().as_uuid(),
            f.people.role.department,
            DepartmentRole::Reviewer,
            f.people.principals[0],
        )
        .await
        .unwrap();
}

#[derive(Clone, Copy)]
enum AccountSchedule {
    Idle,
    Update,
}

async fn update_account(
    p: &PostgresPersistence,
    account: &crate::entities::user::User,
    start: tokio::sync::oneshot::Receiver<()>,
) {
    start.await.unwrap();
    let email = EmailAddress::from(account.email.as_str());
    let saved = p
        .update_profile(
            account.id,
            ProfileUpdate {
                username: &account.username,
                email: &email,
                avatar_url: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.email, account.email);
}

async fn role_first(schedule: AccountSchedule) {
    let f = Box::pin(pending_agent()).await;
    own_default_desk(&f).await;
    let p = f.people.role.p();
    let claim = claim_notice(&f).await;
    let expected = p
        .resolve_notification_projection(&claim.event)
        .await
        .unwrap();
    let mut blocker = p.pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    acquire_channel_gate_on(
        &mut blocker,
        ChannelGateKey::new(f.people.role.company().as_uuid(), f.created.desk_id),
        ChannelGateAccess::ChangeSettings,
    )
    .await
    .unwrap();
    let account =
        UserPersistence::get_by_id(p, f.people.role.admission.binding.target.actor.user_id())
            .await
            .unwrap()
            .unwrap();
    let (role_tx, role_rx) = tokio::sync::oneshot::channel();
    let (account_tx, account_rx) = tokio::sync::oneshot::channel();
    let key = minted_agent_notice_key(&expected);
    let notice = p.commit_notification_projection(NotificationProjectionCommit {
        fence: &claim.lease,
        expected: &expected,
        email: None,
        agent_message_key: key.as_ref(),
        escalation_ack_hours: Default::default(),
    });
    let role = async {
        role_rx.await.unwrap();
        edit_role(&f).await;
    };
    let update = async {
        if matches!(schedule, AccountSchedule::Update) {
            update_account(p, &account, account_rx).await;
        }
    };
    let release = async {
        waiter(p, blocker_pid, Some("ShareLock")).await;
        role_tx.send(()).unwrap();
        let writer = waiter(p, blocker_pid, Some("ExclusiveLock")).await;
        if matches!(schedule, AccountSchedule::Update) {
            account_tx.send(()).unwrap();
            waiter(p, writer, None).await;
        }
        blocker.commit().await.unwrap();
    };
    let (result, (), (), ()) = tokio::join!(notice, role, update, release);
    assert_eq!(
        result.unwrap().0,
        NotificationProjectionCommitOutcome::Deferred
    );
    assert_eq!(
        notice_queue(&f).await,
        json!({"status":"pending","attempts":1,"class":"route_busy","backoff":true})
    );
    assert_eq!(facts(&f).await["messages"], 0);
    retry_notice(&f).await;
    process(&f, claims(&f).await.pop().unwrap()).await;
    assert_eq!(facts(&f).await["messages"], 1);
    assert_eq!(facts(&f).await["intents"], 1);
    assert_eq!(facts(&f).await["runs"], 1);
}

#[tokio::test]
async fn workflow_escalation_agent_real_role_writer_gate_contention_defers_then_admits_once() {
    Box::pin(role_first(AccountSchedule::Idle)).await;
}

#[tokio::test]
async fn workflow_escalation_agent_real_account_update_role_writer_and_notice_all_finish() {
    Box::pin(role_first(AccountSchedule::Update)).await;
}

#[tokio::test]
async fn workflow_escalation_agent_notice_before_real_role_writer_commits_captured_root_once() {
    let f = Box::pin(pending_agent()).await;
    own_default_desk(&f).await;
    let p = f.people.role.p();
    let claim = claim_notice(&f).await;
    let expected = p
        .resolve_notification_projection(&claim.event)
        .await
        .unwrap();
    let mut blocker = p.pool().begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM notification_events WHERE id=$1 FOR UPDATE")
        .bind(claim.event.id.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (role_tx, role_rx) = tokio::sync::oneshot::channel();
    let key = minted_agent_notice_key(&expected);
    let notice = p.commit_notification_projection(NotificationProjectionCommit {
        fence: &claim.lease,
        expected: &expected,
        email: None,
        agent_message_key: key.as_ref(),
        escalation_ack_hours: Default::default(),
    });
    let role = async {
        role_rx.await.unwrap();
        edit_role(&f).await;
    };
    let release = async {
        let projector = waiter(p, blocker_pid, None).await;
        role_tx.send(()).unwrap();
        waiter(p, projector, None).await;
        blocker.commit().await.unwrap();
    };
    let (result, (), ()) = tokio::join!(notice, role, release);
    assert_eq!(
        result.unwrap().0,
        NotificationProjectionCommitOutcome::Applied
    );
    process(&f, claims(&f).await.pop().unwrap()).await;
    assert_eq!(facts(&f).await["messages"], 1);
    assert_eq!(facts(&f).await["intents"], 1);
    assert_eq!(facts(&f).await["runs"], 1);
    let exact: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(run.binding_id=receipt.binding_id \
          AND run.binding_revision=receipt.binding_revision AND ownership.notice_id=receipt.notice_id) \
         FROM workflow_escalation_agent_receipts AS receipt \
         JOIN workflow_escalation_handling_roots AS ownership ON ownership.company_id=receipt.company_id \
           AND ownership.notice_id=receipt.notice_id \
         JOIN workflow_runs AS run ON run.company_id=ownership.company_id AND run.id=ownership.run_id \
         WHERE receipt.company_id=$1",
    ).bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(exact);
}
