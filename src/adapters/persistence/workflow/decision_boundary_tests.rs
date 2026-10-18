use super::*;

async fn rejected_unchanged(f: &DecisionFixture, view: &DecisionView, command: SubmitDecision) {
    let before = f.counts().await;
    assert!(matches!(
        f.persistence().submit_decision(command).await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(f.counts().await, before);
    let read = f
        .persistence()
        .read_decision(f.scope.company, f.actor(0), view.decision)
        .await
        .unwrap();
    assert_eq!(read.revision, 1);
    assert!(read.submission.is_none());
}

#[tokio::test]
async fn workflow_decision_feedback_character_and_byte_boundaries() {
    for character in ["x", "é", "🦀"] {
        let f = Box::pin(DecisionFixture::configured(1, |source| {
            source["limits"]["max_context_bytes"] = json!(131072);
        }))
        .await;
        let view = f.park().await;
        let mut command = f.submit(&view, 0, "feedback-limit");
        command.feedback = character.repeat(registry::MAX_TEXT + 1);
        rejected_unchanged(&f, &view, command.clone()).await;
        command.feedback = character.repeat(registry::MAX_TEXT);
        let result = f.persistence().submit_decision(command).await.unwrap();
        assert_eq!(result.disposition, DecisionDisposition::Applied);
        let read = f
            .persistence()
            .read_decision(f.scope.company, f.actor(0), view.decision)
            .await
            .unwrap();
        assert_eq!(
            read.submission.unwrap().feedback,
            character.repeat(registry::MAX_TEXT)
        );
    }
}

#[tokio::test]
async fn workflow_decision_data_configured_context_boundary() {
    let f = Box::pin(DecisionFixture::configured(1, |source| {
        source["limits"]["max_context_bytes"] = json!(4096);
        source["steps"]["start"]["with"]["data_schema"] = json!({"literal":{"type":"string"}});
    }))
    .await;
    let view = f.park().await;
    // The limit applies to the entire committed output, including its JSON envelope.
    let overhead = serde_json::to_vec(&json!({"choice":"accept","data":"","feedback":""}))
        .unwrap()
        .len();
    let mut command = f.submit(&view, 0, "data-limit");
    command.data = json!("x".repeat(4096 - overhead + 1));
    rejected_unchanged(&f, &view, command.clone()).await;
    command.data = json!("x".repeat(4096 - overhead));
    assert_eq!(
        f.persistence()
            .submit_decision(command)
            .await
            .unwrap()
            .disposition,
        DecisionDisposition::Applied
    );
}

#[tokio::test]
async fn workflow_decision_whole_output_cannot_activate_message_delivery() {
    for content in [
        json!({"ref":"/steps/start/output"}),
        json!({"object":{"body":{"ref":"/steps/start/output/data"}}}),
    ] {
        let whole = content.get("ref").is_some();
        let f = Box::pin(DecisionFixture::configured(1, |source| {
            let message: Value =
                serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
            source["steps"]["finish"] = message["steps"]["start"].clone();
            source["steps"]["finish"]["with"]["content"] = content;
        }))
        .await;
        let view = f.park().await;
        let successor = f
            .persistence()
            .submit_decision(f.submit(&view, 0, "disclosure"))
            .await
            .unwrap()
            .successor
            .unwrap();
        let activation = f.persistence().activate(successor).await;
        if whole {
            assert!(matches!(activation, Err(AppError::BadRequest(_))));
        } else {
            let activated = activation.unwrap();
            assert_eq!(activated.inputs["content"], json!({"body":"accepted"}));
        }
        // Only internal reviewer notices exist: no publish/transport dispatch occurred.
        let deliveries: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM message_deliveries WHERE purpose<>'notification'",
        )
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
        assert_eq!(deliveries, 0);
    }
}
