use super::*;

#[tokio::test]
async fn direct_binding_admission_rechecks_external_source_after_public_policy_revocation() {
    let direct = DirectIntents::new().await;
    let binding = direct.binding().await;
    let p = direct.s.p();
    let company = direct.s.fixture.target.company.as_uuid();
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(direct.principal.as_uuid())
        .execute(direct.s.pool())
        .await
        .unwrap();
    let accepted = direct
        .ingest_from(direct.target(), "outside@sender.test".into())
        .await;
    let source_thread = accepted.thread.unwrap().id;
    sqlx::query("UPDATE principals SET open_to_public=false WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(direct.principal.as_uuid())
        .execute(direct.s.pool())
        .await
        .unwrap();
    direct.s.drain(&direct.s.admissions(admission(p))).await;
    assert!(
        direct.s.runs(binding).await.is_empty(),
        "a previously accepted source is not current admission authority"
    );
    assert!(direct.s.admitted(accepted.intent_ids[0]).await.is_empty());
    let outcomes = direct.s.outcomes(accepted.intent_ids[0]).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[&binding.as_uuid()].0, "refused");
    let retained: Uuid =
        sqlx::query_scalar("SELECT principal_id FROM threads WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(source_thread)
            .fetch_one(direct.s.pool())
            .await
            .unwrap();
    assert_eq!(retained, direct.principal.as_uuid());

    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(direct.principal.as_uuid())
        .execute(direct.s.pool())
        .await
        .unwrap();
    let current = direct
        .ingest_from(direct.target(), "outside@sender.test".into())
        .await;
    direct.s.drain(&direct.s.admissions(admission(p))).await;
    assert_eq!(direct.s.runs(binding).await.len(), 1);
    assert_eq!(
        direct.s.admitted(current.intent_ids[0]).await,
        ids(&[binding])
    );
    assert!(direct.s.admitted(accepted.intent_ids[0]).await.is_empty());
}
