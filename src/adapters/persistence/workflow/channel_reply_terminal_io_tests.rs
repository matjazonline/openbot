use super::*;

#[tokio::test]
async fn channel_reply_terminal_io_predecessor_keeps_receipt_and_local_disposition() {
    let mut source = chain();
    source["steps"]["finish"]["type"] = json!("channel.no_reply");
    let (f, claim) = claimed(source).await;
    let p = f.persistence();
    let done = p
        .settled()
        .complete_io(result(claim.fence))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.response_disposition, ResponseDisposition::Automatic);
    let restarted = PostgresPersistence::new(p.pool().clone());
    let replay = restarted
        .settled()
        .complete_io(result(claim.fence))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay.response_disposition, done.response_disposition);
    assert_eq!(replay.output, done.output);
    let terminal = p
        .advance_pure(
            done.successor.unwrap(),
            BatchBudget::new(4, Duration::from_secs(1)).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        terminal.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    assert_eq!(state(&f).await["attempts"][0]["status"], "completed");
}
