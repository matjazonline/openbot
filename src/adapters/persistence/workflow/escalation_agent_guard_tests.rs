use super::*;

pub(super) async fn reject_requested_binding(
    f: &AgentCase,
    intent: &crate::application::workflow::message_admission::ClaimedIntent,
    other: WorkflowBindingId,
) {
    let p = f.people.role.p();
    let trigger = TriggerRef::new(
        intent.claim.company,
        TriggerId::new(intent.claim.intent),
        TriggerSource::Message {
            message_id: intent.message,
        },
    )
    .unwrap();
    let mut request = f
        .people
        .role
        .admission
        .request("requested-unrelated-notice-binding", trigger);
    request.binding_id = other;
    request.association = RelatedAssociation::Thread {
        target: intent.target,
        thread_id: crate::application::workflow::RelatedThreadId::new(intent.thread),
    };
    request.input = p.trigger_input(intent).await.unwrap().to_value();
    request.admission_claim = Some(AdmissionClaim::Message(intent.claim));
    request.correlation_id = intent.correlation;
    request.delivery_policy = intent.delivery_policy;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let result = service.admit(request).await;
    assert!(
        matches!(result, Err(AppError::NotFound(_))),
        "a requested unrelated binding must not bypass captured-origin revalidation: {result:?}"
    );
    assert_eq!(facts(f).await["runs"], 0);
}

#[tokio::test]
async fn workflow_escalation_agent_origin_policy_stays_private_without_a_reply_context() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let company = f.people.role.company();
    // Deliberately omit only the reply-context snapshot in this isolated DB. The real
    // selected route remains live; immutable private origins must not depend on that snapshot.
    sqlx::query(
        "ALTER TABLE workflow_message_intents DISABLE TRIGGER channel_reply_ingress_snapshot",
    )
    .execute(p.pool())
    .await
    .unwrap();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    assert_eq!(facts(&f).await["messages"], 1);
    let contexts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_reply_contexts WHERE company_id=$1")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(
        contexts, 0,
        "the original channel-reply context guard is inapplicable here"
    );
    let public=sqlx::query("UPDATE workflow_message_intents SET delivery_policy='deliver' WHERE company_id=$1 AND id=(SELECT intent_id FROM workflow_escalation_agent_receipts WHERE company_id=$1)")
        .bind(company.as_uuid()).execute(p.pool()).await.unwrap_err();
    assert_eq!(
        public.as_database_error().unwrap().constraint(),
        Some("workflow_escalation_intent_identity_guard"),
        "{public}"
    );
    let private:bool=sqlx::query_scalar("SELECT delivery_policy='in_app_only' FROM workflow_message_intents WHERE company_id=$1 AND id=(SELECT intent_id FROM workflow_escalation_agent_receipts WHERE company_id=$1)")
        .bind(company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(private);
}

