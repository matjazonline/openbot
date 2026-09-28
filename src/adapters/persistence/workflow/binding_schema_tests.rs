use super::*;

#[tokio::test]
async fn workflow_binding_schema_enforces_selected_revision_and_version_scope() {
    let fixture = Fixture::new().await;
    let service = fixture.service();
    service.save(fixture.save(None)).await.unwrap();
    let publication = fixture.publish();
    service.publish(publication.clone()).await.unwrap();
    let company = fixture.target.company.as_uuid();
    let binding = Uuid::new_v4();
    let pool = fixture.persistence.pool();
    let mut tx = pool.begin().await.unwrap();
    insert_head(&mut tx, company, binding).await;
    // A head without its revision fails at commit, including rollback of the head.
    assert!(
        tx.commit()
            .await
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    let missing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM workflow_bindings WHERE company_id = $1")
            .bind(company)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(missing, 0);
    let mut tx = pool.begin().await.unwrap();
    insert_head(&mut tx, company, binding).await;
    insert_revision(
        &mut tx,
        company,
        binding,
        publication.version.as_uuid(),
        fixture.target.actor.user_id(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let other = CompanyPersistence::create(
        &fixture.persistence,
        fixture.target.actor.user_id(),
        CompanyWrite {
            name: "Other".into(),
            slug: "other".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    insert_head(&mut tx, other.id, binding).await;
    let error = insert_revision(
        &mut tx,
        other.id,
        binding,
        publication.version.as_uuid(),
        fixture.target.actor.user_id(),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    tx.rollback().await.unwrap();
    let error = sqlx::query(
        "UPDATE workflow_bindings SET configuration_revision = 2 WHERE company_id = $1 AND id = $2",
    )
    .bind(company)
    .bind(binding)
    .execute(pool)
    .await
    .unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
}

async fn insert_head(tx: &mut Transaction<'_, Postgres>, company: Uuid, binding: Uuid) {
    sqlx::query("INSERT INTO workflow_bindings (company_id, id, state_revision, configuration_revision) VALUES ($1, $2, 1, 1)")
        .bind(company).bind(binding).execute(&mut **tx).await.unwrap();
}

async fn insert_revision(
    tx: &mut Transaction<'_, Postgres>,
    company: Uuid,
    binding: Uuid,
    version: Uuid,
    actor: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO workflow_binding_revisions (company_id, binding_id, revision, version_id, params, resources, actor_id) VALUES ($1, $2, 1, $3, '{}'::jsonb, '{}'::jsonb, $4)")
        .bind(company).bind(binding).bind(version).bind(actor).execute(&mut **tx).await?;
    Ok(())
}
