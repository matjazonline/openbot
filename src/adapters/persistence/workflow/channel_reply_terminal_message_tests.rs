use super::*;
use crate::application::workflow::{batch::*, completion::*};
use std::time::Duration;

#[tokio::test]
async fn channel_reply_terminal_preserves_an_explicit_published_message() {
    use crate::application::workflow::polling::WorkflowStepKind;
    let fixture = Fixture::new().await;
    let channel = new_channel(&fixture, "explicit").await;
    let mut source: Value =
        serde_json::from_str(&registry::example("message.send").unwrap().source).unwrap();
    source["steps"]["start"]["with"] = json!({
        "content":{"literal":{"subject":"Status","body":"Already sent"}},
        "destinations":{"literal":[{"kind":"channel","id":channel}]}});
    source["steps"]["start"]["routes"]["success"] = json!("finish");
    source["steps"]["finish"] = json!({"type":"channel.no_reply","with":{
        "value":{"literal":"done"},"output_schema":{"literal":{"type":"string"}}},"routes":{"success":"$end"}});
    let version = Box::pin(published_terminal(&fixture, source)).await;
    let target = Box::pin(bound(&fixture, version)).await;
    let scope = admitted(&fixture, target, channel, "explicit-terminal").await;
    let p = &fixture.persistence;
    let claim = p
        .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
        .await
        .unwrap()
        .unwrap();
    let output = match runtime(p)
        .execute(&WorkflowStepKind("message.send".into()), &claim)
        .await
    {
        WorkflowHandlerOutcome::Completed(result) => result.unwrap(),
        other => panic!("{other:?}"),
    };
    let step = p
        .complete_io(FencedWorkflowResult {
            fence: claim.fence,
            output,
            policy: lease(),
        })
        .await
        .unwrap()
        .unwrap();
    let messages: Value =
        sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(message) ORDER BY id) FROM messages message")
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(messages.as_array().unwrap().len(), 1);
    let done = p
        .advance_pure(
            step.successor.unwrap(),
            BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    let after: Value =
        sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(message) ORDER BY id) FROM messages message")
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(after, messages);
}

async fn published_terminal(fixture: &Fixture, source: Value) -> VersionId {
    let directory = step_builtins::StepBuiltinPublicationDirectory {
        persistence: fixture.persistence.clone(),
    };
    let definitions = DefinitionService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
        directory,
    );
    let mut draft = fixture.save(None);
    draft.content.source = source.to_string();
    definitions.save(draft).await.unwrap();
    let publication = fixture.publish();
    definitions.publish(publication.clone()).await.unwrap();
    publication.version
}
