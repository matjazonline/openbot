//! Agent deletion and actual direct ingress compete on the principal before retained-history checks.
use super::*;
use crate::application::use_cases::agent::AgentPersistence;

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct RaceState {
    agents: i64,
    threads: i64,
    intents: i64,
    addresses: i64,
}

async fn race(first: First) {
    let Some(f) = Fixture::isolated("direct_agent_race").await else {
        return;
    };
    let (request, principal) = Box::pin(direct_request(&f)).await;
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id=$1")
        .bind(principal)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    let mut gate = f.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM principals WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(f.company_id)
        .bind(principal)
        .execute(&mut *gate)
        .await
        .unwrap();
    let p = f.persistence.clone();
    let delete = move || tokio::spawn(async move { AgentPersistence::delete(&p, agent).await });
    let p = f.persistence.clone();
    let ingest = move || tokio::spawn(async move { p.commit_inbound(request).await });
    let (ingress, deletion) = match first {
        First::Ingress => {
            let ingress = ingest();
            let ingress_pid = blocked_by(&f, blocker).await;
            let deletion = delete();
            blocked_by(&f, ingress_pid).await;
            (ingress, deletion)
        }
        First::Delete => {
            let deletion = delete();
            let deletion_pid = blocked_by(&f, blocker).await;
            let ingress = ingest();
            blocked_by(&f, deletion_pid).await;
            (ingress, deletion)
        }
    };
    gate.commit().await.unwrap();
    let received = ingress.await.unwrap();
    let removed = deletion.await.unwrap();
    let counts: RaceState = sqlx::query_as(
        "SELECT (SELECT count(*) FROM agents WHERE company_id=$1 AND id=$3) AS agents, \
         (SELECT count(*) FROM threads WHERE company_id=$1 AND principal_id=$2) AS threads, \
         (SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND principal_id=$2) AS intents, \
         (SELECT count(*) FROM addresses WHERE company_id=$1 AND principal_id=$2) AS addresses",
    )
    .bind(f.company_id)
    .bind(principal)
    .bind(agent)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    match first {
        First::Ingress => {
            assert_eq!(received.unwrap().disposition, CommitDisposition::Created);
            assert!(
                matches!(removed, Err(AppError::Conflict(ref reason)) if reason.contains("direct threads"))
            );
            assert_eq!(
                counts,
                RaceState {
                    agents: 1,
                    threads: 1,
                    intents: 1,
                    addresses: 1
                },
                "ingress wins and the complete live agent remains"
            );
        }
        First::Delete => {
            removed.unwrap();
            assert!(
                matches!(received, Err(AppError::Superseded(_))),
                "{received:?}"
            );
            assert_eq!(
                counts,
                RaceState {
                    agents: 0,
                    threads: 0,
                    intents: 0,
                    addresses: 0
                },
                "retirement wins and ingress stores nothing"
            );
        }
    }
}

#[tokio::test]
async fn agent_address_delete_competing_ingress_first_keeps_the_agent_and_direct_history() {
    Box::pin(race(First::Ingress)).await;
}

#[tokio::test]
async fn agent_address_delete_competing_retirement_first_refuses_ingress_atomically() {
    Box::pin(race(First::Delete)).await;
}
