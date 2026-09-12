use super::*;
use crate::entities::task::{BackgroundTask, TaskBoardFilter, ThreadActivity};
use crate::services::task_worker::TaskWorker;
use crate::task_queue::CollaborationReadScope;
use std::collections::HashSet;
use std::time::Duration;

pub(super) async fn extra_channel(fx: &Fixture, name: &str, llm_url: &str) -> Channel {
    let agent = AgentPersistence::create(
        fx.persistence.as_ref(),
        fx.company.id,
        AgentWrite {
            name: format!("{name} Desk"),
            slug: format!("{name}-agent").to_lowercase(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Answer the customer briefly.".into()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    register_scripted_agent_base_url(agent.id, llm_url);
    ChannelPersistence::create(
        fx.persistence.as_ref(),
        fx.company.id,
        ChannelWrite {
            name: name.into(),
            slug: name.to_lowercase(),
            agent_ids: Some(vec![agent.id]),
            participant_emails: Some(vec![fx.customer_email.clone()]),
            add_3rd_party: name == "Support",
            enabled: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

pub(super) fn addressed(fx: &Fixture, to: &str, cc: Option<&str>, body: &str) -> RawInboundPayload {
    RawInboundPayload {
        to: to.into(),
        cc: cc.map(str::to_owned),
        ..inbound(fx, CUSTOMER_FIRST_MESSAGE_ID, "Upgrade and invoice", body)
    }
}

pub(super) fn address(fx: &Fixture, local: &str) -> String {
    format!("{local}@{}.{}", fx.company.slug, APP_DOMAIN)
}

pub(super) async fn task(fx: &Fixture, id: Uuid) -> BackgroundTask {
    fx.persistence.get_task_by_id(id).await.unwrap().unwrap()
}

pub(super) async fn make_due_at(fx: &Fixture, id: Uuid, when: chrono::DateTime<Utc>) {
    sqlx::query("UPDATE background_tasks SET run_at = $2 WHERE id = $1")
        .bind(id)
        .bind(when)
        .execute(&fx.pool)
        .await
        .unwrap();
}

/// Keep the production worker owned by this future; even a timeout shuts it down and awaits it.
pub(super) async fn work_until(
    fx: &Fixture,
    ids: &[Uuid],
    settled: impl Fn(&[BackgroundTask]) -> bool,
) {
    let worker = Arc::new(
        TaskWorker::new(
            fx.persistence.clone(),
            fx.threads.clone(),
            external_test_config(),
        )
        .with_agent_run_timeout(Duration::from_secs(10)),
    );
    let (shutdown, receiver) = tokio::sync::broadcast::channel(1);
    let running = worker.start_worker_loop(receiver);
    tokio::pin!(running);
    let checking = async {
        loop {
            let mut tasks = Vec::with_capacity(ids.len());
            for id in ids {
                tasks.push(fx.persistence.get_task_by_id(*id).await?.unwrap());
            }
            if settled(&tasks) {
                return Ok::<(), AppError>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let outcome = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(20), checking) => result,
        () = &mut running => panic!("the task worker exited before shutdown"),
    };
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("the worker shuts down promptly");
    if let Err(error) = &outcome {
        let mut states = Vec::new();
        for id in ids {
            states.push(task(fx, *id).await);
        }
        panic!("worker did not settle: {error}; task state: {states:?}");
    }
    outcome.unwrap().unwrap();
}

pub(super) async fn deliver_on(fx: &Fixture, channel: &Channel) {
    let queued = fx.queued_delivery_on(channel.id).await;
    fx.deliver(&queued).await;
}

pub(super) async fn assert_board_and_activity(fx: &Fixture, ids: &[Uuid], channels: &[Uuid]) {
    let first = task(fx, ids[0]).await;
    let second = task(fx, ids[1]).await;
    assert_eq!(first.correlation_id, second.correlation_id);
    assert_ne!(first.ownership.owner, second.ownership.owner);
    let board = fx
        .persistence
        .list_task_chain_board(
            fx.company.id,
            TaskBoardFilter::new(None, Utc::now()),
            channels,
        )
        .await
        .unwrap();
    let cards: Vec<_> = board.cards.values().flatten().collect();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].correlation_id, first.correlation_id);
    let detail = fx
        .persistence
        .get_task_chain_detail(
            CollaborationReadScope {
                company_id: fx.company.id,
                visible_channel_ids: channels,
            },
            first.correlation_id,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        detail
            .tasks
            .iter()
            .map(|item| item.task.id)
            .collect::<HashSet<_>>(),
        ids.iter().copied().collect()
    );
    let threads = [first.thread_id.unwrap(), second.thread_id.unwrap()];
    let summaries = fx
        .persistence
        .list_thread_work_summary(&threads)
        .await
        .unwrap();
    for (thread, id) in threads.into_iter().zip(ids) {
        assert_eq!(summaries[&thread].task_id, *id);
        assert_eq!(summaries[&thread].activity, Some(ThreadActivity::Queued));
        let views = fx.threads.get_thread_history(thread).await.unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].task_id, Some(*id));
    }
}

pub(super) async fn assert_reply_kinds(fx: &Fixture, channel_id: Uuid, own: &str, sibling: &str) {
    let thread_id: Uuid = sqlx::query_scalar("SELECT id FROM threads WHERE channel_id = $1")
        .bind(channel_id)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
    let history = fx.threads.get_agent_history(thread_id).await.unwrap();
    assert!(
        history
            .iter()
            .any(|entry| entry.body.contains(own)
                && entry.entry_kind == ThreadEntryKind::Conversation)
    );
    assert!(history.iter().any(
        |entry| entry.body.contains(sibling) && entry.entry_kind == ThreadEntryKind::Delegation
    ));
}

pub(super) fn prompt(llm: &mut ScriptedLlm) -> String {
    let requests = llm.observed();
    assert_eq!(requests.len(), 1);
    requests[0]["messages"].to_string()
}

pub(super) async fn assert_redelivery(fx: &Fixture, raw: RawInboundPayload, ids: &[Uuid]) {
    let repeated = fx.threads.ingest_test_email(raw).await.unwrap();
    assert!(repeated.accepted);
    assert_eq!(repeated.task_ids, ids);
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM background_tasks WHERE company_id = $1), \
         (SELECT count(*) FROM message_deliveries WHERE company_id = $1)",
    )
    .bind(fx.company.id)
    .fetch_one(&fx.pool)
    .await
    .unwrap();
    assert_eq!(counts, (2, 2));
    assert_eq!(fx.transport.sent().len(), 2);
}
