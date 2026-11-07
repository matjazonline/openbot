use super::*;
#[path = "channel_reply_completion_tests.rs"]
mod completion_tests;
#[path = "channel_reply_policy_tests.rs"]
mod policy_tests;
use crate::application::workflow::channel_reply::*;

#[path = "channel_reply_binding_integrity_tests.rs"]
mod integrity;

pub(super) fn literal_contract(body: &str) -> ChannelReplyContract {
    ChannelReplyContract {
        format_version: 1,
        subject: ReplyText::Literal {
            literal: "Status".into(),
        },
        body: ReplyText::Literal {
            literal: body.into(),
        },
    }
}

pub(super) async fn selection(
    s: &Intents,
    binding: WorkflowBindingId,
    expected: Option<ReplySettingsRevision>,
) -> ConfigureChannelReply {
    ConfigureChannelReply {
        company: s.fixture.target.company,
        channel: RelatedChannelId::new(s.channel),
        actor: s.owner(),
        expected,
        approval_policy: None,
        responder: Some(ConfigureResponder {
            binding,
            expected_binding_revision: s.state(binding).await.revision,
            contract: literal_contract("Done"),
        }),
    }
}

pub(super) async fn select(s: &Intents, binding: WorkflowBindingId) -> ChannelReplySettings {
    s.p()
        .configure_channel_reply(selection(s, binding, None).await)
        .await
        .unwrap()
}

pub(super) async fn shape(s: &Intents, intent: Uuid) -> (i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM channel_reply_contexts WHERE company_id=$1 AND intent_id=$2),\
         (SELECT count(*) FROM channel_reply_runs WHERE company_id=$1 AND intent_id=$2)",
    )
    .bind(s.fixture.target.company.as_uuid())
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn channel_reply_binding_roundtrip_update_clear_and_stale_revision() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let first = select(&s, binding).await;
    let read = s
        .p()
        .channel_reply_settings(
            s.fixture.target.company,
            RelatedChannelId::new(s.channel),
            s.owner(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read, first);
    let mut next = selection(&s, binding, Some(first.revision)).await;
    next.responder.as_mut().unwrap().contract = literal_contract("Updated");
    let updated = s.p().configure_channel_reply(next).await.unwrap();
    assert_eq!(updated.revision.get(), 2);
    assert_eq!(
        updated.responder.unwrap().contract,
        literal_contract("Updated")
    );
    assert!(
        s.p()
            .configure_channel_reply(selection(&s, binding, Some(first.revision)).await)
            .await
            .is_err()
    );
    let cleared = s
        .p()
        .configure_channel_reply(ConfigureChannelReply {
            company: s.fixture.target.company,
            channel: RelatedChannelId::new(s.channel),
            actor: s.owner(),
            expected: Some(ReplySettingsRevision::try_from(2).unwrap()),
            responder: None,
            approval_policy: None,
        })
        .await
        .unwrap();
    assert!(cleared.responder.is_none());
    assert_eq!(cleared.revision.get(), 3);
    assert!(
        s.p()
            .configure_channel_reply(selection(&s, binding, None).await)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM channel_reply_settings WHERE company_id=$1")
            .bind(s.fixture.target.company.as_uuid())
            .execute(s.pool())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_reply_binding_competing_assignments_choose_one_current_responder() {
    let s = Intents::new().await;
    let first = s.binding(MessageTrigger::Any, None).await;
    let second = s.binding(MessageTrigger::Any, None).await;
    let a = selection(&s, first, None).await;
    let b = selection(&s, second, None).await;
    let (a, b) = tokio::join!(
        s.p().configure_channel_reply(a),
        s.p().configure_channel_reply(b)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let selected = s
        .p()
        .channel_reply_settings(
            s.fixture.target.company,
            RelatedChannelId::new(s.channel),
            s.owner(),
        )
        .await
        .unwrap()
        .unwrap();
    let winner = selected.responder.unwrap().binding;
    assert!(winner == first || winner == second);
    assert_eq!(selected.revision.get(), 1);
}

#[tokio::test]
async fn channel_reply_binding_freezes_head_contract_and_responder_at_ingress() {
    let s = Intents::new().await;
    let owner = s.binding(MessageTrigger::Any, None).await;
    let observer = s.binding(MessageTrigger::Any, None).await;
    let initial = select(&s, owner).await;
    let intent = s.intent_with(TO, "in_app_only").await;
    assert_eq!(shape(&s, intent).await, (1, 0));
    s.reconfigure(owner, None).await;
    let mut changed = selection(&s, observer, Some(initial.revision)).await;
    changed.responder.as_mut().unwrap().contract = literal_contract("Replacement");
    s.p().configure_channel_reply(changed).await.unwrap();
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, intent).await, (1, 1));
    assert_eq!(s.runs(owner).await.len(), 1);
    assert_eq!(
        s.runs(observer).await.len(),
        1,
        "observers keep independent admission"
    );
    let frozen:(Uuid,i64,Value,String,Value)=sqlx::query_as(
        "SELECT run.binding_id,run.binding_revision,run.params,run.delivery_policy,context.contract \
         FROM channel_reply_runs AS link JOIN workflow_runs AS run ON run.company_id=link.company_id AND run.id=link.run_id \
         JOIN channel_reply_contexts AS context ON context.company_id=link.company_id AND context.intent_id=link.intent_id \
         WHERE link.company_id=$1 AND link.intent_id=$2")
        .bind(s.fixture.target.company.as_uuid()).bind(intent).fetch_one(s.pool()).await.unwrap();
    assert_eq!(
        frozen,
        (
            owner.as_uuid(),
            1,
            json!({}),
            "in_app_only".into(),
            serde_json::to_value(literal_contract("Done")).unwrap()
        )
    );
    s.make_due(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, intent).await, (1, 1));
    assert_eq!(s.runs(owner).await.len(), 1);
}

