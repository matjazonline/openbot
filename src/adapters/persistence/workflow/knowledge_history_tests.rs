use super::*;
use crate::entities::access_level::AccessLevel;

#[tokio::test]
async fn knowledge_run_level_history_filters_before_count_and_raises_returned_content() {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 65_536)).await)
            .await;
    let c = Conversation::new(&f).await;
    let member =
        super::super::super::message_tests::support::member(&f.binding.fixture, "admin").await;
    let company = f.binding.target.company.as_uuid();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(member)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
        .bind(company).bind(c.channel).bind(c.thread).bind(member).execute(f.persistence().pool()).await.unwrap();
    let visible = super::super::knowledge_admission_tests::stamped(&f, &c, AccessLevel::L5).await;
    let hidden = super::super::knowledge_admission_tests::stamped(&f, &c, AccessLevel::L8).await;
    let claim = claimed(&f, &c, "classification-history").await;
    let output = history(&f, &claim).await;
    assert_eq!(ids(&output).len(), 2);
    assert!(ids(&output).contains(&visible));
    assert!(!ids(&output).contains(&hidden));
    let eligible: i64 =
        sqlx::query_scalar("SELECT eligible_count FROM workflow_history_snapshots WHERE run_id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(eligible, 2);
    let content: i16 =
        sqlx::query_scalar("SELECT content_level::smallint FROM workflow_runs WHERE id=$1")
            .bind(claim.fence.scope.run.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(content, 5);
    let error = sqlx::query("UPDATE messages SET access_level=4 WHERE id=$1")
        .bind(visible)
        .execute(f.persistence().pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some("workflow_guard:message_access_level")
    );
}

#[tokio::test]
async fn knowledge_run_level_raise_rolls_back_with_its_read_transaction() {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("atomic-read", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    super::super::super::super::knowledge::raise_content_level_on(
        &mut tx,
        command.company_id(),
        command.proposed_run_id(),
        AccessLevel::L5,
    )
    .await
    .unwrap();
    let in_tx: i16 =
        sqlx::query_scalar("SELECT content_level::smallint FROM workflow_runs WHERE id=$1")
            .bind(command.proposed_run_id().as_uuid())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(in_tx, 5);
    tx.rollback().await.unwrap();
    let after: i16 =
        sqlx::query_scalar("SELECT content_level::smallint FROM workflow_runs WHERE id=$1")
            .bind(command.proposed_run_id().as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(after, 0);
}
