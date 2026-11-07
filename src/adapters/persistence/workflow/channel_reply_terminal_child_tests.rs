use super::*;

#[tokio::test]
async fn channel_reply_terminal_repeat_child_cannot_suppress_parent() {
    let mut leaf = round_child();
    leaf["steps"]["review"] = json!({"type":"channel.no_reply","with":{
        "value":{"literal":{"choice":"accept","artifact":{"body":"done"},"feedback":""}},
        "output_schema":{"literal":leaf["output_schema"]}},"routes":{"success":"$end"}});
    let (f, parent) =
        AdmissionFixture::with_child(leaf, |child, version| repeat_parent(child, version, 1)).await;
    f.persistence().park_child(parent).await.unwrap().unwrap();
    let child = f.current_child(parent).await.unwrap();
    assert_eq!(
        f.persistence()
            .advance_pure(child, budget(4))
            .await
            .unwrap()
            .response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    let step = match f.persistence().resume_wait(parent).await.unwrap() {
        WaitProgress::Completed(step) => step,
        other => panic!("{other:?}"),
    };
    assert_eq!(step.response_disposition, ResponseDisposition::Automatic);
    assert_eq!(step.output["result"]["artifact"]["body"], "done");
    assert_eq!(
        f.persistence()
            .advance_pure(step.successor.unwrap(), budget(4))
            .await
            .unwrap()
            .response_disposition,
        Some(ResponseDisposition::Automatic)
    );
}

#[tokio::test]
async fn channel_reply_terminal_worker_dispatches_without_io() {
    let (f, scope) = fixture(example("channel.no_reply")).await;
    let worker = WorkflowWorker {
        port: f.persistence(),
        handler: &NoIo,
        notices: None,
        worker: WorkflowWorkerId(Uuid::new_v4()),
        lease: LeasePolicy::new(Duration::from_secs(3)).unwrap(),
        poll: PollPolicy::new(Duration::from_millis(50), 16).unwrap(),
        active_jobs: None,
        monitoring: None,
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    assert_eq!(
        worker
            .process(
                PollCandidate {
                    scope,
                    work: PollWork::Job
                },
                &cancel
            )
            .await
            .unwrap(),
        WorkDisposition::Done
    );
    assert_eq!(state(&f, scope.run).await, "succeeded");
    assert_eq!(
        snapshot(&f).await["executions"][0]["response_disposition"],
        "suppressed"
    );
}

#[tokio::test]
async fn channel_reply_terminal_child_cannot_suppress_parent() {
    let (f, scopes) = AdmissionFixture::chain(example("channel.no_reply"), 1).await;
    let (parent, child) = (scopes[0], scopes[1]);
    let done = f
        .persistence()
        .advance_pure(child, budget(4))
        .await
        .unwrap();
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
    let step = match f.persistence().resume_wait(parent).await.unwrap() {
        WaitProgress::Completed(step) => step,
        other => panic!("{other:?}"),
    };
    assert_eq!(step.response_disposition, ResponseDisposition::Automatic);
    assert_eq!(step.output, json!("hello"));
    let replay = match f.persistence().resume_wait(parent).await.unwrap() {
        WaitProgress::Completed(step) => step,
        other => panic!("{other:?}"),
    };
    assert_eq!(replay.response_disposition, ResponseDisposition::Automatic);
}

#[tokio::test]
async fn channel_reply_terminal_parent_selects_its_own_suppression() {
    let (f,parent)=AdmissionFixture::with_child(data_map(),|child,version| {
        let mut source=child_support::call_source(child,version);
        source["steps"]["start"]["routes"]["success"]=json!("finish");
        source["steps"]["finish"]=json!({"type":"channel.no_reply","with":{
            "value":{"ref":"/steps/start/output"},"output_schema":{"literal":true}},"routes":{"success":"$end"}});
        source
    }).await;
    f.persistence().park_child(parent).await.unwrap().unwrap();
    let child = f.current_child(parent).await.unwrap();
    assert_eq!(
        f.persistence()
            .advance_pure(child, budget(4))
            .await
            .unwrap()
            .response_disposition,
        Some(ResponseDisposition::Automatic)
    );
    let step = match f.persistence().resume_wait(parent).await.unwrap() {
        WaitProgress::Completed(step) => step,
        other => panic!("{other:?}"),
    };
    assert_eq!(step.response_disposition, ResponseDisposition::Automatic);
    let done = f
        .persistence()
        .advance_pure(step.successor.unwrap(), budget(4))
        .await
        .unwrap();
    assert_eq!(
        done.response_disposition,
        Some(ResponseDisposition::Suppressed)
    );
}

#[tokio::test]
async fn channel_reply_terminal_cancellation_competes_without_silent_failure() {
    for _ in 0..3 {
        let (f, scope) = fixture(example("channel.no_reply")).await;
        let barrier = Barrier::new(2);
        let complete = async {
            barrier.wait().await;
            f.persistence().advance_pure(scope, budget(4)).await
        };
        let cancel = async {
            barrier.wait().await;
            cancel_current(&f, scope, "terminal.cancel").await
        };
        let (completed, _) = tokio::join!(complete, cancel);
        let saved = snapshot(&f).await;
        match saved["runs"][0]["state"].as_str().unwrap() {
            "succeeded" => assert_eq!(
                completed.unwrap().response_disposition,
                Some(ResponseDisposition::Suppressed)
            ),
            "cancelled" => {
                assert!(completed.is_err());
                assert_eq!(saved["executions"][0]["response_disposition"], "automatic");
                assert_eq!(saved["executions"][0]["completed_at"], Value::Null);
            }
            other => panic!("{other}"),
        }
    }
}
