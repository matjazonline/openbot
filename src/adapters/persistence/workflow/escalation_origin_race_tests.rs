//! Both real serialization orders: source retention owns company, or admission owns company.
use super::*;
use crate::application::workflow::message_admission::ClaimedIntent;
use std::sync::Arc;
use std::time::Duration;

async fn prepared() -> (Arc<AgentCase>, RetentionRoot, ClaimedIntent) {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    other_bindings(&f, 7).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let claim = claims(&f).await.pop().unwrap();
    assert_eq!(p.candidate_bindings(&claim).await.unwrap().len(), 1);
    let finished_at = sqlx::query_scalar("UPDATE workflow_runs SET created_at=clock_timestamp()-interval '100 days', \
        finished_at=clock_timestamp()-interval '91 days' WHERE company_id=$1 AND id=$2 RETURNING finished_at")
        .bind(f.people.role.company().as_uuid()).bind(source.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let root = RetentionRoot {
        company: f.people.role.company(),
        run: source,
        finished_at,
    };
    (Arc::new(f), root, claim)
}

async fn advisory_waiter(pool: &sqlx::PgPool, query: &str) -> i32 {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() \
                AND pid<>pg_backend_pid() AND wait_event_type='Lock' AND wait_event='advisory' AND query LIKE $1 LIMIT 1")
                .bind(query).fetch_optional(pool).await.unwrap();
            if let Some(pid)=pid { return pid; }
            tokio::task::yield_now().await;
        }
    }).await.expect("real owner reached the advisory barrier")
}

async fn blocked_by(pool: &sqlx::PgPool, owner: i32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() \
                AND pid<>pg_backend_pid() AND $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(owner)
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("actual admission waits behind the retention owner")
}

#[tokio::test]
async fn workflow_escalation_origin_retention_first_closes_the_competing_actual_admission() {
    let (f, root, claim) = Box::pin(prepared()).await;
    let p = f.people.role.p();
    let key = i64::from_be_bytes(root.run.as_uuid().as_bytes()[..8].try_into().unwrap());
    let mut barrier = p.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *barrier)
        .await
        .unwrap();
    sqlx::raw_sql(&format!("CREATE FUNCTION hold_origin_retention() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        IF OLD.id='{}'::uuid THEN PERFORM pg_advisory_xact_lock({key}); END IF; RETURN OLD; END $$; \
        CREATE TRIGGER zz_hold_origin_retention BEFORE DELETE ON workflow_runs \
        FOR EACH ROW EXECUTE FUNCTION hold_origin_retention();",root.run.as_uuid()))
        .execute(p.pool()).await.unwrap();
    let retiring = {
        let p = p.clone();
        tokio::spawn(async move { p.retire_tree(root).await })
    };
    let owner = advisory_waiter(
        p.pool(),
        "%DELETE FROM workflow_runs AS run USING retained%",
    )
    .await;
    let admitting = {
        let f = f.clone();
        let claim = claim.clone();
        tokio::spawn(async move { Box::pin(process(&f, claim)).await })
    };
    blocked_by(p.pool(), owner).await;
    barrier.commit().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), retiring)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap()
            .runs,
        1
    );
    tokio::time::timeout(Duration::from_secs(5), admitting)
        .await
        .unwrap()
        .unwrap();
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM workflow_message_intents WHERE company_id=$1 AND id=$2 AND state='done'")
        .bind(claim.claim.company.as_uuid()).bind(claim.claim.intent).fetch_one(p.pool()).await.unwrap();
    assert_eq!(outcome, "notification_source_retired");
    assert_eq!(
        counts(&f).await,
        json!({"runs":0,"cases":0,"receipts":0,"targets":1,"roots":0,"messages":1,"origins":1,"model_calls":0})
    );
}

#[tokio::test]
async fn workflow_escalation_origin_admission_first_preserves_its_only_root_when_source_retention_retries()
 {
    let (f, root, claim) = Box::pin(prepared()).await;
    let p = f.people.role.p();
    let key = i64::from_be_bytes(root.run.as_uuid().as_bytes()[..8].try_into().unwrap());
    let mut barrier = p.pool().begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut *barrier)
        .await
        .unwrap();
    sqlx::raw_sql(&format!("CREATE FUNCTION hold_origin_admission() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
        PERFORM pg_advisory_xact_lock({key}); RETURN NEW; END $$; \
        CREATE TRIGGER zz_hold_origin_admission BEFORE INSERT ON workflow_escalation_handling_roots \
        FOR EACH ROW EXECUTE FUNCTION hold_origin_admission();"))
        .execute(p.pool()).await.unwrap();
    let admitting = {
        let f = f.clone();
        tokio::spawn(async move { Box::pin(process(&f, claim)).await })
    };
    advisory_waiter(p.pool(), "%INSERT INTO workflow_escalation_handling_roots%").await;
    assert!(
        p.retire_tree(root).await.unwrap().is_none(),
        "real retention skips the company already owned by admission"
    );
    barrier.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), admitting)
        .await
        .unwrap()
        .unwrap();
    let before: Uuid = sqlx::query_scalar(
        "SELECT run_id FROM workflow_escalation_handling_roots WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(p.retire_tree(root).await.unwrap().unwrap().runs, 1);
    let after: Uuid = sqlx::query_scalar(
        "SELECT run_id FROM workflow_escalation_handling_roots WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        counts(&f).await,
        json!({"runs":1,"cases":0,"receipts":0,"targets":1,"roots":1,"messages":1,"origins":1,"model_calls":0})
    );
}
