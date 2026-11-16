//! Competing production projection/open and membership retirement in both company-lock orders.
use super::*;
use crate::{
    application::notification::{
        ClaimedNotificationEvent, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
    },
    entities::notification::{NotificationId, NotificationProjection},
    transport::WorkerId,
};
use futures::future::LocalBoxFuture;
use std::{future::Future, pin::Pin, time::Duration};
use tokio::task::JoinSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum First {
    Action,
    Retirement,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Project,
    Open,
}
enum Action {
    Project {
        claimed: Box<ClaimedNotificationEvent>,
        expected: Box<NotificationProjection>,
    },
    Open(NotificationId),
}
enum Outcome {
    Project(crate::app_error::AppResult<NotificationProjectionCommitOutcome>),
    Open(crate::app_error::AppResult<String>),
    Retirement(crate::app_error::AppResult<()>),
}

fn prepare(f: &Fixture, user: Uuid, target: Uuid, kind: Kind) -> LocalBoxFuture<'_, Action> {
    Box::pin(async move {
        match kind {
            Kind::Project => {
                let mut claimed = f
                    .persistence
                    .claim_notification_events(WorkerId::random(), Duration::from_secs(30), 1)
                    .await
                    .unwrap();
                assert_eq!(claimed.len(), 1);
                let claimed = claimed.remove(0);
                let expected = f
                    .persistence
                    .resolve_notification_projection(&claimed.event)
                    .await
                    .unwrap();
                Action::Project {
                    claimed: Box::new(claimed),
                    expected: Box::new(expected),
                }
            }
            Kind::Open => {
                assert_eq!(projected(f).await, 1);
                Action::Open(list(f, user, target).await.items.remove(0).id)
            }
        }
    })
}

fn start_action(set: &mut JoinSet<Outcome>, f: &Fixture, user: Uuid, target: Uuid, action: Action) {
    let store = f.persistence.clone();
    let company = f.company.id;
    // Erase the production operation future before Tokio adds its task/drop machinery.
    let work: Pin<Box<dyn Future<Output = Outcome> + Send>> = Box::pin(async move {
        match action {
            Action::Project { claimed, expected } => Outcome::Project(
                store
                    .commit_notification_projection(NotificationProjectionCommit {
                        fence: &claimed.lease,
                        expected: &expected,
                        email: None,
                        agent_message_key: None,
                        agent_recipient: None,
                        escalation_ack_hours: Default::default(),
                    })
                    .await
                    .map(|result| result.0),
            ),
            Action::Open(id) => Outcome::Open(
                store
                    .open_notification(company, user, PrincipalId::new(target), &[], id)
                    .await,
            ),
        }
    });
    set.spawn(work);
}
fn start_retirement(set: &mut JoinSet<Outcome>, f: &Fixture, user: Uuid) {
    let store = f.persistence.clone();
    let company = f.company.id;
    let work: Pin<Box<dyn Future<Output = Outcome> + Send>> =
        Box::pin(async move { Outcome::Retirement(store.remove_member(company, user).await) });
    set.spawn(work);
}

async fn waiting(f: &Fixture, count: i64) {
    tokio::time::timeout(Duration::from_secs(3),async{
        loop {
            let waiting:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock'")
                .fetch_one(f.pool()).await.unwrap();
            if waiting>=count{return;}
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("both actual contenders reached the owned database's company lock");
}

async fn finish(set: &mut JoinSet<Outcome>) -> Vec<Outcome> {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut results = Vec::new();
        while let Some(result) = set.join_next().await {
            results.push(result.expect("production contender did not panic"));
        }
        results
    })
    .await;
    match result {
        Ok(results) => results,
        Err(_) => {
            set.shutdown().await;
            panic!("contenders must complete without deadlock or lock-timeout victims");
        }
    }
}

fn outcomes(results: Vec<Outcome>, first: First, expected_href: &str) {
    assert_eq!(results.len(), 2);
    for result in results {
        match result {
            Outcome::Retirement(result) => {
                result.expect("retirement commits normally in both orders")
            }
            Outcome::Project(result) => assert_eq!(
                result.expect("projection must not abort"),
                if first == First::Action {
                    NotificationProjectionCommitOutcome::Applied
                } else {
                    NotificationProjectionCommitOutcome::Stale
                }
            ),
            Outcome::Open(result) => {
                if first == First::Action {
                    assert_eq!(result.unwrap(), expected_href);
                } else {
                    assert!(
                        matches!(result, Err(AppError::NotFound(_) | AppError::Conflict(_))),
                        "ordinary current-authority refusal: {result:?}"
                    );
                }
            }
        }
    }
}

fn race(first: First, kind: Kind) -> LocalBoxFuture<'static, ()> {
    Box::pin(async move {
        let Some(f) = Fixture::isolated().await else {
            return;
        };
        let (user, name) = account(&f.persistence, "receipt_race").await;
        let target = f.join(user, &name).await;
        // Keep the canonical-ingress/worker setup futures out of the enclosing race frame.
        let (thread, _) = received(&f, target, f.owner).await;
        let action = prepare(&f, user, target, kind).await;
        let mut barrier = f.pool().begin().await.unwrap();
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
            .bind(f.company.id)
            .execute(&mut *barrier)
            .await
            .unwrap();
        let mut set = JoinSet::new();
        match first {
            First::Action => {
                start_action(&mut set, &f, user, target, action);
                waiting(&f, 1).await;
                start_retirement(&mut set, &f, user);
            }
            First::Retirement => {
                start_retirement(&mut set, &f, user);
                waiting(&f, 1).await;
                start_action(&mut set, &f, user, target, action);
            }
        }
        waiting(&f, 2).await;
        barrier.commit().await.unwrap();
        let expected_href = format!(
            "/ui?company_id={}&principal_id={target}&thread_id={}",
            f.company.id, thread.id
        );
        outcomes(finish(&mut set).await, first, &expected_href);
        assert!(list(&f, user, target).await.items.is_empty());
        let rows: Vec<(String, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as("SELECT state,read_at FROM notifications WHERE company_id=$1")
                .bind(f.company.id)
                .fetch_all(f.pool())
                .await
                .unwrap();
        if kind == Kind::Open || first == First::Action {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].0, "withdrawn");
            assert_eq!(
                rows[0].1.is_some(),
                kind == Kind::Open && first == First::Action
            );
        } else {
            assert!(rows.is_empty());
        }
        let mail: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM message_deliveries WHERE purpose='notification'",
        )
        .fetch_one(f.pool())
        .await
        .unwrap();
        assert_eq!(mail, 0);
        if kind == Kind::Project && first == First::Retirement {
            sqlx::query("UPDATE notification_events SET available_at=clock_timestamp() WHERE company_id=$1 AND status='pending'")
                .bind(f.company.id).execute(f.pool()).await.unwrap();
            assert_eq!(projected(&f).await, 1);
            assert!(list(&f, user, target).await.items.is_empty());
        }
    })
}

#[tokio::test]
async fn direct_thread_notification_competing_projection_and_retirement_finish_in_both_lock_orders()
{
    race(First::Action, Kind::Project).await;
    race(First::Retirement, Kind::Project).await;
}
#[tokio::test]
async fn direct_thread_notification_competing_open_and_retirement_finish_in_both_lock_orders() {
    race(First::Action, Kind::Open).await;
    race(First::Retirement, Kind::Open).await;
}
