use super::*;
use crate::adapters::persistence::workflow::knowledge;
use crate::application::workflow::{actions::*, message::*};
use crate::entities::access_level::AccessLevel;

async fn raise(setup: &Setup, level: AccessLevel) {
    let scope = setup.scope();
    let mut tx = setup.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, level)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
fn refused(outcome: WorkflowHandlerOutcome) {
    let WorkflowHandlerOutcome::Completed(Err(failure)) = outcome else {
        panic!("knowledge refusal must complete with a terminal failure")
    };
    assert_eq!(
        failure.failure.code().as_str(),
        crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
    );
    assert_eq!(failure.failure.class(), FailureClass::Terminal);
}
#[tokio::test]
async fn knowledge_run_level_step_email_and_fanout_refuse_before_any_effect() {
    for targets in [vec![To::User], vec![To::Channel, To::User]] {
        let setup = Box::pin(send(&targets)).await;
        raise(&setup, AccessLevel::L5).await;
        refused(setup.runtime().execute(&setup.kind, &setup.claim).await);
        let counts = rows(setup.pool(), setup.scope().run).await;
        assert_eq!(counts.markers, 0);
        assert_eq!(counts.deliveries, 0);
        assert_eq!(counts.messages, 0);
        assert_eq!(counts.receipts, 0);
    }
}
#[tokio::test]
async fn knowledge_run_level_pending_public_intent_cannot_send_after_content_rises() {
    let setup = Box::pin(send(&[To::User])).await;
    let request = setup
        .fixture
        .persistence
        .message_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let service = ActionService::new(setup.fixture.persistence.clone());
    let original = service.prepare_step(request.clone()).await.unwrap();
    raise(&setup, AccessLevel::L5).await;
    let replay = service.prepare_step(request).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.invocation, original.invocation);
    refused(setup.runtime().execute(&setup.kind, &setup.claim).await);
    assert_eq!(rows(setup.pool(), setup.scope().run).await.markers, 0);
}
#[tokio::test]
async fn knowledge_run_level_internal_message_stamps_current_content_and_completed_replay_retains_stamp()
 {
    let setup = Box::pin(send(&[To::Channel])).await;
    raise(&setup, AccessLevel::L5).await;
    let first = completed(setup.runtime().execute(&setup.kind, &setup.claim).await).unwrap();
    let stamps:Vec<i16>=sqlx::query_scalar("SELECT message.access_level::smallint FROM workflow_message_destinations AS destination JOIN messages AS message ON (message.company_id,message.id)=(destination.company_id,destination.message_id) WHERE destination.run_id=$1")
        .bind(setup.scope().run.as_uuid()).fetch_all(setup.pool()).await.unwrap();
    assert_eq!(stamps, vec![5]);
    raise(&setup, AccessLevel::L7).await;
    let replay = completed(setup.runtime().execute(&setup.kind, &setup.claim).await).unwrap();
    assert_eq!(replay, first);
    assert_eq!(rows(setup.pool(), setup.scope().run).await.messages, 1);
    let stamp:i16=sqlx::query_scalar("SELECT access_level::smallint FROM messages WHERE id=(SELECT message_id FROM workflow_message_destinations WHERE run_id=$1 LIMIT 1)")
        .bind(setup.scope().run.as_uuid()).fetch_one(setup.pool()).await.unwrap();
    assert_eq!(stamp, 5);
}
#[tokio::test]
async fn knowledge_run_level_tool_refusal_is_typed_and_contains_no_content() {
    let setup = Box::pin(send(&[To::Channel])).await;
    raise(&setup, AccessLevel::L5).await;
    let error = tool(
        &setup,
        "private-call",
        send_arguments(&[("user", setup.user)], "never release this body"),
    )
    .await
    .err()
    .expect("tool is refused");
    assert!(matches!(
        error,
        AppError::KnowledgeRelease {
            content: AccessLevel::L5,
            destination: AccessLevel::L0
        }
    ));
    assert!(!error.to_string().contains("never release"));
    assert!(!error.to_string().contains('@'));
    // A later safe tool remains usable under the same live fence.
    assert!(matches!(
        tool(
            &setup,
            "safe-call",
            send_arguments(&[("channel", setup.other)], "safe internal")
        )
        .await
        .unwrap(),
        MessageObservation::Committed(_)
    ));
}
#[tokio::test]
async fn knowledge_run_level_verified_member_email_uses_current_member_level() {
    let setup = Box::pin(send(&[To::User])).await;
    let company = setup.scope().company.as_uuid();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(setup.user)
        .execute(setup.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO participant_identities(id,company_id,principal_id,transport,namespace,subject,status,claim_metadata,provenance) SELECT $1,$2,principal.id,'email','email',account.email,'verified','{\"version\":1,\"kind\":\"account\"}'::jsonb,'account' FROM principals AS principal JOIN users AS account ON account.id=principal.user_id WHERE principal.company_id=$2 AND principal.user_id=$3")
        .bind(Uuid::new_v4()).bind(company).bind(setup.user).execute(setup.pool()).await.unwrap();
    raise(&setup, AccessLevel::L5).await;
    assert!(matches!(
        setup.runtime().execute(&setup.kind, &setup.claim).await,
        WorkflowHandlerOutcome::Suspended(_)
    ));
    assert_eq!(rows(setup.pool(), setup.scope().run).await.deliveries, 1);
}

#[tokio::test]
async fn knowledge_run_level_sql_freeze_constraint_matches_pure_release_matrix() {
    let setup = Box::pin(send(&[To::Channel])).await;
    let p = &setup.fixture.persistence;
    let request = p
        .message_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let source = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap()
        .invocation
        .as_uuid();
    for content in AccessLevel::ALL {
        for destination in AccessLevel::ALL {
            let id = Uuid::new_v4();
            let digest = format!("{:032x}{:032x}", id.as_u128(), source.as_u128());
            let mut tx = setup.pool().begin().await.unwrap();
            let result=sqlx::query("INSERT INTO workflow_action_intents(company_id,run_id,execution_id,id,operation_key,operation_digest,argument_digest,idempotency_key,operation,policy_decision,content_level_at_freeze,destination_level_at_freeze,origin_kind,agent_manifest_id,agent_id,agent_revision_id,agent_binding_id) SELECT company_id,run_id,execution_id,$2,operation_key,$3,argument_digest,'workflow-action:v1:'||$3,operation,policy_decision,$4::smallint,$5::smallint,origin_kind,agent_manifest_id,agent_id,agent_revision_id,agent_binding_id FROM workflow_action_intents WHERE id=$1")
            .bind(source).bind(id).bind(digest).bind(content.as_i16()).bind(destination.as_i16()).execute(&mut *tx).await;
            assert_eq!(
                result.is_ok(),
                crate::entities::access_level::may_release(content, destination),
                "content={content:?}, destination={destination:?}"
            );
            if let Err(error) = result {
                assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("23514")
                );
            }
            tx.rollback().await.unwrap();
        }
    }
}

