use super::*;

#[derive(sqlx::FromRow)]
struct NoticeAssignment {
    principal_id: Uuid,
    notice_delivery_id: Uuid,
}

async fn foreign_company(f: &DecisionFixture) -> (Uuid, Uuid) {
    let company = Uuid::new_v4();
    let principal = Uuid::new_v4();
    sqlx::query("INSERT INTO companies(id,name,slug,user_id) VALUES($1,'Foreign',$2,$3)")
        .bind(company)
        .bind(format!("foreign-{company}"))
        .bind(f.admission.binding.target.actor.user_id())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role) VALUES($1,$2,$3,'admin')")
        .bind(Uuid::new_v4())
        .bind(company)
        .bind(f.users[0])
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) VALUES($1,$2,'person',$3,'Foreign')")
        .bind(principal).bind(company).bind(f.users[0]).execute(f.persistence().pool()).await.unwrap();
    (company, principal)
}

fn constraint(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().unwrap().constraint(),
        Some(expected)
    );
}
fn guard(error: sqlx::Error) {
    let error = error.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("23514"));
    assert!(error.message().contains("workflow decision"), "{error}");
}
async fn notice(tx: &mut Transaction<'_, Postgres>, decision: Uuid, principal: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    let key = format!(
        "notification:workflow-decision:{decision}:principal:{principal}:email:{id}@example.test"
    );
    sqlx::query("INSERT INTO message_deliveries(id,external_destination,correlation_id,transport,purpose,idempotency_key,max_attempts) VALUES($1,'member@example.test',$2,'email','notification',$3,3)")
        .bind(id).bind(Uuid::new_v4()).bind(key).execute(&mut **tx).await.unwrap();
    id
}

#[tokio::test]
async fn workflow_decision_cross_tenant_relationships_and_access() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let (foreign, foreign_principal) = foreign_company(&f).await;
    assert!(matches!(
        f.persistence()
            .read_decision(CompanyId::new(foreign), f.actor(0), view.decision)
            .await,
        Err(AppError::NotFound(_))
    ));
    let local_principal: Uuid = sqlx::query_scalar(
        "SELECT principal_id FROM workflow_decision_reviewers WHERE decision_id=$1",
    )
    .bind(view.decision.0)
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    for (company, author, expected) in [
        (
            foreign,
            foreign_principal,
            Some("workflow_decision_comments_decision_fkey"),
        ),
        (f.scope.company.as_uuid(), foreign_principal, None),
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = sqlx::query("INSERT INTO workflow_decision_comments(company_id,decision_id,sequence,author_principal_id,command_key,body) VALUES($1,$2,1,$3,'foreign','foreign')")
            .bind(company).bind(view.decision.0).bind(author).execute(&mut *tx).await.unwrap_err();
        match expected {
            Some(expected) => constraint(error, expected),
            None => guard(error),
        }
        tx.rollback().await.unwrap();
    }
    for (company, principal, expected) in [
        (
            foreign,
            foreign_principal,
            Some("workflow_decision_reviewers_decision_fkey"),
        ),
        (f.scope.company.as_uuid(), foreign_principal, None),
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let delivery = notice(&mut tx, view.decision.0, principal).await;
        let error = sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,principal_id,ordinal,notice_delivery_id) VALUES($1,$2,$3,1,$4)")
            .bind(company).bind(view.decision.0).bind(principal).bind(delivery).execute(&mut *tx).await.unwrap_err();
        match expected {
            Some(expected) => constraint(error, expected),
            None => guard(error),
        }
        tx.rollback().await.unwrap();
    }
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let error = sqlx::query("INSERT INTO workflow_decisions(company_id,run_id,execution_id,id,wait_id,subject_kind,proposal_digest) SELECT $1,run_id,execution_id,$2,wait_id,subject_kind,proposal_digest FROM workflow_decisions WHERE id=$3")
        .bind(foreign).bind(Uuid::new_v4()).bind(view.decision.0).execute(&mut *tx).await.unwrap_err();
    constraint(error, "workflow_decisions_wait_fkey");
    tx.rollback().await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let error = sqlx::query("UPDATE workflow_decisions SET submitted_by=$2,submitted_at=clock_timestamp(),command_key='foreign',command_fingerprint=repeat('0',64),choice='accept',feedback='',data='null',revision=2 WHERE id=$1")
        .bind(view.decision.0).bind(foreign_principal).execute(&mut *tx).await.unwrap_err();
    guard(error);
    tx.rollback().await.unwrap();
    assert_ne!(local_principal, foreign_principal);
}

#[tokio::test]
async fn workflow_decision_notice_cross_company_and_unique_link() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let view = f.park().await;
    let (_, foreign_principal) = foreign_company(&f).await;
    let rows: Vec<NoticeAssignment> = sqlx::query_as("SELECT principal_id,notice_delivery_id FROM workflow_decision_reviewers WHERE decision_id=$1 ORDER BY ordinal")
        .bind(view.decision.0).fetch_all(f.persistence().pool()).await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let foreign_notice = notice(&mut tx, Uuid::new_v4(), foreign_principal).await;
    sqlx::query("DELETE FROM workflow_decision_reviewers WHERE decision_id=$1 AND principal_id=$2")
        .bind(view.decision.0)
        .bind(rows[0].principal_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,principal_id,ordinal,notice_delivery_id) VALUES($1,$2,$3,0,$4)")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(rows[0].principal_id).bind(foreign_notice).execute(&mut *tx).await.unwrap_err();
    guard(error);
    tx.rollback().await.unwrap();
    // Isolate the unique-link constraint from the earlier provenance guard: even a
    // corrupted provenance predicate cannot admit one delivery assigned twice.
    let mut tx = f.persistence().pool().begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE workflow_decision_reviewers DISABLE TRIGGER workflow_decision_reviewer_guard",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("DELETE FROM workflow_decision_reviewers WHERE decision_id=$1 AND principal_id=$2")
        .bind(view.decision.0)
        .bind(rows[0].principal_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,principal_id,user_id,ordinal,notice_delivery_id) VALUES($1,$2,$3,$4,0,$5)")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(rows[0].principal_id).bind(f.users[0]).bind(rows[1].notice_delivery_id).execute(&mut *tx).await.unwrap_err();
    constraint(error, "workflow_decision_reviewer_notice");
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_decision_notice_rejects_non_notification_independently() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let row: NoticeAssignment = sqlx::query_as("SELECT principal_id,notice_delivery_id FROM workflow_decision_reviewers WHERE decision_id=$1")
        .bind(view.decision.0).fetch_one(f.persistence().pool()).await.unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    // Fault-inject just the delivery's purpose, keeping the exact decision/principal key.
    // Isolate this guard from the delivery table's own standalone-shape constraint.
    sqlx::query(
        "ALTER TABLE message_deliveries DROP CONSTRAINT message_deliveries_attribution_check",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("UPDATE message_deliveries SET purpose='reply' WHERE id=$1")
        .bind(row.notice_delivery_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workflow_decision_reviewers WHERE decision_id=$1")
        .bind(view.decision.0)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,principal_id,ordinal,notice_delivery_id) VALUES($1,$2,$3,0,$4)")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(row.principal_id).bind(row.notice_delivery_id).execute(&mut *tx).await.unwrap_err();
    guard(error);
    tx.rollback().await.unwrap();
}
