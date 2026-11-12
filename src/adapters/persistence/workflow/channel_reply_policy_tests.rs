use super::*;
use crate::application::workflow::channel_reply::ReplyApprovalPolicy;

#[tokio::test]
async fn channel_reply_completion_policy_settings_freeze_duration_for_ingress() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let policy = ReplyApprovalPolicy::new(vec![s.owner_principal()], 60).unwrap();
    let mut request = selection(&s, binding, None).await;
    request.approval_policy = Some(policy.clone());
    let selected = s.p().configure_channel_reply(request).await.unwrap();
    assert_eq!(selected.approval_policy, Some(policy.clone()));
    let intent = s.intent(TO).await;
    let frozen: Value =
        sqlx::query_scalar("SELECT approval_policy FROM channel_reply_contexts WHERE intent_id=$1")
            .bind(intent)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(frozen, serde_json::to_value(&policy).unwrap());
    let mut changed = selection(&s, binding, Some(selected.revision)).await;
    changed.approval_policy =
        Some(ReplyApprovalPolicy::new(vec![s.owner_principal()], 120).unwrap());
    s.p().configure_channel_reply(changed).await.unwrap();
    let retained: Value =
        sqlx::query_scalar("SELECT approval_policy FROM channel_reply_contexts WHERE intent_id=$1")
            .bind(intent)
            .fetch_one(s.pool())
            .await
            .unwrap();
    assert_eq!(retained, frozen);
    assert!(
        sqlx::query("UPDATE channel_reply_contexts SET approval_policy=NULL WHERE intent_id=$1")
            .bind(intent)
            .execute(s.pool())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_reply_completion_policy_rejects_unprivileged_reviewers_and_invalid_sql_shape() {
    use crate::adapters::persistence::workflow::tests::message_tests::support::member;
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let ordinary = member(&s.fixture, "member").await;
    for reviewer in [s.fixture.principal(ordinary).await, Uuid::new_v4()] {
        let mut request = selection(&s, binding, None).await;
        request.approval_policy = Some(ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap());
        assert!(s.p().configure_channel_reply(request).await.is_err());
    }
    let selected = select(&s, binding).await;
    for policy in [
        json!({}),
        json!({"reviewers":[],"review_timeout_seconds":60}),
        json!({"reviewers":[{"kind":"principal","id":s.owner().user_id()}],"review_timeout_seconds":0}),
        json!({"reviewers":[{"kind":"principal","id":s.owner().user_id()}],"review_timeout_seconds":86401}),
        json!({"reviewers":[{"kind":"principal","id":Uuid::nil()}],"review_timeout_seconds":60}),
    ] {
        assert!(sqlx::query("UPDATE channel_reply_settings SET revision=revision+1,approval_policy=$1 WHERE company_id=$2 AND channel_id=$3")
            .bind(policy).bind(s.fixture.target.company.as_uuid()).bind(s.channel).execute(s.pool()).await.is_err());
    }
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
