use super::super::message_tests::support::member;
use super::*;

async fn configured() -> (AdmissionFixture, Uuid) {
    let fixture = Fixture::new().await;
    let reviewer = member(&fixture, "admin").await;
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({
        "activations": 10, "model_calls": 10, "repetitions": 10,
        "tokens": {"max_tokens": 2000, "reviewers": [{"kind":"user","id":reviewer}]}
    });
    let binding = BindingFixture::from_fixture(fixture, source).await;
    (AdmissionFixture::with_binding(binding).await, reviewer)
}

#[tokio::test]
async fn workflow_admit_token_reviewers_current_authority_and_saved_replay() {
    let (f, reviewer) = configured().await;
    let command = f.prepare(f.request("first", f.manual())).await;
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Created(command.proposed_run_id())
    );
    let next = f.prepare(f.request("next", f.manual())).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(command.company_id().as_uuid())
        .bind(reviewer)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&next).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Replayed(command.proposed_run_id())
    );
    let policy: Value = sqlx::query_scalar(
        "SELECT token_policy FROM workflow_root_budgets WHERE company_id=$1 AND root_run_id=$2",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.proposed_run_id().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(
        policy,
        json!({"max_tokens":2000,"reviewers":[{"kind":"user","id":reviewer}]})
    );
}

#[tokio::test]
async fn workflow_admit_token_reviewer_removal_competes_before_commit() {
    let (f, reviewer) = configured().await;
    let command = f.prepare(f.request("removed", f.manual())).await;
    let mut revoked = f.persistence().pool().begin().await.unwrap();
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(command.company_id().as_uuid())
        .bind(reviewer)
        .execute(&mut *revoked)
        .await
        .unwrap();
    admission_authority_tests::rejected_after_commit(&f, &command, revoked).await;
}

#[tokio::test]
async fn workflow_admit_token_reviewer_requires_current_person_identity() {
    let (f, reviewer) = configured().await;
    let command = f.prepare(f.request("missing-person", f.manual())).await;
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(command.company_id().as_uuid())
        .bind(reviewer)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&command).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
}

#[tokio::test]
async fn workflow_admit_token_child_inherits_root_reviewers_and_rechecks_them() {
    let (f, reviewer) = configured().await;
    let root = f.prepare(f.request("root", f.manual())).await;
    f.persistence().admit(&root).await.unwrap();
    let parent = ExecutionRef::new(
        root.company_id(),
        root.proposed_run_id(),
        root.first_execution_id(),
        root.entry().clone(),
    );
    let trigger = TriggerRef::new(
        root.company_id(),
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(parent),
        },
    )
    .unwrap();
    let child = f.prepare(f.request("child", trigger.clone())).await;
    f.persistence().admit(&child).await.unwrap();
    let root_id: Uuid = sqlx::query_scalar(
        "SELECT root_run_id FROM workflow_run_budgets WHERE company_id=$1 AND run_id=$2",
    )
    .bind(child.company_id().as_uuid())
    .bind(child.proposed_run_id().as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(root_id, root.proposed_run_id().as_uuid());
    let next_trigger = TriggerRef::new(
        root.company_id(),
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(ExecutionRef::new(
                child.company_id(),
                child.proposed_run_id(),
                child.first_execution_id(),
                child.entry().clone(),
            )),
        },
    )
    .unwrap();
    let next = f.prepare(f.request("next-child", next_trigger)).await;
    sqlx::query("UPDATE company_members SET role='member' WHERE company_id=$1 AND user_id=$2")
        .bind(root.company_id().as_uuid())
        .bind(reviewer)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().admit(&next).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
    assert_eq!(
        f.persistence().admit(&child).await.unwrap(),
        AdmissionResult::Replayed(child.proposed_run_id())
    );
}
