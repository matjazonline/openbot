//! Authentic source proof and immutable targeting stay enforced after source/intent loss.
use super::*;

async fn reject(f: &AgentCase, sql: &str) {
    let error = sqlx::query(sql)
        .bind(f.people.role.company().as_uuid())
        .execute(f.people.role.p().pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514"),
        "{sql}: {error}"
    );
}

#[tokio::test]
async fn workflow_escalation_origin_database_requires_authentic_complete_immutable_target_and_links()
 {
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    for sql in [
        "DELETE FROM workflow_escalation_notice_targets WHERE company_id=$1",
        "UPDATE workflow_escalation_notice_targets SET binding_id=gen_random_uuid() WHERE company_id=$1",
        "UPDATE workflow_escalation_notice_targets SET live_intent_id=NULL WHERE company_id=$1",
        "DELETE FROM workflow_message_notification_origins WHERE company_id=$1",
        "UPDATE workflow_message_notification_origins SET origin_id=gen_random_uuid() WHERE company_id=$1",
        "UPDATE workflow_notification_origins SET notice_id=NULL WHERE company_id=$1",
        "DELETE FROM workflow_notification_origins WHERE company_id=$1",
        "INSERT INTO workflow_notification_origins(company_id,id,notice_id) SELECT $1,value,value FROM (SELECT gen_random_uuid() AS value) AS forged",
        "INSERT INTO workflow_message_notification_origins(company_id,message_id,origin_id) SELECT company_id,gen_random_uuid(),notice_id FROM workflow_escalation_notice_targets WHERE company_id=$1",
        "INSERT INTO workflow_run_notification_origins(company_id,run_id,origin_id) SELECT target.company_id,escalation.run_id,target.notice_id FROM workflow_escalation_notice_targets AS target JOIN workflow_escalation_cases AS escalation ON escalation.company_id=target.company_id WHERE target.company_id=$1",
        "INSERT INTO workflow_escalation_notice_targets(company_id,notice_id,message_id,intent_id,live_intent_id,principal_id,agent_id,desk_id,binding_id,binding_revision) SELECT company_id,notice_id,gen_random_uuid(),intent_id,intent_id,principal_id,agent_id,desk_id,binding_id,binding_revision FROM workflow_escalation_notice_targets WHERE company_id=$1",
    ] {
        reject(&f, sql).await;
    }
    // Box the actual retention/intent-loss phase; it descends into storage owners.
    Box::pin(intent_loss(&f, source)).await;
}

