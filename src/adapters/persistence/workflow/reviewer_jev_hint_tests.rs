//! A real message-admitted JEV run needs its effect actor, not future final recipients.
use super::*;
use crate::adapters::persistence::workflow::tests::jev_tests as jev_fixture;
use crate::application::workflow::jev::*;

struct JevReply {
    fixture: FinalFixture,
    claim: ClaimedWorkflow,
    connection: JevConnection,
}

async fn binding(s: &mut Intents, connection: &JevConnection) -> WorkflowBindingId {
    let mut source: Value =
        serde_json::from_str(&registry::example("jev.evaluate").unwrap().source).unwrap();
    source["input_schema"] = crate::application::workflow::triggers::MessageTriggerInput::schema();
    s.fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    source["workflow_id"] = json!(s.fixture.target.workflow.to_string());
    s.version = Box::pin(publish_root(s, source, FinalPublication::Standard)).await;
    let target = s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
    let mut configure = s.configuration(target, None, None, Some(MessageTrigger::Any));
    configure.resources.insert(
        ResourceName::parse("service").unwrap(),
        RuntimeResourceId::new(connection.id),
    );
    let state = s.bindings().configure(configure).await.unwrap();
    s.bindings()
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    target.binding
}

async fn fixture() -> JevReply {
    let base = Box::pin(jev_fixture::fixture()).await;
    let connection = jev_fixture::create(&base).await;
    let mut s = Box::pin(Intents::in_fixture(base)).await;
    let retired = member(&s.fixture, "admin").await;
    let reviewer = s.fixture.principal(retired).await;
    sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
        .execute(s.pool())
        .await
        .unwrap();
    let binding = Box::pin(binding(&mut s, &connection)).await;
    let mut request = selection(&s, binding, None).await;
    request.responder.as_mut().unwrap().contract = literal_contract("Complete");
    request.approval_policy = Some(ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap());
    s.p().configure_channel_reply(request).await.unwrap();
    let intent = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    let fixture = Box::pin(FinalFixture::from_intent(s, intent)).await;
    let policy = LeasePolicy::new(std::time::Duration::from_secs(60)).unwrap();
    let claim = fixture
        .p
        .claim_io(fixture.scope, WorkflowWorkerId(Uuid::new_v4()), policy)
        .await
        .unwrap()
        .unwrap();
    fixture.p.delete_user_account(retired).await.unwrap();
    JevReply {
        fixture,
        claim,
        connection,
    }
}

async fn accounting(f: &JevReply) -> Value {
    let scope = f.claim.fence.scope;
    sqlx::query_scalar("SELECT jsonb_build_object('calls',usage.model_calls, \
        'usage',(SELECT count(*) FROM model_usage_calls WHERE company_id=usage.company_id AND root_run_id=usage.root_run_id), \
        'invocations',(SELECT count(*) FROM workflow_jev_invocations WHERE company_id=usage.company_id AND run_id=usage.root_run_id), \
        'finals',(SELECT count(*) FROM channel_reply_proposals WHERE company_id=usage.company_id AND run_id=usage.root_run_id)) \
        FROM workflow_root_budget_usage AS usage WHERE company_id=$1 AND root_run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.fixture.p.pool()).await.unwrap()
}

#[tokio::test]
async fn workflow_reviewer_follow_reply_jev_authorization_and_admission_ignore_unused_retired_final_reviewers()
 {
    let f = Box::pin(fixture()).await;
    let p = &f.fixture.p;
    let credential = p.authorize_jev(&f.claim).await.unwrap();
    assert_eq!(credential.connection_id, f.connection.id);
    assert_eq!(credential.model.as_str(), "jev-1.14.0");
    let mut stale = f.claim.clone();
    stale.fence.generation = WorkflowGeneration(Uuid::new_v4());
    assert!(matches!(
        p.authorize_jev(&stale).await,
        Err(AppError::Execution(_))
    ));
    let policy = LeasePolicy::new(std::time::Duration::from_secs(60)).unwrap();
    let request = JevRequest::prepare(&f.claim.activation.inputs).unwrap();
    assert!(matches!(
        p.admit_jev(&f.claim, &request, policy).await.unwrap(),
        JevAdmissionResult::New(_)
    ));
    assert!(matches!(
        p.admit_jev(&f.claim, &request, policy).await.unwrap(),
        JevAdmissionResult::AlreadyAdmitted
    ));
    let before = accounting(&f).await;
    assert_eq!(
        before,
        json!({"calls":1,"usage":1,"invocations":1,"finals":0})
    );
    let disabled = p
        .update_jev(
            f.fixture.scope.company,
            f.fixture.s.owner(),
            f.connection.id,
            JevConnectionUpdate {
                expected_revision: f.connection.revision,
                enabled: false,
                models: f.connection.models.clone(),
            },
        )
        .await
        .unwrap();
    assert!(!disabled.enabled);
    assert!(
        matches!(p.authorize_jev(&f.claim).await, Err(AppError::BadRequest(ref message))
        if message=="Selected TypeSafe connection or model is unavailable")
    );
    assert!(
        matches!(p.admit_jev(&f.claim, &request, policy).await, Err(AppError::BadRequest(ref message))
        if message=="Selected TypeSafe connection or model is unavailable")
    );
    assert_eq!(
        accounting(&f).await,
        before,
        "revocation cannot repeat or grant model admission"
    );
}
