use super::*;
use crate::application::workflow::{
    polling::WorkflowStepKind,
    supervise::{SupervisedResult, supervise_io},
    worker::WorkflowHandler,
};
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, ScriptedExchange, ScriptedResponse, register_scripted_agent_base_url,
    scripted_llm, scripted_scenario,
};

async fn supervised(f: &HistoryThread, claim: ClaimedWorkflow) -> SupervisedResult {
    let handler = runtime(f);
    let kind = WorkflowStepKind("agent.run".into());
    let operation = handler.execute(&kind, &claim);
    supervise_io(
        f.s.p(),
        claim.clone(),
        policy(),
        tokio::time::Instant::now() + Duration::from_secs(15),
        operation,
        std::future::pending::<()>(),
    )
    .await
}

fn ready(result: SupervisedResult) -> Value {
    let SupervisedResult::Ready(result) = result else {
        panic!("independent provider must finish")
    };
    result.output
}

#[tokio::test]
async fn agent_history_ordering_supervised_park_frees_work_and_independent_stream_has_all_database_connections()
 {
    let mut f = Box::pin(HistoryThread::new()).await;
    let owner = f.next().await;
    let (arrived, arrival) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let mut response = ScriptedResponse::turn(LlmTurn::text("\"held\""), 0);
    response.barrier = Some((arrived, released));
    let held = scripted_scenario(vec![ScriptedExchange::new(
        |request| {
            if request.body["model"] != SCRIPTED_MODEL {
                return Err("scripted model");
            }
            Ok(())
        },
        response,
    )])
    .await;
    register_scripted_agent_base_url(f.agent, &held.base_url);
    let handler = runtime(&f);
    let kind = WorkflowStepKind("agent.run".into());
    let operation = handler.execute(&kind, &owner);
    let p = f.s.p().clone();
    // Own the actual supervised future; dropping it cancels the provider, with no detached task.
    let mut pending = Box::pin(supervise_io(
        &p,
        owner.clone(),
        policy(),
        tokio::time::Instant::now() + Duration::from_secs(15),
        operation,
        std::future::pending::<()>(),
    ));
    tokio::select! {
        signal=tokio::time::timeout(Duration::from_secs(3),arrival)=>signal.unwrap().unwrap(),
        _=&mut pending=>panic!("provider returned before its controlled release"),
    }
    let queued = f.next().await;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), supervised(&f, queued.clone()))
            .await
            .unwrap(),
        SupervisedResult::Suspended(_)
    ));
    all_connections_available(&p).await;
    super::capacity::new_thread(&mut f).await;
    let independent = f.next().await;
    let other = scripted_llm(vec![LlmTurn::text("\"independent\"")]).await;
    // The held request already has its endpoint; a second fixture server handles the next call.
    register_scripted_agent_base_url(f.agent, &other.base_url);
    assert_eq!(
        ready(
            tokio::time::timeout(Duration::from_secs(3), supervised(&f, independent))
                .await
                .unwrap()
        ),
        json!("independent")
    );
    let current: Option<Value> = sqlx::query_scalar(
        "SELECT output FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(owner.fence.scope.company.as_uuid())
    .bind(owner.fence.scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(current.is_none());
    release.send(()).unwrap();
    assert_eq!(ready(pending.await), json!("held"));
    assert!(matches!(
        p.resume_wait(queued.fence.scope).await.unwrap(),
        WaitProgress::Continued
    ));
    assert_eq!(held.finish().await.unwrap(), 1);
    assert_eq!(other.finish().await.unwrap(), 1);
}

async fn all_connections_available(p: &PostgresPersistence) {
    // Acquire every connection simultaneously while the real provider remains held.
    let mut connections = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), async {
        for _ in 0..p.pool().options().get_max_connections() {
            connections.push(p.pool().acquire().await.unwrap());
        }
    })
    .await
    .expect("provider and turn waiting must retain no database connection");
    drop(connections);
}
