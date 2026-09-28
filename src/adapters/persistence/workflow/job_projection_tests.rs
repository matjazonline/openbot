use super::*;
use crate::{
    entities::{dashboard::DashboardWindow, task::TaskStatus},
    services::dashboard_snapshot::DashboardPersistence,
};

#[tokio::test]
async fn workflow_job_projection_member_asks_exclude_workflow_auxiliary_rows() {
    use crate::use_cases::company_invite::CompanyInvitePersistence;
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    let user = f.binding.target.actor.user_id();
    let (transport, namespace, subject): (String, String, String) = sqlx::query_as(
        "SELECT identity.transport, identity.namespace, identity.subject FROM participant_identities AS identity JOIN principals AS principal ON principal.id = identity.principal_id AND principal.company_id = identity.company_id WHERE principal.company_id = $1 AND principal.user_id = $2 AND identity.transport = 'email' LIMIT 1",
    ).bind(company).bind(user).fetch_one(p.pool()).await.unwrap();
    for task in [workflow, legacy] {
        let outreach = Uuid::new_v4();
        let inserted = sqlx::query("INSERT INTO task_outreaches (id,task_id,company_id,status,required_threshold_percent,expires_at,outreach_key,subject,body) VALUES ($1,$2,$3,'waiting',100,CURRENT_TIMESTAMP + interval '2 days',$1::text,'Ask','Body')")
            .bind(outreach).bind(task).bind(company).execute(p.pool()).await;
        if task == workflow {
            constraint(inserted.unwrap_err(), "legacy_auxiliary_task_queue_kind");
            continue;
        }
        inserted.unwrap();
        sqlx::query("INSERT INTO task_outreach_targets (id,outreach_id,company_id,email,target_kind,external_transport,external_namespace,external_subject,status) VALUES (gen_random_uuid(),$1,$2,$3,'external',$4,$5,$6,'active')")
            .bind(outreach).bind(company).bind(format!("{subject}@{namespace}"))
            .bind(&transport).bind(&namespace).bind(&subject).execute(p.pool()).await.unwrap();
    }
    let work = p.member_work_at_stake(company, user).await.unwrap();
    assert_eq!(
        work.delegated_asks
            .iter()
            .map(|ask| ask.task_id)
            .collect::<Vec<_>>(),
        vec![legacy]
    );
}

#[tokio::test]
async fn workflow_job_projection_dashboard_excludes_shared_attempts_and_jobs() {
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    sqlx::query("UPDATE background_tasks SET created_at = CURRENT_TIMESTAMP - interval '10 minutes' WHERE company_id = $1")
        .bind(company).execute(p.pool()).await.unwrap();
    for (task, tokens, seconds, number) in [(legacy, 3_i64, 1_i32, 1_i32), (workflow, 900, 90, 2)] {
        sqlx::query("INSERT INTO task_attempts (id,task_id,attempt_number,execution_generation,status,worker_id,machine_id,started_at,finished_at,prompt_tokens,completion_tokens) VALUES (gen_random_uuid(),$1,$4,gen_random_uuid(),'completed',gen_random_uuid(),'projection',CURRENT_TIMESTAMP - make_interval(secs => $3),CURRENT_TIMESTAMP,$2,$2)")
            .bind(task).bind(tokens).bind(seconds).bind(number).execute(p.pool()).await.unwrap();
    }
    for scope in [None, Some(company)] {
        let snapshot = p
            .dashboard_snapshot(scope, DashboardWindow::last_hour())
            .await
            .unwrap();
        assert_eq!(snapshot.tasks.count_of(TaskStatus::Pending), 1);
        assert_eq!(snapshot.tasks.due_now, 1);
        assert_eq!(snapshot.queue_depth.last().unwrap().open, 1);
        assert_eq!(
            snapshot
                .outstanding
                .iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            vec![legacy]
        );
        assert_eq!(snapshot.attempts.attempts, 1);
        assert_eq!(snapshot.attempts.retries, 0);
        assert_eq!(snapshot.attempts.prompt_tokens, 3);
        assert_eq!(snapshot.attempts.p95_ms, Some(1000));
        assert_eq!(
            snapshot
                .retry_rate
                .iter()
                .map(|bucket| bucket.attempts)
                .sum::<i64>(),
            1
        );
        assert_eq!(
            snapshot
                .latency
                .iter()
                .filter_map(|bucket| bucket.p95_ms)
                .collect::<Vec<_>>(),
            vec![1000]
        );
    }
    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE company_id = $1")
        .bind(company)
        .execute(p.pool())
        .await
        .unwrap();
    for scope in [None, Some(company)] {
        let snapshot = p
            .dashboard_snapshot(scope, DashboardWindow::last_hour())
            .await
            .unwrap();
        assert_eq!(snapshot.throughput_total(), 1);
        assert_eq!(snapshot.tasks.count_of(TaskStatus::Completed), 1);
    }
}

#[tokio::test]
async fn workflow_job_projection_thread_fallback_preserves_message_and_legacy_task() {
    use crate::{
        entities::message::{MessageDirection, MessageRole},
        use_cases::thread::{MessageAuthorWrite, MessageWrite, ThreadPersistence},
    };
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let thread = f.thread;
    let correlation: Uuid =
        sqlx::query_scalar("SELECT correlation_id FROM background_tasks WHERE id = $1")
            .bind(workflow)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let message = p
        .create_message(&MessageWrite::internal(
            thread,
            MessageAuthorWrite::Platform,
            "Projection",
            "Body",
            MessageDirection::Inbound,
            MessageRole::Human,
            correlation.into(),
        ))
        .await
        .unwrap();
    let views = p.list_thread_message_views(thread).await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].canonical_id, message.canonical_id);
    assert_eq!(
        views[0].task_id, None,
        "workflow correlation must not decorate a legacy view"
    );
    sqlx::query("UPDATE background_tasks SET thread_id = $2, correlation_id = $3 WHERE id = $1")
        .bind(legacy)
        .bind(thread)
        .bind(correlation)
        .execute(p.pool())
        .await
        .unwrap();
    assert_eq!(
        p.list_thread_message_views(thread).await.unwrap()[0].task_id,
        Some(legacy)
    );
}

#[tokio::test]
async fn workflow_job_projection_channel_removal_stops_only_legacy_work() {
    use crate::{
        adapters::persistence::channel_assignment::{RemovalBudget, update_channel_within},
        use_cases::channel::ChannelUpdate,
    };
    let (f, workflow, legacy) = mixed_fixture().await;
    let p = &f.binding.fixture.persistence;
    let company = f.binding.target.company.as_uuid();
    sqlx::query("UPDATE background_tasks SET owner_principal_id = (SELECT id FROM principals WHERE company_id = $2 AND kind = 'agent'), owner_principal_kind = 'agent' WHERE id = $1")
        .bind(legacy).bind(company).execute(p.pool()).await.unwrap();
    let removal = update_channel_within(
        p.pool(),
        ChannelUpdate {
            company_id: company,
            channel_id: f.channel,
            actor_user_id: f.binding.target.actor.user_id(),
            write: ChannelWrite {
                name: "Jobs".into(),
                slug: "jobs".into(),
                agent_ids: Some(vec![]),
                ..Default::default()
            },
        },
        RemovalBudget::DEFAULT,
    )
    .await
    .unwrap();
    assert_eq!(removal.stopped_tasks, 1);
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id,status FROM background_tasks WHERE company_id = $1")
            .bind(company)
            .fetch_all(p.pool())
            .await
            .unwrap();
    assert!(rows.contains(&(workflow, "pending".into())));
    assert!(rows.contains(&(legacy, "stopped".into())));
}