async fn verified_route_recipient(setup: &Setup) -> String {
    let company = setup.scope().company.as_uuid();
    let mut tx = setup.pool().begin().await.unwrap();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(setup.user)
        .execute(&mut *tx)
        .await
        .unwrap();
    let email: String = sqlx::query_scalar(
        "UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1 RETURNING email::text",
    )
    .bind(setup.user)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(company)
            .bind(setup.user)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    let identity = crate::application::ports::thread::qualified_email_identity(&email).unwrap();
    crate::adapters::persistence::participant::upsert_account_identity_on(
        &mut tx,
        company,
        crate::entities::transport::PrincipalId::new(principal),
        &identity,
        "recipient",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    email
}
async fn member_route_wait(observer: &mut PgConnection, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(2),async{
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity AS activity WHERE $1=ANY(pg_blocking_pids(activity.pid)) AND activity.wait_event_type='Lock' AND activity.query LIKE 'SELECT principal.id, account.email FROM company_members%')")
                .bind(blocker).fetch_one(&mut *observer).await.unwrap();
            if waiting{return;}
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.expect("publication reaches its real member route lock wait");
}
#[tokio::test]
async fn knowledge_run_level_race_verified_address_change_cannot_authorize_a_frozen_retired_recipient()
 {
    let setup = Box::pin(send(&[To::User])).await;
    let old = verified_route_recipient(&setup).await;
    raise(&setup, AccessLevel::L5).await;
    let mut gate = setup.pool().begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM company_members WHERE company_id=$1 AND user_id=$2 FOR UPDATE")
        .bind(setup.scope().company.as_uuid())
        .bind(setup.user)
        .execute(&mut *gate)
        .await
        .unwrap();
    let runtime = setup.runtime();
    let publish = runtime.execute(&setup.kind, &setup.claim);
    tokio::pin!(publish);
    let mut observer = setup.pool().acquire().await.unwrap();
    tokio::select! {
        _=member_route_wait(&mut observer,blocker)=>{},
        _=publish.as_mut()=>panic!("publication must not escape its actual member route gate"),
    }
    // The confirmed-address writer's real atomic mutations: account row, old-identity
    // retirement, new verified identity and historic adoption. It does not lock company/member.
    let replacement = format!("replacement-{}@example.test", Uuid::new_v4().simple());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut change = setup.pool().begin().await.unwrap();
        sqlx::query("UPDATE users SET email=$2,email_verified_at=clock_timestamp() WHERE id=$1")
            .bind(setup.user)
            .bind(&replacement)
            .execute(&mut *change)
            .await
            .unwrap();
        crate::adapters::persistence::account_identity::rekey_account_identity_on(
            &mut change,
            setup.user,
            &old,
            &replacement,
        )
        .await
        .unwrap();
        change.commit().await.unwrap();
    })
    .await
    .expect("address writer commits while publication is paused on the recipient member");
    gate.rollback().await.unwrap();
    // The route query keeps its earlier MVCC snapshot/address; the release query must check
    // that exact prepared recipient against current proof rather than bless the new address.
    refused(publish.await);
    let counts = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(counts.markers, 0);
    assert_eq!(counts.deliveries, 0);
    assert_eq!(counts.messages, 0);
}