#[tokio::test]
async fn channel_reply_binding_quiet_and_unselected_ingress_get_no_snapshot() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let ordinary = s.intent(TO).await;
    assert_eq!(shape(&s, ordinary).await, (0, 0));
    select(&s, binding).await;
    let quiet = s
        .intent(Facts {
            admissible: false,
            ..TO
        })
        .await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, quiet).await, (0, 0));
    assert_eq!(
        shape(&s, ordinary).await,
        (0, 0),
        "later selection cannot acquire an old message"
    );
    assert_eq!(s.runs(binding).await.len(), 1);
}

#[tokio::test]
async fn channel_reply_binding_trigger_mismatch_and_revocation_create_no_obligation() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Mentioned, None).await;
    select(&s, binding).await;
    let mismatch = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, mismatch).await, (1, 0));
    let eligible = s
        .intent(Facts {
            mentioned: true,
            ..TO
        })
        .await;
    s.deactivate(binding).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, eligible).await, (1, 0));
    assert!(s.runs(binding).await.is_empty());
    assert_eq!(
        s.outcomes(eligible).await.len(),
        1,
        "revoked selected responder has a visible refusal"
    );
}

#[tokio::test]
async fn channel_reply_binding_snapshot_and_referenced_source_facts_are_immutable() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent_with(TO, "in_app_only").await;
    for mutation in [
        "delivery_policy='deliver'",
        "origin='internal'",
        "admissible=false",
        "role='cc'",
        "mentioned=true",
        "hop_count=1",
        "correlation_id=gen_random_uuid()",
        "created_at=created_at-interval '1 hour'",
        "responder_principal_id=NULL",
    ] {
        let result = sqlx::query(&format!(
            "UPDATE workflow_message_intents SET {mutation} WHERE id=$1"
        ))
        .bind(intent)
        .execute(s.pool())
        .await;
        assert!(result.is_err(), "{mutation}");
    }
    for mutation in [
        "delivery_policy='deliver'",
        "contract='{}'::jsonb",
        "actor_id=gen_random_uuid()",
        "source=source || '{\"origin\":\"internal\"}'::jsonb",
    ] {
        assert!(
            sqlx::query(&format!(
                "UPDATE channel_reply_contexts SET {mutation} WHERE intent_id=$1"
            ))
            .bind(intent)
            .execute(s.pool())
            .await
            .is_err(),
            "{mutation}"
        );
    }
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(
        shape(&s, intent).await,
        (1, 1),
        "normal mutable queue/admission state remains usable"
    );
}

