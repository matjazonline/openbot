use super::*;
use crate::adapters::persistence::jsonb_test_support::LogCapture;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn jsonb_row_context_thread_contract_is_optional_per_message() {
    let fixture = Fixture::isolated("json_contract").await.unwrap();
    let mut ids = Vec::new();
    for suffix in ["broken", "valid"] {
        let message = inbound_email(
            fixture.thread.id,
            email_metadata(&format!("<{suffix}-{}@test.invalid>", fixture.suffix)),
            "{}",
        );
        fixture.persistence.create_message(&message).await.unwrap();
        ids.push(message.id);
        let format = if suffix == "broken" {
            "unsupported"
        } else {
            "json_schema"
        };
        let value = serde_json::json!({"body":"{}", "fingerprint":"a".repeat(64),
            "contract":{"version":1,"format":format,"schema":{"type":"object"}}});
        sqlx::query("UPDATE messages SET structured_response=$1 WHERE id=$2")
            .bind(value)
            .bind(message.id.as_uuid())
            .execute(&fixture.pool)
            .await
            .unwrap();
    }
    let logs = LogCapture::default();
    async {
        let broken = fixture
            .persistence
            .get_thread_message(fixture.thread.id, ids[0])
            .await
            .unwrap()
            .unwrap();
        assert!(broken.response_contract.is_none());
        assert_eq!(broken.clean_text_body, "{}");
        let valid = fixture
            .persistence
            .get_thread_message(fixture.thread.id, ids[1])
            .await
            .unwrap()
            .unwrap();
        assert!(valid.response_contract.is_some());
        let views = fixture
            .persistence
            .list_thread_message_views(fixture.thread.id)
            .await
            .unwrap();
        assert_eq!(views.len(), 2);
        assert_eq!(
            views
                .iter()
                .filter(|view| view.response_contract.is_some())
                .count(),
            1
        );
        let page = fixture
            .persistence
            .list_thread_message_views_after(fixture.thread.id, None, 100)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert!(
            fixture
                .persistence
                .get_thread_message_view(fixture.thread.id, ids[0])
                .await
                .unwrap()
                .unwrap()
                .response_contract
                .is_none()
        );
    }
    .with_subscriber(logs.dispatch())
    .await;
    assert!(logs.text().contains(&ids[0].as_uuid().to_string()));
    assert!(logs.text().contains("response_contract"));
    fixture.cleanup().await;
}