/// Mutate the canonical key immediately before native receipt validation, while the real
/// projector fence/route/source are still valid. Post-completion replay would mask this check.
#[tokio::test]
async fn workflow_escalation_agent_receipt_guard_binds_the_message_key_to_its_notice() {
    use crate::adapters::persistence::test_support::{
        minted_agent_notice_key, minted_agent_notice_recipient,
    };
    use crate::application::notification::{NotificationPersistence, NotificationProjectionCommit};
    for key in [
        "'<'||gen_random_uuid()::text||'@'||split_part(metadata.rfc_message_id,'@',2)",
        "'<escalation-'||NEW.notice_id::text||'@'||split_part(metadata.rfc_message_id,'@',2)",
        "'<'||NEW.notice_id::text||'>'",
    ] {
        let f = Box::pin(pending_agent()).await;
        let p = f.people.role.p();
        sqlx::raw_sql(&format!("CREATE SEQUENCE notice_key_probe_reached; \
            CREATE FUNCTION tamper_notice_key() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
              PERFORM nextval('notice_key_probe_reached'); \
              UPDATE email_message_metadata AS metadata SET rfc_message_id={key} \
                WHERE metadata.company_id=NEW.company_id AND metadata.message_id=NEW.message_id; \
              RETURN NEW; END $$; \
            CREATE TRIGGER aa_tamper_notice_key BEFORE INSERT ON workflow_escalation_agent_receipts \
              FOR EACH ROW EXECUTE FUNCTION tamper_notice_key();"))
            .execute(p.pool()).await.unwrap();
        let claim = p
            .claim_notification_events(
                crate::transport::WorkerId::random(),
                std::time::Duration::from_secs(30),
                1,
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        let expected = p
            .resolve_notification_projection(&claim.event)
            .await
            .unwrap();
        let key = minted_agent_notice_key(&expected);
        let recipient = minted_agent_notice_recipient(&expected);
        let error = p
            .commit_notification_projection(NotificationProjectionCommit {
                fence: &claim.lease,
                expected: &expected,
                email: None,
                agent_message_key: key.as_ref(),
                agent_recipient: recipient.as_ref(),
                escalation_ack_hours: Default::default(),
            })
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("agent notice origin must prove"),
            "{error}"
        );
        let reached: bool = sqlx::query_scalar("SELECT is_called FROM notice_key_probe_reached")
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert!(
            reached,
            "failure before the native receipt boundary proves nothing about key binding"
        );
        assert_eq!(facts(&f).await["messages"], 0);
        assert_eq!(facts(&f).await["intents"], 0);
        let receipts: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_agent_receipts")
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(receipts, 0);
    }
}

#[tokio::test]
async fn workflow_escalation_agent_authenticated_projector_ignores_unobserved_wire_identity_locks()
{
    use crate::adapters::persistence::test_support::{
        minted_agent_notice_key, minted_agent_notice_recipient,
    };
    use crate::application::notification::{
        NotificationPersistence, NotificationProjectionCommit, NotificationProjectionCommitOutcome,
    };
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let claim = p
        .claim_notification_events(
            crate::transport::WorkerId::random(),
            std::time::Duration::from_secs(30),
            1,
        )
        .await
        .unwrap()
        .pop()
        .unwrap();
    let expected = p
        .resolve_notification_projection(&claim.event)
        .await
        .unwrap();
    let key = minted_agent_notice_key(&expected);
    let recipient = minted_agent_notice_recipient(&expected);
    let mut blocked = p.pool().begin().await.unwrap();
    crate::adapters::persistence::participant::lock_identity_key(
        &mut blocked,
        f.people.role.company().as_uuid(),
        recipient.as_ref().unwrap(),
    )
    .await
    .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        p.commit_notification_projection(NotificationProjectionCommit {
            fence: &claim.lease,
            expected: &expected,
            email: None,
            agent_message_key: key.as_ref(),
            agent_recipient: recipient.as_ref(),
            escalation_ack_hours: Default::default(),
        }),
    )
    .await;
    blocked.rollback().await.unwrap();
    assert_eq!(
        result
            .expect("the authenticated projector observes no wire identity")
            .unwrap()
            .0,
        NotificationProjectionCommitOutcome::Applied
    );
    let unobserved: bool=sqlx::query_scalar(
        "SELECT count(*)=1 AND bool_and(author.kind='system' AND message.authored_identity_id IS NULL \
         AND NOT EXISTS(SELECT 1 FROM message_participants AS participant \
             WHERE participant.company_id=message.company_id AND participant.message_id=message.id)) \
         FROM workflow_escalation_agent_receipts AS receipt JOIN messages AS message \
           ON message.company_id=receipt.company_id AND message.id=receipt.message_id \
         JOIN principals AS author ON author.company_id=message.company_id AND author.id=message.author_principal_id \
         WHERE receipt.company_id=$1")
        .bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(unobserved);
    let intent = claims(&f).await.pop().unwrap();
    process(&f, intent).await;
    assert_eq!(
        facts(&f).await["runs"],
        1,
        "the actual targeted claim admits its one accountable root"
    );
}

#[path = "escalation_agent_native_claim_tests.rs"]
mod native_claims;
