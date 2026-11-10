//! A frozen preparation keeps the department context it froze (01-departments p03 B3,
//! 01DEP-P03-D11, G3-F1): `department_context_`.
use super::*;

/// The run's channel is owned by a department with `charter`.
async fn owned(f: &HistoryThread, claim: &ClaimedWorkflow, charter: &str) -> Uuid {
    let company = claim.fence.scope.company.as_uuid();
    let channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(f.s.pool())
            .await
            .unwrap();
    let department = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO departments (id, company_id, name, slug, charter, created_by) \
         VALUES ($1, $2, 'Sales', 'sales', $3, \
           '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}')",
    )
    .bind(department)
    .bind(company)
    .bind(charter)
    .execute(f.s.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE channels SET department_id=$1 WHERE company_id=$2 AND id=$3")
        .bind(department)
        .bind(company)
        .bind(channel)
        .execute(f.s.pool())
        .await
        .unwrap();
    department
}

/// G3-F1: the preamble is stored and the frozen history's budget counts its length, so a charter
/// edited between the first preparation and a retry -- a longer one here -- must neither fail
/// the retry's budget check nor its document comparison, and the retry sends what was frozen.
#[tokio::test]
async fn department_context_frozen_preparation_replays_the_department_it_froze() {
    let mut f = Box::pin(HistoryThread::new()).await;
    f.s.fixture.persistence =
        f.s.fixture
            .persistence
            .clone()
            .with_app_domain("agents.test");
    let claim = f.next().await;
    let department = owned(&f, &claim, "Win and keep customers.").await;
    let first =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .unwrap()
            .expect_ready();
    let preamble = first.prepared.preamble().to_owned();
    assert!(
        preamble.contains("Department: Sales (sales)."),
        "{preamble}"
    );
    assert!(preamble.contains("Charter:\nWin and keep customers."));
    assert!(preamble.contains("Desks around this department:"));
    assert!(first.prepared.frozen_history().is_some());
    let saved = frozen(&f, &claim).await;
    assert_eq!(saved["department"]["charter"], "Win and keep customers.");

    sqlx::query("UPDATE departments SET charter=repeat('Grow every account. ', 40) WHERE id=$1")
        .bind(department)
        .execute(f.s.pool())
        .await
        .unwrap();
    let retry =
        f.s.p()
            .prepare_agent(&claim, policy())
            .await
            .expect("the retry restores what it froze")
            .expect_ready();
    assert_eq!(retry.prepared.preamble(), preamble);
    assert_eq!(
        retry.prepared.spec(claim.fence).unwrap().prepared,
        first.prepared.spec(claim.fence).unwrap().prepared
    );
    assert_eq!(frozen(&f, &claim).await, saved);
}
