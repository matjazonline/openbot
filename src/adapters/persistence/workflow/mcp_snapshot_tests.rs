//! Identical read operations keep their saved snapshot; a model-call ID is not a refresh key.
use super::*;
use crate::application::agent::ActionEffect;

#[tokio::test]
async fn workflow_mcp_new_model_ids_keep_read_snapshot_after_peer_changes() {
    let setup = Box::pin(mcp_setup(Call::Json)).await;
    assert_eq!(
        operation(setup.company()).tool.policy.effect,
        ActionEffect::Read
    );
    let claim = claimed(&setup).await;
    let runtime = setup.runtime();
    let saved = runtime
        .run(&kind(), &claim)
        .await
        .completed_result()
        .unwrap();
    assert_eq!(saved["structuredContent"]["name"], "Ada");
    setup.server.set_tool_result(
        TOOL.to_owned().try_into().unwrap(),
        json!({
            "content":[{"type":"text","text":"{\"name\":\"Later\"}"}],
            "structuredContent":{"name":"Later"},"isError":false
        }),
    );
    for id in ["read-model-1", "read-model-2"] {
        let RemoteDispatchObservation::Committed(replayed) = runtime
            .call_tool(claim.fence, tool_call(id), &CancellationToken::new())
            .await
            .unwrap()
        else {
            panic!("known read receipt required");
        };
        assert_eq!(replayed.result, saved);
    }
    assert_eq!(
        setup.server.calls(),
        1,
        "new IDs must not pretend the same saved read is fresh"
    );
    let mut another = tool_call("read-model-3");
    another.arguments = json!({"customer_id":"c-43"});
    let RemoteDispatchObservation::Committed(current) = runtime
        .call_tool(claim.fence, another, &CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("new semantic read required");
    };
    assert_eq!(current.result["structuredContent"]["name"], "Later");
    assert_eq!(
        setup.server.calls(),
        2,
        "the peer mutation is real for a new semantic request"
    );
}
