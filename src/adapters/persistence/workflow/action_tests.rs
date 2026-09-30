use super::*;
use crate::application::workflow::actions::tests::request as action_request;
use crate::application::workflow::actions::*;

async fn setup() -> (AdmissionFixture, ActionRequest) {
    let (fixture, execution) = fixture().await;
    fixture.persistence().activate(execution).await.unwrap();
    let request = action_request(ActionScope {
        company: execution.company,
        run: execution.run,
        execution: execution.execution,
    });
    (fixture, request)
}

async fn counts(f: &AdmissionFixture, request: &ActionRequest) -> (i64, i64) {
    let intents = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_intents WHERE company_id=$1 AND run_id=$2",
    )
    .bind(request.scope.company.as_uuid())
    .bind(request.scope.run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    let calls = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_action_model_calls WHERE company_id=$1 AND run_id=$2",
    )
    .bind(request.scope.company.as_uuid())
    .bind(request.scope.run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap();
    (intents, calls)
}

#[tokio::test]
async fn workflow_action_competing_step_and_tool_prepare_one_intent_restart_replays() {
    let (f, request) = setup().await;
    let step = ActionService::new(f.persistence().clone());
    let tool = ActionService::new(f.persistence().clone());
    let barrier = Barrier::new(2);
    let first = async {
        barrier.wait().await;
        step.prepare_step(request.clone()).await.unwrap()
    };
    let second = async {
        barrier.wait().await;
        tool.prepare_tool(
            request.clone(),
            ModelToolCallId::parse("model-call-1").unwrap(),
        )
        .await
        .unwrap()
    };
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.invocation, second.invocation);
    assert_eq!(first.idempotency_key, second.idempotency_key);
    assert_ne!(first.replayed, second.replayed);
    assert_eq!(counts(&f, &request).await, (1, 1));
    let fresh = ActionService::new(PostgresPersistence::new(f.persistence().pool().clone()));
    let saved = fresh
        .prepare_tool(request, ModelToolCallId::parse("model-call-1").unwrap())
        .await
        .unwrap();
    assert!(saved.replayed);
    assert_eq!(saved.approval_subject(), first.approval_subject());
    assert_eq!(saved.decision, ActionPolicyDecision::Unevaluated);
}

#[tokio::test]
async fn workflow_action_changed_arguments_need_new_call_and_context_changes_conflict() {
    let (f, original) = setup().await;
    let service = ActionService::new(f.persistence().clone());
    let first = service
        .prepare_tool(original.clone(), ModelToolCallId::parse("call-1").unwrap())
        .await
        .unwrap();
    let mut changed = original.clone();
    changed.arguments = json!({"value":2});
    assert!(
        service
            .prepare_tool(changed.clone(), ModelToolCallId::parse("call-1").unwrap())
            .await
            .is_err()
    );
    assert_eq!(counts(&f, &original).await, (1, 1));
    let second = service
        .prepare_tool(changed, ModelToolCallId::parse("call-2").unwrap())
        .await
        .unwrap();
    assert_ne!(first.invocation, second.invocation);
    assert_ne!(first.approval_subject(), second.approval_subject());
    assert_eq!(second.decision, ActionPolicyDecision::Unevaluated);
    let mut changed_context = original.clone();
    changed_context.context.approval_required = true;
    assert!(service.prepare_step(changed_context).await.is_err());
    assert_eq!(counts(&f, &original).await, (2, 2));
}

#[tokio::test]
async fn workflow_action_schema_fk_immutable_and_cancelled_new_intent_fail_closed() {
    let (f, original) = setup().await;
    let service = ActionService::new(f.persistence().clone());
    let mut protected = original.clone();
    protected.context.approval_required = true;
    let intent = service.prepare_step(protected.clone()).await.unwrap();
    assert_eq!(intent.decision, ActionPolicyDecision::ApprovalRequired);
    assert!(sqlx::query("UPDATE workflow_action_intents SET argument_digest=repeat('0',64) WHERE company_id=$1 AND id=$2")
        .bind(original.scope.company.as_uuid()).bind(intent.invocation.as_uuid()).execute(f.persistence().pool()).await.is_err());
    let mut foreign = original.clone();
    foreign.scope.execution = crate::domain::workflow::ExecutionId::new(Uuid::new_v4());
    assert!(service.prepare_step(foreign).await.is_err());
    let mut invalid = original.clone();
    invalid.arguments = json!({"value":"secret"});
    assert!(service.prepare_step(invalid).await.is_err());
    assert_eq!(counts(&f, &original).await, (1, 0));
    assert!(sqlx::query("INSERT INTO workflow_action_model_calls (company_id,run_id,execution_id,model_call_id,invocation_id,argument_digest) VALUES ($1,$2,$3,'foreign',$4,repeat('0',64))")
        .bind(Uuid::new_v4()).bind(original.scope.run.as_uuid()).bind(original.scope.execution.as_uuid())
        .bind(intent.invocation.as_uuid()).execute(f.persistence().pool()).await.is_err());
    sqlx::query("UPDATE workflow_runs SET state='cancelled' WHERE company_id=$1 AND id=$2")
        .bind(original.scope.company.as_uuid())
        .bind(original.scope.run.as_uuid())
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let replay = service.prepare_step(protected).await.unwrap();
    assert!(replay.replayed);
    let mut new = original;
    new.arguments = json!({"value":99});
    assert!(service.prepare_step(new).await.is_err());
}

