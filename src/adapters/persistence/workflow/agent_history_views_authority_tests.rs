use super::*;

async fn assert_restricted_view(
    f: &ManualHistory,
    claim: &ClaimedWorkflow,
    manager: WorkflowActor,
) {
    let restricted = detail(f, claim, manager).await;
    assert_eq!(restricted.entry.payload, HistoryPayloadState::Restricted);
    assert!(
        restricted.payload.is_none()
            && restricted.status.is_none()
            && restricted.entry.source.is_none()
            && restricted.entry.exchange.is_none()
    );
    let encoded = serde_json::to_string(&restricted).unwrap();
    for forbidden in [
        "CLASSIFIED-INPUT",
        "CLASSIFIED-OUTPUT",
        "selected",
        "omitted",
        "revision",
        "source",
        "checkpoint",
    ] {
        assert!(!encoded.contains(forbidden), "{forbidden}");
    }
    let html = agent_history_detail(anchor(f, claim), &restricted);
    assert!(html.contains("Restricted exchange (completed)"));
    assert!(html.contains("Created") && html.contains("finished"));
    assert!(!html.contains("CLASSIFIED"));
    assert!(detail(f, claim, f.f.s.owner()).await.payload.is_some());
}
async fn assert_members_denied(f: &ManualHistory, claim: &ClaimedWorkflow) {
    let member = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &f.f.s.fixture,
        "member",
    )
    .await;
    let p = f.f.s.p();
    sqlx::query("UPDATE users SET email_verified_at=clock_timestamp() WHERE id=$1")
        .bind(member)
        .execute(f.f.s.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','system' FROM principals WHERE company_id=$1 AND user_id=$3 ON CONFLICT DO NOTHING")
        .bind(f.f.s.fixture.target.company.as_uuid()).bind(f.f.s.channel).bind(member).execute(f.f.s.pool()).await.unwrap();
    let context =
        crate::application::ports::participant::PrincipalAccessPersistence::access_context_for_user(
            p,
            f.f.s.fixture.target.company.as_uuid(),
            member,
        )
        .await
        .unwrap()
        .unwrap();
    let channel = crate::application::workflow::WorkflowChannelReader::workflow_channel(
        p,
        RelatedChannelId::new(f.f.s.channel),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        channel.viewer_access(context),
        "ordinary member can view the actual business channel but cannot Inspect native history"
    );
    let outsider = WorkflowActor::authenticated(Uuid::new_v4()).unwrap();
    for viewer in [WorkflowActor::authenticated(member).unwrap(), outsider] {
        let read_scope = scope(f, claim, viewer);
        assert!(
            f.f.s
                .p()
                .agent_history_page(
                    read_scope,
                    claim.fence.scope.execution.as_uuid(),
                    HistoryPageRequest::new(None, None).unwrap()
                )
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.f.s
                .p()
                .agent_history_exchange(
                    read_scope,
                    claim.fence.scope.execution.as_uuid(),
                    id(claim)
                )
                .await
                .unwrap()
                .is_none()
        );
    }
}
async fn assert_foreign_denied(f: &ManualHistory, claim: &ClaimedWorkflow) {
    let native = f.f.s.p();
    let read_scope = scope(f, claim, f.f.s.owner());
    let foreign = Box::pin(HistoryThread::in_fixture(
        Fixture::in_database(f.f.s.fixture._db.clone(), "-history-foreign").await,
    ))
    .await;
    let foreign_claim = foreign.next().await;
    foreign
        .s
        .p()
        .prepare_agent(&foreign_claim, policy())
        .await
        .unwrap()
        .expect_ready();
    assert!(
        native
            .agent_history_exchange(
                read_scope,
                claim.fence.scope.execution.as_uuid(),
                AgentExchangeId::for_execution(foreign_claim.fence.scope.execution)
            )
            .await
            .unwrap()
            .is_none()
    );
    let foreign_scope = RunReadScope {
        company: read_scope.company,
        run: foreign_claim.fence.scope.run,
        viewer: read_scope.viewer,
    };
    assert!(
        native
            .agent_history_page(
                foreign_scope,
                foreign_claim.fence.scope.execution.as_uuid(),
                HistoryPageRequest::new(None, None).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
}
async fn assert_isolated_stream(f: &mut ManualHistory, claim: &ClaimedWorkflow) {
    f.from_run = None;
    let other = f
        .complete(json!("other input"), json!("other stream"))
        .await;
    let native = f.f.s.p();
    let read_scope = scope(f, claim, f.f.s.owner());
    assert!(
        native
            .agent_history_exchange(
                read_scope,
                claim.fence.scope.execution.as_uuid(),
                id(&other)
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .agent_history_page(
                read_scope,
                other.fence.scope.execution.as_uuid(),
                HistoryPageRequest::new(None, None).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT conversation_id FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
    )
    .bind(read_scope.company.as_uuid())
    .bind(id(&other).as_uuid())
    .fetch_one(f.f.s.pool())
    .await
    .unwrap();
    let cursor = HistoryCursor::after(AgentConversationId::new(conversation), 1).unwrap();
    assert!(
        native
            .agent_history_page(
                read_scope,
                claim.fence.scope.execution.as_uuid(),
                HistoryPageRequest::new(Some(cursor.as_str().into()), None).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .agent_history_exchange(
                read_scope,
                claim.fence.scope.execution.as_uuid(),
                AgentExchangeId::new(Uuid::new_v4())
            )
            .await
            .unwrap()
            .is_none(),
        "sample/guessed ids have no native authority"
    );
    assert_foreign_denied(f, claim).await;
}

#[tokio::test]
async fn agent_history_views_private_admin_positive_restricted_member_customer_foreign_scope_cursor_and_sample_denials()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let manager = admin(&f, 5).await;
    let llm = scripted_llm(vec![
        LlmTurn::text("\"CLASSIFIED-OUTPUT\""),
        LlmTurn::text("\"other stream\""),
    ])
    .await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f.start(json!("CLASSIFIED-INPUT")).await;
    level::raise(&f.f, &claim, AccessLevel::L5).await;
    assert_eq!(
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &claim))
            .await
            .completed_result()
            .unwrap(),
        json!("CLASSIFIED-OUTPUT")
    );
    cancel(&f.f, &claim, "view-accepted").await;
    let visible = detail(&f, &claim, manager).await;
    assert_eq!(
        visible.payload.as_ref().unwrap().input,
        json!("CLASSIFIED-INPUT"),
        "another currently authorized admin may inspect the private run"
    );
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(f.f.s.fixture.target.company.as_uuid())
        .bind(manager.user_id())
        .execute(f.f.s.pool())
        .await
        .unwrap();
    assert_restricted_view(&f, &claim, manager).await;
    assert_members_denied(&f, &claim).await;
    Box::pin(assert_isolated_stream(&mut f, &claim)).await;
    assert_eq!(llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn agent_history_level_history_reader_rechecks_demotion_and_inflight_raise_after_payload_fetch()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let manager = admin(&f, 5).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"CLASSIFIED-OUTPUT\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let completed = f.start(json!("CLASSIFIED-INPUT")).await;
    level::raise(&f.f, &completed, AccessLevel::L5).await;
    assert_eq!(
        Box::pin(runtime(&f.f).execute(&WorkflowStepKind("agent.run".into()), &completed))
            .await
            .completed_result()
            .unwrap(),
        json!("CLASSIFIED-OUTPUT")
    );
    cancel(&f.f, &completed, "completed-before-view-race").await;
    let p = f.f.s.p().clone();
    let read_scope = scope(&f, &completed, manager);
    let pause = testing::pause(read_scope.company);
    let execution = completed.fence.scope.execution.as_uuid();
    let exchange = id(&completed);
    let read = tokio::spawn(async move {
        p.agent_history_exchange(read_scope, execution, exchange)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), pause.fetched.notified())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(read_scope.company.as_uuid())
        .bind(manager.user_id())
        .execute(f.f.s.pool())
        .await
        .unwrap();
    pause.resume.notify_one();
    let result = read.await.unwrap().unwrap().unwrap();
    assert_eq!(result.entry.payload, HistoryPayloadState::Restricted);
    assert!(result.payload.is_none());
    // A queued/prepared call has no accepted output stamp; current source classification covers its input.
    f.from_run = None;
    let active = f.start(json!("new active input")).await;
    f.f.s
        .p()
        .prepare_agent(&active, policy())
        .await
        .unwrap()
        .expect_ready();
    assert!(
        detail(&f, &active, manager).await.payload.is_some(),
        "the actual input is visible before its concurrent classification rise"
    );
    let p = f.f.s.p().clone();
    let read_scope = scope(&f, &active, manager);
    let pause = testing::pause(read_scope.company);
    let execution = active.fence.scope.execution.as_uuid();
    let exchange = id(&active);
    let read = tokio::spawn(async move {
        p.agent_history_exchange(read_scope, execution, exchange)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), pause.fetched.notified())
        .await
        .unwrap();
    level::raise(&f.f, &active, AccessLevel::L5).await;
    pause.resume.notify_one();
    let result = read.await.unwrap().unwrap().unwrap();
    assert_eq!(result.entry.payload, HistoryPayloadState::Restricted);
    assert!(result.payload.is_none());
    assert_eq!(llm.finish().await.unwrap(), 1);
}

#[tokio::test]
async fn agent_history_views_payload_expiry_after_fetch_releases_only_expired_metadata_without_stale_payload()
 {
    let mut f = Box::pin(ManualHistory::new()).await;
    let llm = scripted_llm(vec![LlmTurn::text("\"old output\"")]).await;
    register_scripted_agent_base_url(f.f.agent, &llm.base_url);
    let claim = f
        .complete(json!("OLD-RETAINED-INPUT"), json!("old output"))
        .await;
    let p = f.f.s.p().clone();
    let read_scope = scope(&f, &claim, f.f.s.owner());
    let pause = testing::pause(read_scope.company);
    let execution = claim.fence.scope.execution.as_uuid();
    let exchange = id(&claim);
    let pending = tokio::spawn(async move {
        p.agent_history_exchange(read_scope, execution, exchange)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), pause.fetched.notified())
        .await
        .unwrap();
    clock::expire(f.f.s.p(), claim.fence.scope).await;
    pause.resume.notify_one();
    let result = pending.await.unwrap().unwrap().unwrap();
    assert_eq!(result.entry.payload, HistoryPayloadState::Expired);
    assert_eq!(result.entry.activity, Some(HistoryActivity::Produced));
    assert!(result.payload.is_none());
    assert!(
        result.entry.provenance.is_some(),
        "expiry preserves inspectable native agent/revision provenance"
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("OLD-RETAINED-INPUT")
    );
    assert_eq!(llm.finish().await.unwrap(), 1);
}
