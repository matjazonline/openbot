//! The one-statement current audience boundary, including actual sender identity clearance.
use super::*;
use crate::adapters::persistence::workflow::{knowledge, knowledge_release};
use crate::application::workflow::{actions::*, message::MessageActionPreparation};
use crate::entities::access_level::AccessLevel;

async fn verified_sender(c: &Conversation) {
    let setup = &c.setup;
    let p = &setup.fixture.persistence;
    let company = setup.scope().company.as_uuid();
    let key = Uuid::new_v4().simple().to_string();
    let account = p.create_user(&key, CUSTOMER, "hash").await.unwrap();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(account.id)
        .execute(setup.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO company_members(id,company_id,user_id,role,access_level) VALUES($1,$2,$3,'member',5)").bind(Uuid::new_v4()).bind(company).bind(account.id).execute(setup.pool()).await.unwrap();
    // The final facts of accepting the observed sender as a member, retaining its historic party.
    sqlx::query("UPDATE principals SET kind='person',user_id=$2 WHERE company_id=$1 AND id IN (SELECT principal_id FROM participant_identities WHERE company_id=$1 AND subject=$3)")
        .bind(company).bind(account.id).bind(CUSTOMER).execute(setup.pool()).await.unwrap();
    sqlx::query("UPDATE participant_identities SET status='verified',provenance='account',claim_metadata='{\"version\":1,\"kind\":\"account\"}'::jsonb WHERE company_id=$1 AND subject=$2")
        .bind(company).bind(CUSTOMER).execute(setup.pool()).await.unwrap();
    let scope = setup.scope();
    let mut tx = setup.pool().begin().await.unwrap();
    knowledge::raise_content_level_on(&mut tx, scope.company, scope.run, AccessLevel::L5)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
async fn add_cc(c: &Conversation) {
    let principal = Uuid::new_v4();
    let setup = &c.setup;
    let company = setup.scope().company.as_uuid();
    let mut tx = setup.pool().begin().await.unwrap();
    sqlx::query(
        "INSERT INTO principals(id,company_id,kind,display_label) VALUES($1,$2,'external','CC')",
    )
    .bind(principal)
    .bind(company)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) VALUES($1,$2,$3,$4,'participant')").bind(company).bind(setup.channel).bind(c.thread).bind(principal).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn knowledge_run_level_reply_cc_added_after_intent_refuses_publication_without_partial_effect()
 {
    let c = Box::pin(conversation(
        |_, _, source, _, _| reply(source),
        Source::Question,
    ))
    .await;
    verified_sender(&c).await;
    let setup = &c.setup;
    let p = &setup.fixture.persistence;
    let request = p
        .message_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    add_cc(&c).await;
    let WorkflowHandlerOutcome::Completed(Err(failure)) =
        setup.runtime().execute(&setup.kind, &setup.claim).await
    else {
        panic!("CC must refuse the whole reply")
    };
    assert_eq!(
        failure.failure.code().as_str(),
        crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
    );
    let counts = rows(setup.pool(), setup.scope().run).await;
    assert_eq!(counts.markers, 0);
    assert_eq!(counts.messages, 0);
    assert_eq!(counts.deliveries, 0);
}
#[tokio::test]
async fn knowledge_run_level_race_cc_commit_after_destination_snapshot_is_ordered_after_that_freeze()
 {
    let c = Box::pin(conversation(
        |_, _, source, _, _| reply(source),
        Source::Question,
    ))
    .await;
    verified_sender(&c).await;
    let setup = &c.setup;
    let p = &setup.fixture.persistence;
    let request = p
        .message_step_request(&setup.claim, &setup.kind)
        .await
        .unwrap();
    let intent = ActionService::new(p.clone())
        .prepare_step(request)
        .await
        .unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT operation FROM workflow_action_intents WHERE id=$1")
            .bind(intent.invocation.as_uuid())
            .fetch_one(setup.pool())
            .await
            .unwrap();
    let action = FrozenAction::restore(saved).unwrap();
    let mut freeze = setup.pool().begin().await.unwrap();
    let facts = knowledge_release::evaluate_on(
        &mut freeze,
        &action,
        knowledge_release::MessageRecipients::Current,
    )
    .await
    .unwrap();
    assert_eq!(facts.destination, AccessLevel::L5);
    // A CC writer must commit while the freeze transaction still owns its run lock. It does not
    // take an inverse run lock, and the already-read destination is one stable MVCC fact group.
    tokio::time::timeout(std::time::Duration::from_secs(2), add_cc(&c))
        .await
        .unwrap();
    assert_eq!(facts.content, AccessLevel::L5);
    assert_eq!(facts.destination, AccessLevel::L5);
    freeze.commit().await.unwrap();
    let mut later = setup.pool().begin().await.unwrap();
    assert!(matches!(
        knowledge_release::evaluate_on(
            &mut later,
            &action,
            knowledge_release::MessageRecipients::Current
        )
        .await,
        Err(AppError::KnowledgeRelease {
            destination: AccessLevel::L0,
            ..
        })
    ));
    later.rollback().await.unwrap();
}
#[tokio::test]
async fn knowledge_run_level_reply_unverified_historical_address_stays_public() {
    let c = Box::pin(conversation(
        |_, _, source, _, _| reply(source),
        Source::Question,
    ))
    .await;
    verified_sender(&c).await;
    sqlx::query("UPDATE users SET email_verified_at=NULL WHERE email=$1")
        .bind(CUSTOMER)
        .execute(c.setup.pool())
        .await
        .unwrap();
    let WorkflowHandlerOutcome::Completed(Err(failure)) = c
        .setup
        .runtime()
        .execute(&c.setup.kind, &c.setup.claim)
        .await
    else {
        panic!("unverified address must not receive private content")
    };
    assert_eq!(
        failure.failure.code().as_str(),
        crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT
    );
    assert_eq!(
        rows(c.setup.pool(), c.setup.scope().run).await.deliveries,
        0
    );
}
