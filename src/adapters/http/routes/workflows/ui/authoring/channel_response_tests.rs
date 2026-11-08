use super::*;
use crate::adapters::http::routes::workflows::channel_replies::{ResponseBody, save};
use crate::adapters::http::routes::workflows::ui::authoring::channel_response::{
    card, save_response,
};
use crate::application::workflow::RelatedChannelId;

async fn responding_binding(f: &Fixture, channel: Uuid) -> (Uuid, String) {
    let workflow = Uuid::new_v4();
    let mut source = example("data.map", workflow);
    source["output_schema"] = json!({"type":"string","maxLength":16384});
    let version = f
        .published(workflow, "Response workflow", &source.to_string())
        .await;
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    let key = field_value(&pane, "form_key");
    let saved = f
        .save_binding(
            f.owner,
            workflow,
            &[
                ("form_key", &key),
                ("version_id", &version.to_string()),
                ("association", &format!("channel:{channel}")),
                ("params_mode", "json"),
                ("params_json", "{}"),
                ("message_trigger", "direct"),
            ],
        )
        .await;
    assert!(saved.contains("Saved binding"), "{saved}");
    let binding = f.usage(workflow).await.remove(0).binding;
    f.activity(
        f.owner,
        workflow,
        binding.binding.as_uuid(),
        binding.state_revision,
        true,
    )
    .await
    .unwrap();
    let binding = f.usage(workflow).await.remove(0).binding;
    (
        workflow,
        format!("{}:{}", binding.binding.as_uuid(), binding.state_revision),
    )
}

fn mapping() -> String {
    json!({"format_version":1,"subject":{"literal":"Response"},"body":{"ref":"/input"}}).to_string()
}

async fn submit(
    f: &Fixture,
    channel: Uuid,
    revision: &str,
    selected: &str,
    contract: &str,
) -> String {
    save_response(
        f.ui(f.owner),
        Path(channel),
        f.query(),
        form(&[
            ("expected_revision", revision),
            ("responder", selected),
            ("contract", contract),
            ("approval_policy", ""),
        ]),
    )
    .await
    .unwrap()
    .0
}

#[tokio::test]
async fn channel_reply_authoring_select_clear_stale_form_and_escaped_rejection() {
    let f = Fixture::new().await.expect("owned database fixture");
    let channel = f.channel("Desk", "desk", false).await;
    let (_, selected) = responding_binding(&f, channel).await;
    let before = card(&f.ui(f.owner), f.company, channel, None, None)
        .await
        .unwrap();
    assert!(before.contains("No responding workflow is selected"));
    let saved = submit(&f, channel, "", &selected, &mapping()).await;
    assert!(
        saved.contains("responding") && saved.contains("Response workflow"),
        "{saved}"
    );
    let stale = submit(&f, channel, "", &selected, &mapping()).await;
    assert!(
        stale.contains("Conflict:") && stale.contains(&selected),
        "{stale}"
    );
    let attack = "</textarea><img src=x onerror=alert(1)>";
    let rejected = submit(&f, channel, "1", &selected, attack).await;
    assert!(rejected.contains("&lt;/textarea&gt;&lt;img"));
    assert!(!rejected.contains(attack));
    assert!(rejected.contains("Reload current settings"));
    let cleared = submit(&f, channel, "1", "", "").await;
    assert!(cleared.contains("No responding workflow is selected"));
    let current = f
        .engine
        .channel_replies()
        .channel_reply_settings(
            CompanyId::new(f.company),
            RelatedChannelId::new(channel),
            f.ui(f.owner).actor().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(current.responder.is_none());
    assert_eq!(current.revision.get(), 2);
}

#[tokio::test]
async fn channel_reply_authoring_rejects_invalid_projection_before_activation_and_keeps_selection()
{
    let f = Fixture::new().await.expect("owned database fixture");
    let channel = f.channel("Desk", "desk", false).await;
    let (_, selected) = responding_binding(&f, channel).await;
    let invalid = json!({"format_version":1,"subject":{"literal":"Response"},"body":{"ref":"/input/missing"}}).to_string();
    let html = submit(&f, channel, "", &selected, &invalid).await;
    assert!(
        html.contains(&selected) && html.contains("/input/missing"),
        "{html}"
    );
    assert!(
        f.engine
            .channel_replies()
            .channel_reply_settings(
                CompanyId::new(f.company),
                RelatedChannelId::new(channel),
                f.ui(f.owner).actor().unwrap(),
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn channel_reply_authoring_private_foreign_and_member_requests_reveal_no_settings() {
    let f = Fixture::new().await.expect("owned database fixture");
    let other = Fixture::new()
        .await
        .expect("owned foreign database fixture");
    let private = f.channel("Secret channel", "secret", true).await;
    let foreign = other.channel("Foreign channel", "foreign", false).await;
    for (user, channel) in [(f.admin, private), (f.owner, foreign), (f.member, private)] {
        let result = card(&f.ui(user), f.company, channel, None, None).await;
        assert!(matches!(result, Err(AppError::NotFound(_))));
        let result = save(
            &f.engine,
            f.company,
            channel,
            f.ui(user).actor().unwrap(),
            ResponseBody {
                expected_revision: None,
                responder: None,
                approval_policy: None,
            },
        )
        .await;
        assert!(matches!(result, Err(AppError::NotFound(_))));
    }
}

#[tokio::test]
async fn channel_reply_authoring_json_routes_share_current_settings_and_conflict_scope() {
    use crate::adapters::http::routes::workflows::{AuthenticatedUser, channel_replies};
    use axum::{Json, extract::State, response::IntoResponse};
    let f = Fixture::new().await.expect("owned database fixture");
    let channel = f.channel("JSON desk", "json-desk", false).await;
    let (_, selected) = responding_binding(&f, channel).await;
    submit(&f, channel, "", &selected, &mapping()).await;
    let read = channel_replies::read(
        State(f.engine.clone()),
        AuthenticatedUser { id: f.owner },
        Path((f.company, channel)),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(read["revision"], 1);
    assert_eq!(read["responder"]["contract"]["body"]["ref"], "/input");
    let cleared = channel_replies::configure(
        State(f.engine.clone()),
        AuthenticatedUser { id: f.owner },
        Path((f.company, channel)),
        Json(ResponseBody {
            expected_revision: Some(1),
            responder: None,
            approval_policy: None,
        }),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(cleared["revision"], 2);
    assert!(cleared["responder"].is_null());
    let stale = channel_replies::configure(
        State(f.engine.clone()),
        AuthenticatedUser { id: f.owner },
        Path((f.company, channel)),
        Json(ResponseBody {
            expected_revision: Some(1),
            responder: None,
            approval_policy: None,
        }),
    )
    .await;
    assert_eq!(stale.into_response().status(), StatusCode::CONFLICT);
    let foreign = channel_replies::read(
        State(f.engine.clone()),
        AuthenticatedUser { id: f.stranger },
        Path((f.company, channel)),
    )
    .await;
    assert_eq!(foreign.into_response().status(), StatusCode::NOT_FOUND);
    assert!(
        serde_json::from_value::<ResponseBody>(json!({"expected_revision":2,"responder":null,
        "approval_policy":null,"run_as":f.owner}))
        .is_err()
    );
}
