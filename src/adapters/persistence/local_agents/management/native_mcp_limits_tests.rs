use super::*;

fn invocation(f: &McpSetup, ordinal: usize) -> NativeMcpRequest {
    let mut request = f.request(json!({"query":format!("limit-{ordinal}")}));
    request.body.invocation.model_call_id = format!("toolu_limit_{ordinal}").try_into().unwrap();
    request
}

#[tokio::test]
async fn native_mcp_competing_machine_capacity_refuses_the_ninth_and_releases_only_finished_work() {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let mut tasks = tokio::task::JoinSet::new();
    for i in 0..=MAX_MACHINE_UNFINISHED {
        let request = invocation(&f, i);
        let p = p.clone();
        tasks.spawn(async move { (i, call_with_retry(p.as_ref(), &request).await) });
    }
    let mut admitted = Vec::new();
    let mut refused = None;
    while let Some(result) = tasks.join_next().await {
        let (i, result) = result.unwrap();
        match result {
            Ok(response) => admitted.push((i, response)),
            Err(error) => {
                assert!(
                    matches!(error, AppError::ServiceUnavailable(_)),
                    "{error:?}"
                );
                assert!(refused.replace(i).is_none());
            }
        }
    }
    assert_eq!(admitted.len(), MAX_MACHINE_UNFINISHED);
    let (ordinal, first) = &admitted[0];
    let mut cancel = invocation(&f, *ordinal);
    cancel.body.action = NativeMcpAction::CancelInvocation;
    assert_eq!(p.native_mcp(&cancel).await.unwrap().call_id, first.call_id);
    assert!(
        p.native_mcp(&invocation(&f, refused.unwrap()))
            .await
            .is_err(),
        "cancel intent alone does not free active work"
    );
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
    p.native_mcp(&invocation(&f, refused.unwrap()))
        .await
        .unwrap();
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE run.state IN ('queued','running','waiting')) FROM native_mcp_calls AS call JOIN workflow_runs AS run ON run.company_id=call.company_id AND run.id=call.run_id WHERE call.company_id=$1")
        .bind(f.s.f.company).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(
        counts,
        (
            (MAX_MACHINE_UNFINISHED + 1) as i64,
            MAX_MACHINE_UNFINISHED as i64
        )
    );
}

#[tokio::test]
async fn native_mcp_operation_capacity_includes_resolved_calls_but_never_blocks_existing_identity_replay()
 {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let mut original = None;
    for i in 0..MAX_OPERATION_CALLS {
        let response = p.native_mcp(&invocation(&f, i)).await.unwrap();
        if i == 0 {
            original = Some(response.call_id);
        }
        let mut cancel = invocation(&f, i);
        cancel.body.action = NativeMcpAction::CancelInvocation;
        p.native_mcp(&cancel).await.unwrap();
        assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
    }
    assert!(
        p.native_mcp(&invocation(&f, MAX_OPERATION_CALLS))
            .await
            .is_err()
    );
    let replay = p.native_mcp(&invocation(&f, 0)).await.unwrap();
    assert_eq!(Some(replay.call_id), original);
    assert!(matches!(replay.state, NativeMcpState::Cancelled));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM native_mcp_calls WHERE company_id=$1 AND operation_id=$2",
    )
    .bind(f.s.f.company)
    .bind(f.operation.as_uuid())
    .fetch_one(&f.s.f.database.pool)
    .await
    .unwrap();
    assert_eq!(count, MAX_OPERATION_CALLS as i64);
}
