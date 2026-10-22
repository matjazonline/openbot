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
async fn workflow_admit_token_child_admission_refused_and_runtime_child_shares_root_policy() {
    // Category C (07-D1 §D1.2a): a child is never admitted, so its reviewers are never checked
    // at admission. A runtime child spends under the root's token policy through its budget link.
    let (f, _) = configured().await;
    let root = f.prepare(f.request("root", f.manual())).await;
    f.persistence().admit(&root).await.unwrap();
    let trigger = TriggerRef::new(
        root.company_id(),
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Step(ChildStepRef::new(
                ExecutionRef::new(
                    root.company_id(),
                    root.proposed_run_id(),
                    root.first_execution_id(),
                    root.entry().clone(),
                ),
                ChildOrdinal::FIRST,
            )),
        },
    )
    .unwrap();
    let before = f.counts().await;
    let p = f.persistence().clone();
    let refused = WorkflowService::new(p.clone(), p, NoRuntime, NoRuntime, Preflight)
        .admit(f.request("child", trigger))
        .await;
    assert!(
        matches!(refused, Err(AppError::BadRequest(_))),
        "{refused:?}"
    );
    assert_eq!(f.counts().await, before);

    let fixture = Fixture::new().await;
    let reviewer = member(&fixture, "admin").await;
    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["limits"]["root_budget"] = json!({
        "activations": 10, "model_calls": 10, "repetitions": 10,
        "tokens": {"max_tokens": 2000, "reviewers": [{"kind":"user","id":reviewer}]}
    });
    let (f, scopes) = AdmissionFixture::chain_in(fixture, source, 1).await;
    let (root, child) = (scopes[0], scopes[1]);
    let (root_id, reviewers): (Uuid, Value) = sqlx::query_as(
        "SELECT link.root_run_id, budget.token_policy->'reviewers' FROM workflow_run_budgets AS link \
         JOIN workflow_root_budgets AS budget ON budget.company_id=link.company_id AND budget.root_run_id=link.root_run_id \
         WHERE link.company_id=$1 AND link.run_id=$2",
    )
    .bind(child.company.as_uuid())
    .bind(child.run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    assert_eq!(root_id, root.run.as_uuid());
    assert_eq!(reviewers, json!([{"kind":"user","id":reviewer}]));
    let budgets: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_root_budgets")
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    assert_eq!(budgets, 1);
}
