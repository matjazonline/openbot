use super::*;
use crate::application::workflow::batch::{BatchBudget, WorkflowBatch};
use std::time::Duration;

#[tokio::test]
async fn channel_reply_terminal_human_continuation_preserves_notices() {
    let f = Box::pin(DecisionFixture::configured(1, |source| {
        source["steps"]["finish"]["type"] = json!("channel.no_reply");
    }))
    .await;
    let view = f.park().await;
    let before = f.counts().await;
    let applied = f
        .persistence()
        .submit_decision(f.submit(&view, 0, "terminal.human"))
        .await
        .unwrap();
    let done = f
        .persistence()
        .advance_pure(
            applied.successor.unwrap(),
            BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    let after = f.counts().await;
    assert_eq!(after["deliveries"], before["deliveries"]);
    assert_eq!(after["run"], "succeeded");
}
