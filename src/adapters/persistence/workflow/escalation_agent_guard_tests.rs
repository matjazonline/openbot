use super::*;
use crate::application::workflow::channel_reply::{ChannelReplies, ConfigureChannelReply};

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
        channel_id: RelatedChannelId::new(intent.channel),
        thread_id: crate::application::workflow::RelatedThreadId::new(intent.thread),
    };
    request.input = p.trigger_input(intent).await.unwrap().to_value();
    request.admission_claim = Some(AdmissionClaim::Message(intent.claim));
    request.correlation_id = intent.correlation;
    request.delivery_policy = intent.delivery_policy;
    let service = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    let result = service.admit(request).await.unwrap();
    assert!(
        matches!(result, AdmissionResult::Stale),
        "a requested unrelated binding must not bypass captured-origin revalidation: {result:?}"
    );
    assert_eq!(facts(f).await["runs"], 0);
}

#[tokio::test]
async fn workflow_escalation_agent_origin_policy_stays_private_without_a_reply_context() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let company = f.people.role.company();
    let actor = f.people.role.admission.binding.target.actor;
    let channel = RelatedChannelId::new(f.created.desk_id);
    let settings = p
        .channel_reply_settings(company, channel, actor)
        .await
        .unwrap()
        .unwrap();
    p.configure_channel_reply(ConfigureChannelReply {
        company,
        channel,
        actor,
        expected: Some(settings.revision),
        responder: None,
        approval_policy: None,
    })
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

/// The receipt guard binds the receipt's message to its notice through the key's local part.
/// Replaying the committed receipt passes the guard and is refused only as a duplicate; once the
/// message's key names anything but this notice, the guard itself refuses it.
#[tokio::test]
async fn workflow_escalation_agent_receipt_guard_binds_the_message_key_to_its_notice() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let replay = "INSERT INTO workflow_escalation_agent_receipts(company_id,notice_id,event_id, \
        principal_id,agent_id,desk_id,binding_id,binding_revision,message_id,intent_id) \
        SELECT company_id,notice_id,event_id,principal_id,agent_id,desk_id,binding_id, \
        binding_revision,message_id,intent_id FROM workflow_escalation_agent_receipts WHERE company_id=$1";
    let refused = async |key: Option<&str>| {
        let mut tx = p.pool().begin().await.unwrap();
        if let Some(key) = key {
            sqlx::query(&format!(
                "UPDATE email_message_metadata AS metadata SET rfc_message_id={key} \
                 FROM workflow_escalation_agent_receipts AS receipt \
                 WHERE receipt.company_id=$1 AND metadata.company_id=receipt.company_id \
                   AND metadata.message_id=receipt.message_id"
            ))
            .bind(company)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        let error = sqlx::query(replay)
            .bind(company)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        tx.rollback().await.unwrap();
        let error = error.into_database_error().unwrap();
        (
            error.code().unwrap().into_owned(),
            error.constraint().map(str::to_owned),
        )
    };
    // The minted key: everything holds, so only the primary key refuses the copy.
    assert_eq!(refused(None).await.0, "23505");
    let guard = Some("workflow_escalation_agent_receipt_guard".to_owned());
    for forged in [
        // Another notice's id under the same deployment domain.
        "'<'||gen_random_uuid()::text||'@'||split_part(metadata.rfc_message_id,'@',2)",
        // The retired hand-built local part under the same deployment domain.
        "'<escalation-'||receipt.notice_id::text||'@'||split_part(metadata.rfc_message_id,'@',2)",
        // The notice id with no domain at all.
        "'<'||receipt.notice_id::text||'>'",
    ] {
        assert_eq!(
            refused(Some(forged)).await,
            ("23514".to_owned(), guard.clone()),
            "{forged}"
        );
    }
}
