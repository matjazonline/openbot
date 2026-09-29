use super::*;

#[tokio::test]
async fn workflow_control_authority_lock_allows_runtime_fk_and_fences_owner_mutation() {
    let (f, scope) = fixture().await;
    let claim = f
        .persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let mut tx = f.persistence().pool().begin().await.unwrap();
    crate::adapters::persistence::workflow::authority::authorize_company(
        &mut tx,
        scope.company,
        f.binding.target.actor,
    )
    .await
    .unwrap();
    let result = f
        .persistence()
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output: json!({"items":[],"token_count":0}),
        })
        .await
        .unwrap();
    assert!(result.is_some());
    for sql in [
        "UPDATE companies SET name=name WHERE id=$1",
        "DELETE FROM companies WHERE id=$1",
    ] {
        let mut contender = f.persistence().pool().begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='100ms'")
            .execute(&mut *contender)
            .await
            .unwrap();
        let error = sqlx::query(sql)
            .bind(scope.company.as_uuid())
            .execute(&mut *contender)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55P03")
        );
        contender.rollback().await.unwrap();
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_control_receipt_delete_and_deferred_fault_preserve_all_writes() {
    let (f, scope) = fixture().await;
    f.persistence()
        .claim_io(scope, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    let before = snapshot(&f).await;
    sqlx::query("CREATE FUNCTION control_test_fault() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'control fault'; END $$")
        .execute(f.persistence().pool()).await.unwrap();
    sqlx::query("CREATE CONSTRAINT TRIGGER control_test_fault AFTER INSERT ON workflow_control_commands DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION control_test_fault()")
        .execute(f.persistence().pool()).await.unwrap();
    let cmd = command(&f, scope, "cancel").await;
    assert!(f.persistence().cancel(cmd.clone()).await.is_err());
    assert_eq!(snapshot(&f).await, before);
    assert_eq!(receipts(&f).await, json!([]));
    sqlx::query("DROP TRIGGER control_test_fault ON workflow_control_commands")
        .execute(f.persistence().pool())
        .await
        .unwrap();
    f.persistence().cancel(cmd).await.unwrap();
    let saved = receipts(&f).await;
    assert!(
        sqlx::query("DELETE FROM workflow_control_commands WHERE company_id=$1")
            .bind(scope.company.as_uuid())
            .execute(f.persistence().pool())
            .await
            .is_err()
    );
    assert_eq!(receipts(&f).await, saved);
}

#[tokio::test]
async fn workflow_control_child_admission_competes_and_existing_child_replays() {
    let (f, scope) = fixture().await;
    let parent = ExecutionRef::new(
        scope.company,
        scope.run,
        scope.execution,
        StepId::parse("start").unwrap(),
    );
    let trigger = TriggerRef::new(
        scope.company,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Child {
            parent: ChildCause::Execution(parent),
        },
    )
    .unwrap();
    let child = f.prepare(f.request("child", trigger)).await;
    let cancel = command(&f, scope, "cancel").await;
    let barrier = Barrier::new(2);
    let admission = async {
        barrier.wait().await;
        f.persistence().admit(&child).await
    };
    let control = async {
        barrier.wait().await;
        f.persistence().cancel(cancel).await.unwrap()
    };
    let (admitted, cancelled) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(admission, control)
    })
    .await
    .unwrap();
    assert!(matches!(cancelled, CancelResult::Applied { .. }));
    match admitted {
        Ok(AdmissionResult::Created(id)) => assert_eq!(
            f.persistence().admit(&child).await.unwrap(),
            AdmissionResult::Replayed(id)
        ),
        Err(AppError::Conflict(_)) => assert!(matches!(
            f.persistence().admit(&child).await,
            Err(AppError::Conflict(_))
        )),
        other => panic!("unexpected admission {other:?}"),
    }
}