#[tokio::test]
async fn channel_reply_binding_rejects_foreign_inactive_thread_and_stale_selection() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let mut stale = selection(&s, binding, None).await;
    s.reconfigure(binding, Some(MessageTrigger::Any)).await;
    assert!(s.p().configure_channel_reply(stale).await.is_err());
    stale = selection(&s, binding, None).await;
    stale.channel = RelatedChannelId::new(new_channel(&s.fixture, "other").await);
    assert!(s.p().configure_channel_reply(stale).await.is_err());
    let outsider = member(&s.fixture, "member").await;
    let mut denied = selection(&s, binding, None).await;
    denied.actor = WorkflowActor::authenticated(outsider).unwrap();
    assert!(s.p().configure_channel_reply(denied).await.is_err());
    assert!(
        s.p()
            .channel_reply_settings(
                s.fixture.target.company,
                RelatedChannelId::new(s.channel),
                WorkflowActor::authenticated(outsider).unwrap()
            )
            .await
            .is_err()
    );
    let mut foreign = selection(&s, binding, None).await;
    foreign.company = CompanyId::new(Uuid::new_v4());
    assert!(s.p().configure_channel_reply(foreign).await.is_err());
    let mut invalid = selection(&s, binding, None).await;
    invalid.responder.as_mut().unwrap().contract.body = ReplyText::Reference {
        r#ref: "/input/missing".to_owned().try_into().unwrap(),
    };
    assert!(s.p().configure_channel_reply(invalid).await.is_err());
    s.deactivate(binding).await;
    assert!(
        s.p()
            .configure_channel_reply(selection(&s, binding, None).await)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_reply_binding_passive_snapshot_follows_source_retention() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent(TO).await;
    assert!(
        sqlx::query("DELETE FROM channel_reply_contexts WHERE company_id=$1 AND intent_id=$2")
            .bind(s.fixture.target.company.as_uuid())
            .bind(intent)
            .execute(s.pool())
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM workflow_message_intents WHERE company_id=$1 AND id=$2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(intent)
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(shape(&s, intent).await, (0, 0));
}

#[tokio::test]
async fn channel_reply_binding_live_run_blocks_snapshot_and_principal_deletion() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, intent).await, (1, 1));
    let mut tx = s.pool().begin().await.unwrap();
    assert!(
        sqlx::query("DELETE FROM channel_reply_runs WHERE company_id=$1 AND intent_id=$2")
            .bind(s.fixture.target.company.as_uuid())
            .bind(intent)
            .execute(s.pool())
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM workflow_message_intents WHERE company_id=$1 AND id=$2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(intent)
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(tx.commit().await.is_err());
    let principal: Uuid = sqlx::query_scalar(
        "SELECT responder_principal_id FROM channel_reply_contexts WHERE intent_id=$1",
    )
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert!(
        sqlx::query("DELETE FROM principals WHERE company_id=$1 AND id=$2")
            .bind(s.fixture.target.company.as_uuid())
            .bind(principal)
            .execute(s.pool())
            .await
            .is_err()
    );
    assert_eq!(shape(&s, intent).await, (1, 1));
}

#[tokio::test]
async fn channel_reply_binding_company_teardown_removes_scoped_selection_and_snapshots() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    select(&s, binding).await;
    let intent = s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(shape(&s, intent).await, (1, 1));
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(s.fixture.target.company.as_uuid())
        .execute(s.pool())
        .await
        .unwrap();
    assert_eq!(shape(&s, intent).await, (0, 0));
}