async fn intent_loss(f: &AgentCase, source: RunId) {
    let p = f.people.role.p();
    let original: Value = sqlx::query_scalar(
        "SELECT to_jsonb(intent) FROM workflow_message_intents AS intent WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    retire(f, source).await;
    reject(
        f,
        "DELETE FROM workflow_escalation_notice_targets WHERE company_id=$1",
    )
    .await;
    reject(
        f,
        "DELETE FROM workflow_notification_origins WHERE company_id=$1",
    )
    .await;
    sqlx::query("DELETE FROM workflow_message_intents WHERE company_id=$1")
        .bind(f.people.role.company().as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let remains: bool = sqlx::query_scalar(
        "SELECT live_intent_id IS NULL FROM workflow_escalation_notice_targets WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        remains,
        "only the optional pointer was cleared by the actual intent deletion"
    );
    for payload in [original.clone(), {
        let mut replaced = original;
        replaced["id"] = json!(Uuid::new_v4());
        replaced
    }] {
        let error = sqlx::query("INSERT INTO workflow_message_intents SELECT copied.* FROM jsonb_populate_record(NULL::workflow_message_intents,$2) AS copied WHERE copied.company_id=$1")
            .bind(f.people.role.company().as_uuid()).bind(payload).execute(p.pool()).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("23514")
        );
    }
    assert_eq!(counts(f).await["targets"], 1);
    assert_eq!(counts(f).await["roots"], 0);
}

#[tokio::test]
async fn workflow_escalation_origin_existing_root_failure_after_source_retention_never_restarts_the_ladder()
 {
    use crate::application::app_error::WorkflowGuardViolation;
    use crate::application::workflow::guard::{
        GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
    };
    let (f, source) = Box::pin(pending_agent_from_run()).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    process(&f, claims(&f).await.pop().unwrap()).await;
    let run = RunId::new(
        sqlx::query_scalar(
            "SELECT run_id FROM workflow_escalation_handling_roots WHERE company_id=$1",
        )
        .bind(f.people.role.company().as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap(),
    );
    for sql in [
        "DELETE FROM workflow_escalation_handling_roots WHERE company_id=$1",
        "UPDATE workflow_escalation_handling_roots SET live_run_id=NULL WHERE company_id=$1",
        "UPDATE workflow_escalation_handling_roots SET run_id=gen_random_uuid() WHERE company_id=$1",
        "DELETE FROM workflow_run_notification_origins WHERE company_id=$1",
        "UPDATE workflow_run_notification_origins SET origin_id=gen_random_uuid() WHERE company_id=$1",
    ] {
        reject(&f, sql).await;
    }
    retire(&f, source).await;
    let scope = f.people.scope(run).await;
    assert_eq!(
        p.fail_run_on_guard(GuardFailure {
            scope,
            violation: WorkflowGuardViolation::from_constraint(
                "workflow_guard:test_escalation_guard",
                "forced guard"
            )
            .unwrap(),
            site: GuardSite::Step,
            fence: None,
        })
        .await
        .unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        counts(&f).await,
        json!({"runs":1,"cases":0,"receipts":0,"targets":1,"roots":1,"messages":1,"origins":1,"model_calls":0})
    );
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        0
    );
    retire(&f, run).await;
    assert_eq!(counts(&f).await["roots"], 1);
    assert_eq!(counts(&f).await["origins"], 1);
}

#[tokio::test]
async fn workflow_escalation_origin_missing_message_seal_aborts_real_fenced_completion_and_retries_atomically()
 {
    use crate::adapters::persistence::test_support::minted_agent_notice_key;
    use crate::application::notification::{
        NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
    };
    use crate::transport::WorkerId;
    let (f, _) = Box::pin(pending_agent_from_run()).await;
    let p = f.people.role.p();
    sqlx::raw_sql("CREATE SEQUENCE notice_completion_reached; \
        CREATE FUNCTION omit_message_origin() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; \
        CREATE TRIGGER aa_omit_message_origin BEFORE INSERT ON workflow_message_notification_origins \
          FOR EACH ROW EXECUTE FUNCTION omit_message_origin(); \
        CREATE FUNCTION mark_notice_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
          IF NEW.source_kind='escalation_notice' AND NEW.status='projected' \
            THEN PERFORM nextval('notice_completion_reached'); END IF; RETURN NEW; END $$; \
        CREATE TRIGGER zz_mark_notice_completion AFTER UPDATE ON notification_events \
          FOR EACH ROW EXECUTE FUNCTION mark_notice_completion();")
        .execute(p.pool()).await.unwrap();
    let claims = p
        .claim_notification_events(WorkerId::random(), std::time::Duration::from_secs(30), 1)
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    let projection = p
        .resolve_notification_projection(&claim.event)
        .await
        .unwrap();
    let key = minted_agent_notice_key(&projection);
    let request = || NotificationProjectionCommit {
        fence: &claim.lease,
        expected: &projection,
        email: None,
        agent_message_key: key.as_ref(),
        escalation_ack_hours: Default::default(),
    };
    let error = p
        .commit_notification_projection(request())
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("authentic notice must seal"),
        "{error:?}"
    );
    let reached: bool = sqlx::query_scalar("SELECT is_called FROM notice_completion_reached")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert!(
        reached,
        "the real event fence completed before deferred completeness rejected commit"
    );
    assert_eq!(
        counts(&f).await,
        json!({"runs":1,"cases":1,"receipts":0,"targets":0,"roots":0,"messages":0,"origins":0,"model_calls":0})
    );
    assert_eq!(notice_queue(&f).await["status"], "processing");
    sqlx::query("DROP TRIGGER aa_omit_message_origin ON workflow_message_notification_origins")
        .execute(p.pool())
        .await
        .unwrap();
    let (outcome, _) = p.commit_notification_projection(request()).await.unwrap();
    assert_eq!(outcome, NotificationProjectionCommitOutcome::Applied);
    assert_eq!(counts(&f).await["receipts"], 1);
    assert_eq!(counts(&f).await["targets"], 1);
    assert_eq!(counts(&f).await["origins"], 1);
}
