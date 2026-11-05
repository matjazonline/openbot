use super::*;

async fn bounded(pool: &sqlx::PgPool) -> Transaction<'_, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='100ms'")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

pub(super) async fn prepare<'a>(
    f: &'a Fixture,
    request: &PublishAgent,
) -> (Transaction<'a, Postgres>, PublishedAgentRevision) {
    let mut tx = begin_authorized(&f.p, f.scope).await.unwrap();
    let (draft, definition) = capture::capture_on(&mut tx, f.scope).await.unwrap();
    let result = storage::insert_revision_on(&mut tx, f.scope, draft, definition)
        .await
        .unwrap();
    storage::activate_on(&mut tx, request, &result)
        .await
        .unwrap();
    (tx, result)
}

#[tokio::test]
async fn autonomous_agent_publication_capture_serializes_before_actual_skill_deletion() {
    let f = Fixture::new().await;
    let skill = f.skill(vec![]).await;
    let request = f.request("publish-first").await;
    let (publication, result) = prepare(&f, &request).await;
    let assignment = result.definition.skills[0].assignment_id;
    let mut deletion = bounded(f.p.pool()).await;
    let error = sqlx::query("DELETE FROM skills WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(skill.id)
        .execute(&mut *deletion)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    deletion.rollback().await.unwrap();
    publication.commit().await.unwrap();
    f.p.delete_company(f.scope.company.as_uuid(), skill.id)
        .await
        .unwrap();
    let old =
        f.p.published_agent_revision(f.scope, result.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(old.digest, result.digest);
    assert_eq!(
        old.definition.skills[0].skill.instructions,
        "Original shared text."
    );
    let live: Option<Uuid> =
        sqlx::query_scalar("SELECT live_skill_id FROM agent_skill_assignments WHERE id=$1")
            .bind(assignment.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(live, None);
    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM agent_skill_grants WHERE assignment_id=$1",
    )
    .bind(assignment.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert!(revoked);
}

#[tokio::test]
async fn autonomous_agent_publication_actual_skill_deletion_serializes_before_capture() {
    let f = Fixture::new().await;
    let skill = f.skill(vec![]).await;
    let assignment: Uuid =
        sqlx::query_scalar("SELECT assignment_id FROM agent_skills WHERE agent_id=$1")
            .bind(f.scope.agent.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    let request = f.request("delete-first").await;
    let mut deletion = f.p.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM skills WHERE company_id=$1 AND id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(skill.id)
        .execute(&mut *deletion)
        .await
        .unwrap();
    let mut publication = begin_authorized(&f.p, f.scope).await.unwrap();
    assert!(matches!(
        capture::capture_on(&mut publication, f.scope).await,
        Err(AppError::DatabaseTimeout(_))
    ));
    publication.rollback().await.unwrap();
    deletion.commit().await.unwrap();
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
    let result = f.p.publish_agent(request).await.unwrap();
    assert!(result.definition.skills.is_empty());
    let grants: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_skill_grants WHERE assignment_id=$1")
            .bind(assignment)
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(grants, 0);
}

#[tokio::test]
async fn autonomous_agent_publication_capture_holds_draft_and_shared_text_until_commit() {
    let f = Fixture::new().await;
    let skill = f.skill(vec![]).await;
    let request = f.request("captured").await;
    let (publication, result) = prepare(&f, &request).await;
    let mut edit = bounded(f.p.pool()).await;
    let error = sqlx::query("UPDATE agents SET system_prompt='Concurrent draft',draft_revision=draft_revision+1 WHERE id=$1")
        .bind(f.scope.agent.as_uuid()).execute(&mut *edit).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    edit.rollback().await.unwrap();
    let mut text = bounded(f.p.pool()).await;
    let error = sqlx::query("UPDATE skills SET instructions='Concurrent shared text' WHERE id=$1")
        .bind(skill.id)
        .execute(&mut *text)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    text.rollback().await.unwrap();
    publication.commit().await.unwrap();
    let old =
        f.p.published_agent_revision(f.scope, result.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(old.definition.instructions, "Original agent instructions.");
    assert_eq!(
        old.definition.skills[0].skill.instructions,
        "Original shared text."
    );
}

#[tokio::test]
async fn autonomous_agent_publication_draft_edit_first_refuses_stale_expected_state() {
    let f = Fixture::new().await;
    let request = f.request("stale-draft").await;
    let mut edit = f.p.pool().begin().await.unwrap();
    sqlx::query("UPDATE agents SET system_prompt='Concurrent draft',draft_revision=draft_revision+1 WHERE id=$1")
        .bind(f.scope.agent.as_uuid()).execute(&mut *edit).await.unwrap();
    assert!(matches!(
        f.p.publish_agent(request.clone()).await,
        Err(AppError::DatabaseTimeout(_))
    ));
    edit.commit().await.unwrap();
    assert!(matches!(
        f.p.publish_agent(request).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 0,
            requests: 0
        }
    );
    let current =
        f.p.publish_agent(f.request("fresh-draft").await)
            .await
            .unwrap();
    assert_eq!(current.definition.instructions, "Concurrent draft");
}
