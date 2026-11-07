use super::*;
use crate::entities::correlation::CorrelationId;

#[tokio::test]
async fn channel_reply_binding_rechecks_current_run_as_private_channel_visibility() {
    let s = Intents::new().await;
    let run_as = member(&s.fixture, "admin").await;
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(run_as)
        .execute(s.pool())
        .await
        .unwrap();
    let binding = s.binding(MessageTrigger::Any, Some(run_as)).await;
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE company_id=$1 AND id=$2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(s.channel)
        .execute(s.pool())
        .await
        .unwrap();
    assert!(
        s.p()
            .configure_channel_reply(selection(&s, binding, None).await)
            .await
            .is_err()
    );
    assert!(
        s.p()
            .channel_reply_settings(
                s.fixture.target.company,
                RelatedChannelId::new(s.channel),
                s.owner()
            )
            .await
            .unwrap()
            .is_none()
    );
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(s.fixture.target.company.as_uuid())
            .bind(run_as)
            .fetch_one(s.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) \
        VALUES($1,$2,$3,'view','manager')")
        .bind(s.fixture.target.company.as_uuid()).bind(s.channel).bind(principal).execute(s.pool()).await.unwrap();
    let selected = select(&s, binding).await;
    sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id=$3")
        .bind(s.fixture.target.company.as_uuid()).bind(s.channel).bind(principal).execute(s.pool()).await.unwrap();
    let mut change = selection(&s, binding, Some(selected.revision)).await;
    change.responder.as_mut().unwrap().contract = literal_contract("Changed");
    assert!(s.p().configure_channel_reply(change).await.is_err());
    assert_eq!(
        s.p()
            .channel_reply_settings(
                s.fixture.target.company,
                RelatedChannelId::new(s.channel),
                s.owner()
            )
            .await
            .unwrap()
            .unwrap(),
        selected
    );
}

#[tokio::test]
async fn channel_reply_binding_inactive_before_ingress_has_visible_refusal_until_selection_cleared()
{
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let selected = select(&s, binding).await;
    s.deactivate(binding).await;
    let refused = s.intent(TO).await;
    assert_eq!(shape(&s, refused).await, (1, 0));
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, refused).await, (1, 0));
    let outcomes = s.outcomes(refused).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[&binding.as_uuid()].0, "refused");
    assert!(s.runs(binding).await.is_empty());
    s.p()
        .configure_channel_reply(ConfigureChannelReply {
            company: s.fixture.target.company,
            channel: RelatedChannelId::new(s.channel),
            actor: s.owner(),
            expected: Some(selected.revision),
            responder: None,
            approval_policy: None,
        })
        .await
        .unwrap();
    let unselected = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, unselected).await, (0, 0));
    assert!(s.outcomes(unselected).await.is_empty());
}

#[tokio::test]
async fn channel_reply_binding_competing_ingress_claimants_create_one_owned_run() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent(TO).await;
    let service = s.admissions(admission(s.p()));
    let (first, second) = tokio::join!(
        service.claim(MessageIntentWorkerId(Uuid::new_v4()), 1),
        service.claim(MessageIntentWorkerId(Uuid::new_v4()), 1)
    );
    let claimed: Vec<_> = first.unwrap().into_iter().chain(second.unwrap()).collect();
    assert_eq!(claimed.len(), 1);
    service
        .process(claimed.into_iter().next().unwrap())
        .await
        .unwrap();
    assert_eq!(shape(&s, intent).await, (1, 1));
    assert_eq!(s.runs(binding).await.len(), 1);
}

#[tokio::test]
async fn channel_reply_binding_thread_scoped_binding_cannot_own_channel_responses() {
    let s = Intents::new().await;
    let target = s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
    let mut request = s.configuration(target, None, None, Some(MessageTrigger::Any));
    request.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(s.channel),
        thread_id: RelatedThreadId::new(s.thread),
    };
    let state = s.bindings().configure(request).await.unwrap();
    s.bindings()
        .activate(SetBindingActivity {
            target,
            expected: state.revision,
        })
        .await
        .unwrap();
    assert!(
        s.p()
            .configure_channel_reply(selection(&s, target.binding, None).await)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_reply_binding_manual_root_has_no_automatic_response_authority() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent(TO).await;
    let request = AdmitWorkflowRequest {
        company_id: s.fixture.target.company,
        actor: s.owner(),
        association: RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(s.channel),
            thread_id: RelatedThreadId::new(s.thread),
        },
        trigger: TriggerRef::new(
            s.fixture.target.company,
            TriggerId::new(Uuid::new_v4()),
            TriggerSource::Manual,
        )
        .unwrap(),
        correlation_id: CorrelationId::new(),
        binding_id: binding,
        idempotency_key: IdempotencyKey::parse("manual-with-selected-channel").unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: DeliveryPolicy::InAppOnly,
        expected_binding_revision: None,
        admission_claim: None,
    };
    let AdmissionResult::Created(run) = admission(s.p()).admit(request).await.unwrap() else {
        panic!("manual root");
    };
    assert_eq!(shape(&s, intent).await, (1, 0));
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_reply_runs WHERE run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(s.pool())
        .await
        .unwrap();
    assert_eq!(links, 0);
}

#[tokio::test]
async fn channel_reply_binding_context_insert_and_source_acceptance_roll_back_together() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let message = s.message().await;
    let intent = Uuid::new_v4();
    let mut tx = s.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO workflow_message_intents(company_id,id,message_id,channel_id,thread_id,role,\
        mentioned,admissible,origin,hop_count,trace_channels,delivery_policy,correlation_id) \
        VALUES($1,$2,$3,$4,$5,'to',false,true,'external',0,'{}','deliver',$2)",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(intent)
    .bind(message.as_uuid())
    .bind(s.channel)
    .bind(s.thread)
    .execute(&mut *tx)
    .await
    .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_reply_contexts WHERE intent_id=$1")
            .bind(intent)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 1);
    tx.rollback().await.unwrap();
    assert_eq!(shape(&s, intent).await, (0, 0));
}

#[tokio::test]
async fn channel_reply_binding_observer_run_cannot_replace_response_owner() {
    let s = Intents::new().await;
    let owner = s.binding(MessageTrigger::Any, None).await;
    let observer = s.binding(MessageTrigger::Any, None).await;
    select(&s, owner).await;
    let intent = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    let observer_run = s.runs(observer).await[0];
    let mut tx = s.pool().begin().await.unwrap();
    let refused = sqlx::query(
        "INSERT INTO channel_reply_runs(company_id,intent_id,run_id,binding_id,binding_revision) \
        VALUES($1,$2,$3,$4,1)",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(intent)
    .bind(observer_run)
    .bind(owner.as_uuid())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(
        refused.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
    assert_eq!(shape(&s, intent).await, (1, 1));
}