async fn recovery_setup() -> Setup {
    let fixture = Fixture::new().await;
    let channel = new_channel(&fixture, "runs").await;
    let other = new_channel(&fixture, "notices").await;
    let user = member(&fixture, "member").await;
    let owner = fixture.target.actor.user_id();
    let mut source: Value =
        serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
    source["steps"]["start"]["with"] = json!({"content":{"literal":{"body":"restricted body"}},
        "destinations":{"literal":[{"kind":"user","id":user}]}});
    source["steps"]["start"]["routes"]["error"] = json!("recover");
    source["steps"]["recover"] = json!({"type":"data.map","with":{"value":{"literal":42},
        "output_schema":{"literal":{"type":"integer"}}},"routes":{"success":"$end"}});
    let directory = step_builtins::StepBuiltinPublicationDirectory {
        persistence: fixture.persistence.clone(),
    };
    let definitions = DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
        directory,
    );
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    let target = Box::pin(bound(&fixture, publication.version)).await;
    let claim = Box::pin(claimed(&fixture, target, channel)).await;
    Setup {
        fixture,
        channel,
        other,
        user,
        owner,
        claim,
        kind: crate::application::workflow::polling::WorkflowStepKind("message.send".into()),
    }
}

async fn assert_failed_attention(setup: &Setup) {
    use crate::application::attention::AttentionPersistence;
    use crate::entities::attention::{AttentionQuery, AttentionSourceKind, AttentionView};
    use crate::entities::transport::PrincipalId;
    let scope = setup.scope();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(scope.company.as_uuid())
            .bind(setup.owner)
            .fetch_one(setup.pool())
            .await
            .unwrap();
    let items = setup
        .fixture
        .persistence
        .list_attention(AttentionQuery {
            company_id: scope.company.as_uuid(),
            principal_id: PrincipalId::new(principal),
            visible_channel_ids: &[setup.channel],
            manager: true,
            company_items: true,
            view: AttentionView::MyWork,
            cursor: None,
            limit: 100,
        })
        .await
        .unwrap()
        .items;
    assert!(
        items
            .iter()
            .any(|item| item.source_kind == AttentionSourceKind::RunFailure
                && item.run_id == Some(scope.run.as_uuid()))
    );
    let state: Value = sqlx::query_scalar("SELECT jsonb_build_object('state',run.state,'terminal',run.terminal_execution_id,
        'executions',(SELECT count(*) FROM workflow_executions WHERE company_id=run.company_id AND run_id=run.id),
        'code',(SELECT workflow_failure_code FROM task_attempts WHERE task_id=$3 AND attempt_number=$4))
        FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.job.0).bind(setup.claim.fence.attempt.0)
        .fetch_one(setup.pool()).await.unwrap();
    assert_eq!(
        state,
        json!({"state":"failed","terminal":scope.execution.as_uuid(),"executions":1,
        "code":crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT})
    );
    let effects = rows(setup.pool(), scope.run).await;
    assert_eq!(effects.markers, 0);
    assert_eq!(effects.messages, 0);
    assert_eq!(effects.receipts, 0);
    assert_eq!(effects.deliveries, 0);
}

#[tokio::test]
async fn knowledge_run_level_refused_message_cannot_recover_into_success() {
    let setup = Box::pin(recovery_setup()).await;
    raise(&setup, AccessLevel::L5).await;
    let WorkflowHandlerOutcome::Completed(Err(report)) =
        setup.runtime().execute(&setup.kind, &setup.claim).await
    else {
        panic!("real message action must refuse")
    };
    assert_eq!(
        report.failure.code().as_str(),
        crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
    );
    assert!(
        setup
            .fixture
            .persistence
            .release_io(
                setup.claim.fence,
                lease(),
                LeaseReleaseCause::Classified(report)
            )
            .await
            .unwrap()
    );
    assert_failed_attention(&setup).await;
}
