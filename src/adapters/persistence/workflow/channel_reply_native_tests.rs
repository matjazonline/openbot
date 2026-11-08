use super::*;
use crate::application::workflow::channel_reply::{ChannelReplies, ConfigureChannelReply};

#[tokio::test]
async fn channel_reply_authoring_native_bootstrap_selects_once_and_replay_preserves_clear() {
    let f = Box::pin(command_fixture()).await;
    let created = service(&f)
        .create_assistant_channel(company(&f), owner(&f), command("native", "native"))
        .await
        .unwrap()
        .unwrap();
    let channel =
        RelatedChannelId::new(Uuid::parse_str(created["channel"]["id"].as_str().unwrap()).unwrap());
    let selected = f
        .persistence()
        .channel_reply_settings(company(&f), channel, owner(&f))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected.revision.get(), 1);
    assert_eq!(
        selected
            .responder
            .as_ref()
            .unwrap()
            .binding
            .as_uuid()
            .to_string(),
        created["binding_id"]
    );
    assert!(selected.approval_policy.is_none());
    f.persistence()
        .configure_channel_reply(ConfigureChannelReply {
            company: company(&f),
            channel,
            actor: owner(&f),
            expected: Some(selected.revision),
            responder: None,
            approval_policy: None,
        })
        .await
        .unwrap();
    assert_eq!(
        service(&f)
            .create_assistant_channel(company(&f), owner(&f), command("native", "native"))
            .await
            .unwrap()
            .unwrap(),
        created
    );
    let cleared = f
        .persistence()
        .channel_reply_settings(company(&f), channel, owner(&f))
        .await
        .unwrap()
        .unwrap();
    assert!(cleared.responder.is_none());
    assert_eq!(cleared.revision.get(), 2);
}

#[tokio::test]
async fn channel_reply_authoring_native_competing_commands_share_one_selection() {
    let f = Box::pin(command_fixture()).await;
    let barrier = Barrier::new(2);
    let (a, b) = tokio::join!(
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(
                    company(&f),
                    owner(&f),
                    command("race-native", "race-native"),
                )
                .await
        },
        async {
            barrier.wait().await;
            service(&f)
                .create_assistant_channel(
                    company(&f),
                    owner(&f),
                    command("race-native", "race-native"),
                )
                .await
        }
    );
    let created = a.unwrap().unwrap();
    assert_eq!(created, b.unwrap().unwrap());
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_reply_settings WHERE company_id=$1")
            .bind(company(&f).as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn channel_reply_authoring_native_invalid_custom_end_rolls_back_every_resource() {
    let (f, claim) = Box::pin(claimed(false)).await;
    let mut invalid = spec("no-partial-native");
    invalid["steps"] = json!([{"id":"early","slot":"before_reply","type":"data.map",
        "with":{"value":{"literal":{"body":"Should never send"}},"output_schema":{"literal":true}},
        "routes":{"success":"$end"}}]);
    let request = call(&f, &claim, ProvisioningKind::CreateAgentChannel, invalid).await;
    let before = written(&f).await;
    let receipt = committed(&dispatch(&f), &request).await;
    assert_eq!(receipt["error"]["code"], PROVISIONING_INVALID);
    let delta = grew(&before, &written(&f).await);
    for resource in ["agents", "channels", "workflows", "bindings"] {
        assert_eq!(delta[resource], 0, "{delta}");
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_reply_settings WHERE company_id=$1")
            .bind(company(&f).as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}