#[tokio::test]
async fn workflow_action_numeric_jsonb_roundtrip_preserves_digest_and_replay() {
    let (f, mut request) = setup().await;
    request.contract.contract.input_schema = json!(true);
    request.contract.contract.output_schema =
        serde_json::from_str(r#"{"examples":[-0.0,1e2,1.0]}"#).unwrap();
    request.arguments = serde_json::from_str("[-0.0,1e2,1.0,1e20,-1e20,1e-10]").unwrap();
    let service = ActionService::new(f.persistence().clone());
    let first = service.prepare_step(request.clone()).await.unwrap();
    let replay = service.prepare_step(request.clone()).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(first.invocation, replay.invocation);
    assert_eq!(first.argument_digest, replay.argument_digest);
    request.arguments =
        serde_json::from_str("[0,100,1,100000000000000000000,-100000000000000000000,0.0000000001]")
            .unwrap();
    let equivalent = service
        .prepare_tool(request, ModelToolCallId::parse("numeric-call").unwrap())
        .await
        .unwrap();
    assert_eq!(first.invocation, equivalent.invocation);
    assert_eq!(first.idempotency_key, equivalent.idempotency_key);
}

#[tokio::test]
async fn workflow_action_authority_sql_scope_actor_and_current_membership() {
    let (f, mut request) = setup().await;
    request.context.actor = f.binding.target.actor.user_id();
    let service = ActionService::new(f.persistence().clone());
    let intent = service
        .prepare_tool(
            request.clone(),
            ModelToolCallId::parse("authority").unwrap(),
        )
        .await
        .unwrap();
    let subject = intent.approval_subject();
    let loaded = f
        .persistence()
        .action_authority(request.scope, &subject)
        .await
        .unwrap();
    assert_eq!(loaded.actor, f.binding.target.actor);
    assert_eq!(
        loaded.action.saved_operation(),
        &serde_json::to_value(&request).unwrap()
    );
    assert_eq!(loaded.bundle.company_id(), request.scope.company);
    for mode in 0..4 {
        let mut scope = request.scope;
        let mut wrong = subject.clone();
        match mode {
            0 => scope.company = CompanyId::new(Uuid::new_v4()),
            1 => scope.run = RunId::new(Uuid::new_v4()),
            2 => scope.execution = ExecutionId::new(Uuid::new_v4()),
            _ => wrong.invocation = ActionInvocationId::new(Uuid::new_v4()),
        }
        assert!(
            f.persistence()
                .action_authority(scope, &wrong)
                .await
                .is_err()
        );
    }
    sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
        .bind(request.scope.company.as_uuid())
        .bind(request.context.actor)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    assert!(matches!(
        f.persistence()
            .action_authority(request.scope, &subject)
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(counts(&f, &request).await, (1, 1));
}

#[tokio::test]
async fn workflow_action_authority_revocation_contender_blocks_then_rechecks() {
    let (f, mut request) = setup().await;
    request.context.actor = f.binding.target.actor.user_id();
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(request.clone())
        .await
        .unwrap();
    let mut revoke = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(request.scope.company.as_uuid())
        .execute(&mut *revoke)
        .await
        .unwrap();
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(request.scope.company.as_uuid())
        .bind(request.context.actor)
        .execute(&mut *revoke)
        .await
        .unwrap();
    let commit = async {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        revoke.commit().await.unwrap();
    };
    let subject = intent.approval_subject();
    let (_, result) = tokio::join!(
        commit,
        f.persistence().action_authority(request.scope, &subject)
    );
    assert!(matches!(result, Err(AppError::NotFound(_))));
    assert_eq!(counts(&f, &request).await, (1, 0));
}

#[tokio::test]
async fn workflow_action_authority_run_lock_wait_cannot_cross_deadline() {
    let (f, mut request) = setup().await;
    request.context.actor = f.binding.target.actor.user_id();
    let intent = ActionService::new(f.persistence().clone())
        .prepare_step(request.clone())
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_runs SET deadline=clock_timestamp()+interval '200 milliseconds' WHERE company_id=$1 AND id=$2")
        .bind(request.scope.company.as_uuid()).bind(request.scope.run.as_uuid()).execute(f.persistence().pool()).await.unwrap();
    let mut holder = f.persistence().pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM workflow_runs WHERE company_id=$1 AND id=$2 FOR UPDATE")
        .bind(request.scope.company.as_uuid())
        .bind(request.scope.run.as_uuid())
        .execute(&mut *holder)
        .await
        .unwrap();
    let release = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        holder.commit().await.unwrap();
    };
    let subject = intent.approval_subject();
    let (_, result) = tokio::join!(
        release,
        f.persistence().action_authority(request.scope, &subject)
    );
    assert!(matches!(result, Err(AppError::Conflict(_))));
}
