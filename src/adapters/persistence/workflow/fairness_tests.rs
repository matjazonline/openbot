use super::*;
use crate::application::workflow::{polling::*, worker::*};
use tokio_util::sync::CancellationToken;

#[path = "fairness_acceptance_tests.rs"]
mod acceptance_tests;

async fn denied(p: &PostgresPersistence, scope: ActivationRequest, requester: WorkflowWorkerId) {
    assert!(
        p.claim_io(scope, requester, lease())
            .await
            .unwrap()
            .is_none()
    );
}
async fn ticket(
    p: &PostgresPersistence,
    company: CompanyId,
) -> (i64, Option<chrono::DateTime<chrono::Utc>>) {
    sqlx::query_as(
        "SELECT ticket,opportunity_until FROM workflow_dispatch_demand WHERE company_id=$1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn workflow_fairness_full_wait_preserves_age_then_protects_quiet_retry() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let quiet = tenant(&f).await;
    let noise = tenant(&f).await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let ca = p.claim_io(a, worker(), lease()).await.unwrap().unwrap();
    let cb = p
        .claim_io(admit(&b, "owner").await, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    let q = admit(&quiet, "quiet").await;
    let w = worker();
    denied(p, q, w).await;
    let original = ticket(p, q.company).await;
    assert!(original.1.is_none());
    sqlx::query(
        "UPDATE workflow_dispatch_demand SET registered_at=clock_timestamp()-interval '2 minutes'",
    )
    .execute(p.pool())
    .await
    .unwrap();
    for index in 0..8 {
        denied(p, admit(&noise, &format!("noise-{index}")).await, worker()).await;
        assert_eq!(ticket(p, q.company).await, original);
    }
    let sticky = p.poll_fair(w, 128).await.unwrap();
    assert_eq!(sticky.candidates.len(), 1);
    assert_eq!(sticky.candidates[0].scope, q);
    assert_eq!(sticky.demand, Some(DemandTicket(original.0)));
    // Release immediately after the quiet poll. Refill must protect its next retry.
    finish(p, &ca).await;
    let next_noise = admit(&noise, "refill").await;
    denied(p, next_noise, worker()).await;
    let offered = ticket(p, q.company).await;
    assert!(offered.1.is_some());
    for _ in 0..8 {
        denied(p, next_noise, worker()).await;
    }
    assert_eq!(ticket(p, q.company).await, offered);
    let cq = PostgresPersistence::new(p.pool().clone())
        .claim_io(q, w, lease())
        .await
        .unwrap()
        .unwrap();
    finish(p, &cq).await;
    finish(p, &cb).await;
    assert!(
        p.claim_io(next_noise, worker(), lease())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn workflow_fairness_open_offer_survives_older_company_becoming_uncapped() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let c = tenant(&f).await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(3, 1).unwrap())
        .await
        .unwrap();
    let ca = p.claim_io(a, worker(), lease()).await.unwrap().unwrap();
    let a2 = admit(&f, "older-capped").await;
    denied(p, a2, worker()).await;
    assert!(ticket(p, a.company).await.1.is_none());
    let b1 = admit(&b, "quiet").await;
    // Seed supported B demand while the singleton is held, by temporarily filling
    // the remaining global slots with independent real claims.
    let c1 = admit(&c, "owner-c").await;
    let d = tenant(&f).await;
    let cc = p.claim_io(c1, worker(), lease()).await.unwrap().unwrap();
    let cd = p
        .claim_io(admit(&d, "owner-d").await, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    let bw = worker();
    denied(p, b1, bw).await;
    finish(p, &cc).await;
    let c2 = admit(&c, "refill").await;
    denied(p, c2, worker()).await;
    let offered = ticket(p, b1.company).await;
    assert!(offered.1.is_some());
    finish(p, &ca).await; // Older A now eligible; B still owns the opportunity.
    denied(p, a2, worker()).await;
    assert_eq!(ticket(p, b1.company).await, offered);
    assert!(ticket(p, a.company).await.1.is_none());
    let quiet = p.claim_io(b1, bw, lease()).await.unwrap().unwrap();
    finish(p, &quiet).await;
    finish(p, &cd).await;
    assert!(p.claim_io(a2, worker(), lease()).await.unwrap().is_some());
}

#[tokio::test]
async fn workflow_fairness_abandoned_offer_and_stale_withdrawal_are_bounded() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let quiet = tenant(&f).await;
    let noise = tenant(&f).await;
    let p = f.persistence();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let ca = p.claim_io(a, worker(), lease()).await.unwrap().unwrap();
    let cb = p
        .claim_io(admit(&b, "owner").await, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    let q = admit(&quiet, "abandoned").await;
    let qw = worker();
    denied(p, q, qw).await;
    let old = ticket(p, q.company).await.0;
    let n = admit(&noise, "next").await;
    denied(p, n, worker()).await;
    p.withdraw_demand(worker(), DemandTicket(old))
        .await
        .unwrap();
    assert_eq!(ticket(p, q.company).await.0, old);
    finish(p, &ca).await;
    denied(p, n, worker()).await;
    sqlx::query("UPDATE workflow_dispatch_demand SET opportunity_until=clock_timestamp()-interval '1 second' WHERE ticket=$1")
        .bind(old).execute(p.pool()).await.unwrap();
    denied(p, n, worker()).await; // Retire expired offer, never recycle its old age.
    let cn = p.claim_io(n, worker(), lease()).await.unwrap().unwrap();
    denied(p, q, qw).await;
    let replacement = ticket(p, q.company).await.0;
    assert!(replacement > old);
    p.withdraw_demand(qw, DemandTicket(old)).await.unwrap();
    assert_eq!(ticket(p, q.company).await.0, replacement);
    finish(p, &cn).await;
    finish(p, &cb).await;
    assert!(p.claim_io(q, qw, lease()).await.unwrap().is_some());
}

#[tokio::test]
async fn workflow_fairness_worker_traversals_and_finite_horizons_are_independent() {
    let (f, first) = fixture().await;
    let other = tenant(&f).await;
    let second = admit(&other, "other").await;
    let p = f.persistence();
    let workers = [worker(), worker()];
    for w in workers {
        let a = p.poll_fair(w, 1).await.unwrap();
        let b = p.poll_fair(w, 1).await.unwrap();
        let mut found = vec![a.candidates[0].scope.company, b.candidates[0].scope.company];
        found.sort_by_key(|company| company.as_uuid());
        let mut expected = vec![first.company, second.company];
        expected.sort_by_key(|company| company.as_uuid());
        assert_eq!(found, expected);
    }
    // New work at EXACT horizon is deferred; it cannot fill the current epoch.
    let new = admit(&f, "new").await;
    sqlx::query("UPDATE background_tasks SET created_at=(SELECT horizon FROM workflow_poll_companies WHERE worker_id=$2 AND company_id=$3) WHERE id=$1")
        .bind(new.job.0).bind(workers[0].0).bind(first.company.as_uuid()).execute(p.pool()).await.unwrap();
    // Select this exact tenant and remove the UUID cursor as an alternative filter.
    sqlx::query("UPDATE workflow_poll_workers SET after_company=$2 WHERE worker_id=$1")
        .bind(workers[0].0)
        .bind(second.company.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE workflow_poll_companies SET after_job=NULL WHERE worker_id=$1 AND company_id=$2",
    )
    .bind(workers[0].0)
    .bind(first.company.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    let epoch = p.poll_fair(workers[0], 128).await.unwrap();
    assert!(!epoch.candidates.is_empty());
    assert!(
        epoch
            .candidates
            .iter()
            .all(|c| c.scope.company == first.company)
    );
    assert!(!epoch.candidates.iter().any(|c| c.scope == new));
    let mut seen = false;
    for _ in 0..8 {
        seen |= p
            .poll_fair(workers[0], 128)
            .await
            .unwrap()
            .candidates
            .iter()
            .any(|c| c.scope == new);
    }
    assert!(seen);
    // Simultaneously revive two stale cursor owners; cleanup must skip their locks.
    sqlx::query("UPDATE workflow_poll_workers SET touched_at=clock_timestamp()-interval '2 days'")
        .execute(p.pool())
        .await
        .unwrap();
    let barrier = Barrier::new(2);
    let poll = |w| {
        let barrier = &barrier;
        async move {
            barrier.wait().await;
            p.poll_fair(w, 1).await
        }
    };
    let (a, b) = tokio::join!(poll(workers[0]), poll(workers[1]));
    assert!(a.is_ok() && b.is_ok());
}

#[path = "fairness_worker_tests.rs"]
mod worker_tests;

#[tokio::test]
async fn workflow_fairness_invalid_offer_is_cleaned_before_older_invalid_history() {
    let (f, a) = fixture().await;
    let b = tenant(&f).await;
    let quiet = tenant(&f).await;
    let noise = tenant(&f).await;
    let newer = tenant(&f).await;
    let p = f.persistence();
    // Reserve historical ticket numbers without constructing unrelated executions.
    sqlx::query("SELECT setval(pg_get_serial_sequence('workflow_dispatch_demand','ticket'),100)")
        .execute(p.pool())
        .await
        .unwrap();
    p.configure_capacity(WorkflowCapacityPolicy::new(2, 1).unwrap())
        .await
        .unwrap();
    let ca = p.claim_io(a, worker(), lease()).await.unwrap().unwrap();
    p.claim_io(admit(&b, "owner").await, worker(), lease())
        .await
        .unwrap()
        .unwrap();
    let q = admit(&quiet, "will-be-ineligible").await;
    denied(p, q, worker()).await;
    let n = admit(&noise, "abandoned-next").await;
    denied(p, n, worker()).await;
    finish(p, &ca).await;
    denied(p, n, worker()).await;
    assert!(ticket(p, q.company).await.1.is_some());
    // Simulate retained metadata whose companies have been deleted. The offered
    // invalid row follows more than the cleanup batch; all rows satisfy the schema.
    for old in 1..=33_i64 {
        sqlx::query("INSERT INTO workflow_dispatch_demand (company_id,ticket,worker_id,run_id,execution_id,job_id) OVERRIDING SYSTEM VALUE VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(Uuid::new_v4()).bind(old).bind(Uuid::new_v4()).bind(Uuid::new_v4())
            .bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(p.pool()).await.unwrap();
    }
    sqlx::query(
        "UPDATE background_tasks SET run_at=clock_timestamp()+interval '1 hour' WHERE id=$1",
    )
    .bind(q.job.0)
    .execute(p.pool())
    .await
    .unwrap();
    let fresh = admit(&newer, "newer").await;
    denied(p, fresh, worker()).await; // Must not unique-violate and roll back cleanup.
    assert!(ticket(p, n.company).await.1.is_some());
    sqlx::query("UPDATE workflow_dispatch_demand SET opportunity_until=clock_timestamp()-interval '1 second' WHERE company_id=$1")
        .bind(n.company.as_uuid()).execute(p.pool()).await.unwrap();
    denied(p, fresh, worker()).await;
    assert!(
        p.claim_io(fresh, worker(), lease())
            .await
            .unwrap()
            .is_some()
    );
}
