//! A genuine finality-breach conflict row rejects UPDATE and DELETE through its own native
//! immutable trigger (migration 20260930180000:122), leaving every public row unchanged.
use super::*;

async fn reject(f: &AdmissionFixture, sql: &str, company: Uuid) {
    let before = all_tables(f).await;
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let error = sqlx::query(sql)
        .bind(company)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    let db_error = error.as_database_error().unwrap();
    assert_eq!(db_error.code().as_deref(), Some("P0001"), "{sql}");
    assert_eq!(db_error.constraint(), None, "{sql}");
    assert_eq!(
        db_error.message(),
        "workflow action effect fact is append-only",
        "{sql}"
    );
    tx.rollback().await.unwrap();
    assert_eq!(all_tables(f).await, before, "all public rows after {sql}");
}

#[tokio::test]
async fn workflow_action_reconciliation_conflict_row_rejects_update_and_delete() {
    let mut h = Box::pin(held_truth()).await;
    Box::pin(schedule_final(&h, "conflict-immutable-final")).await;
    let before = all_tables(&h.fixture).await;
    assert_eq!(before["workflow_action_evidence_conflicts"], json!([]));
    assert!(matches!(
        adapter(&h.fixture, 0)
            .finish_remote(h.entry.take().unwrap(), h.result.clone())
            .await
            .unwrap(),
        RemoteDispatchObservation::Interrupted
    ));
    let after = all_tables(&h.fixture).await;
    retained_truth(&before, &after, h.entry_id, &h.result);
    assert_eq!(
        after["workflow_action_evidence_conflicts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // The only row-level BEFORE UPDATE/DELETE guard on the table is the M:122 trigger.
    let guards: Vec<(String, String)> = sqlx::query_as(
        "SELECT tgname::text,tgfoid::regproc::text FROM pg_trigger \
         WHERE tgrelid='workflow_action_evidence_conflicts'::regclass AND NOT tgisinternal \
         AND tgenabled='O' AND tgtype&3=3 AND tgtype&24<>0 ORDER BY 1",
    )
    .fetch_all(h.fixture.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        guards,
        [(
            "workflow_action_evidence_conflict_immutable".to_owned(),
            "workflow_action_fact_immutable".to_owned()
        )]
    );
    let company = h.request.scope().company.as_uuid();
    for sql in [
        "UPDATE workflow_action_evidence_conflicts SET created_at=created_at+interval '1 second' WHERE company_id=$1",
        "UPDATE workflow_action_evidence_conflicts SET company_id=company_id WHERE company_id=$1",
        "DELETE FROM workflow_action_evidence_conflicts WHERE company_id=$1",
    ] {
        reject(&h.fixture, sql, company).await;
    }
    assert_eq!(all_tables(&h.fixture).await, after);
    h.fixture.persistence().pool().close().await;
}
