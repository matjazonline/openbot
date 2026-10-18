use super::*;

#[tokio::test]
async fn workflow_decision_comment_bounds_and_immutable_facts() {
    let f = Box::pin(DecisionFixture::new(1, None)).await;
    let view = f.park().await;
    let mut comment = AddDecisionComment {
        company: f.scope.company,
        actor: f.actor(0),
        decision: view.decision,
        command_key: DecisionCommandKey::parse("boundary").unwrap(),
        body: String::new(),
    };
    assert!(matches!(
        f.persistence().add_decision_comment(comment.clone()).await,
        Err(AppError::BadRequest(_))
    ));
    comment.body = "é".repeat(MAX_COMMENT_CHARS + 1);
    assert!(matches!(
        f.persistence().add_decision_comment(comment.clone()).await,
        Err(AppError::BadRequest(_))
    ));
    sqlx::query("INSERT INTO workflow_decision_comments(company_id,decision_id,sequence,author_principal_id,command_key,body) SELECT $1,$2,sequence,(SELECT id FROM principals WHERE company_id=$1 AND user_id=$3),'seed-'||sequence,'seed' FROM generate_series(1,999) AS sequence")
        .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(view.reviewers[0]).execute(f.persistence().pool()).await.unwrap();
    comment.body = "é".repeat(MAX_COMMENT_CHARS);
    assert_eq!(
        f.persistence()
            .add_decision_comment(comment.clone())
            .await
            .unwrap()
            .sequence,
        1000
    );
    comment.command_key = DecisionCommandKey::parse("over-limit").unwrap();
    assert!(matches!(
        f.persistence().add_decision_comment(comment).await,
        Err(AppError::Conflict(_))
    ));
    for statement in [
        "UPDATE workflow_decisions SET proposal_digest=repeat('0',64)",
        "UPDATE workflow_decision_reviewers SET ordinal=ordinal",
        "UPDATE workflow_decision_comments SET body='changed'",
    ] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        let error = sqlx::query(statement).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        tx.rollback().await.unwrap();
    }
    let read = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(read.comments.len(), 1000);
    assert_eq!(read.comments[999].body.chars().count(), MAX_COMMENT_CHARS);
    assert_eq!(read.revision, 1);
}

#[derive(sqlx::FromRow)]
struct Assignment {
    principal_id: Uuid,
    notice_delivery_id: Uuid,
    ordinal: i16,
}

#[tokio::test]
async fn workflow_decision_reviewer_notice_provenance_rejects_foreign_delivery() {
    let f = Box::pin(DecisionFixture::new(2, None)).await;
    let view = f.park().await;
    let rows: Vec<Assignment> = sqlx::query_as("SELECT principal_id,notice_delivery_id,ordinal FROM workflow_decision_reviewers WHERE decision_id=$1 ORDER BY ordinal")
        .bind(view.decision.0).fetch_all(f.persistence().pool()).await.unwrap();
    let other = f
        .admission
        .prepare(f.admission.request("another", f.admission.manual()))
        .await;
    f.persistence().admit(&other).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(other.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    let scope = ActivationRequest {
        company: other.company_id(),
        run: other.proposed_run_id(),
        execution: other.first_execution_id(),
        job: WorkflowJobId(job),
    };
    f.persistence()
        .park_decision(scope, &f.notices)
        .await
        .unwrap()
        .unwrap();
    let other_notice: Uuid = sqlx::query_scalar("SELECT reviewer.notice_delivery_id FROM workflow_decision_reviewers AS reviewer JOIN workflow_decisions AS decision ON decision.company_id=reviewer.company_id AND decision.id=reviewer.decision_id WHERE decision.run_id=$1 LIMIT 1")
        .bind(scope.run.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    for invalid_notice in [rows[1].notice_delivery_id, other_notice] {
        let mut tx = f.persistence().pool().begin().await.unwrap();
        sqlx::query("DELETE FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 AND principal_id=$3")
            .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(rows[0].principal_id).execute(&mut *tx).await.unwrap();
        let error = sqlx::query("INSERT INTO workflow_decision_reviewers(company_id,decision_id,principal_id,ordinal,notice_delivery_id) VALUES($1,$2,$3,$4,$5)")
            .bind(f.scope.company.as_uuid()).bind(view.decision.0).bind(rows[0].principal_id).bind(rows[0].ordinal).bind(invalid_notice).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        tx.rollback().await.unwrap();
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_decision_reviewers WHERE decision_id=$1")
            .bind(view.decision.0)
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 2);
}
