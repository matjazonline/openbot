use super::*;
use crate::use_cases::{agent::AgentPersistence, channel::ChannelPersistence};

#[tokio::test]
async fn harness_fence_covers_owner_only_tasks() {
    let _queue_guard = super::super::test_support::UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let p = PostgresPersistence::new(pool.clone());
    let channel = ChannelPersistence::create(
        &p,
        fixture.company,
        ChannelWrite {
            name: "Unassigned".into(),
            slug: "unassigned".into(),
            enabled: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let task = Uuid::new_v4();
    let mut creating = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id, correlation_id, company_id, channel_id, owner_principal_id, owner_principal_kind, task_type, payload) VALUES ($1,$1,$2,$3,(SELECT id FROM principals WHERE company_id = $2 AND agent_id = $4),'agent','test','{}')")
        .bind(task).bind(fixture.company).bind(channel.id).bind(fixture.agent)
        .execute(&mut *creating).await.unwrap();
    let update = change_harness(&pool, fixture.agent);
    tokio::pin!(update);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut update)
            .await
            .is_err()
    );
    creating.commit().await.unwrap();
    assert_fenced(
        tokio::time::timeout(Duration::from_secs(5), update)
            .await
            .unwrap(),
    );
    sqlx::query("UPDATE background_tasks SET status = 'completed' WHERE id = $1")
        .bind(task)
        .execute(&pool)
        .await
        .unwrap();
    change_harness(&pool, fixture.agent).await.unwrap();

    CompanyPersistence::delete(&p, fixture.company)
        .await
        .unwrap();
}

#[tokio::test]
async fn harness_fence_covers_library_channel_assignments() {
    let _queue_guard = super::super::test_support::UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let p = PostgresPersistence::new(pool.clone());
    // Library agents have no company/principal; their channel relation must still fence them.
    let library = p
        .create_library(AgentWrite {
            name: "Shared".into(),
            slug: format!("shared-{}", Uuid::new_v4().simple()),
            harness_kind: Some(crate::entities::harness::HarnessKind::AiAgents),
            ..Default::default()
        })
        .await
        .unwrap();
    let shared = ChannelPersistence::create(
        &p,
        fixture.company,
        ChannelWrite {
            name: "Shared".into(),
            slug: "shared".into(),
            enabled: false,
            agent_ids: Some(vec![library.id]),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut creating = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO background_tasks (id, correlation_id, company_id, channel_id, task_type, payload) VALUES (gen_random_uuid(),gen_random_uuid(),$1,$2,'test','{}')")
        .bind(fixture.company).bind(shared.id).execute(&mut *creating).await.unwrap();
    let update = change_harness(&pool, library.id);
    tokio::pin!(update);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut update)
            .await
            .is_err()
    );
    creating.commit().await.unwrap();
    assert_fenced(
        tokio::time::timeout(Duration::from_secs(5), update)
            .await
            .unwrap(),
    );
    CompanyPersistence::delete(&p, fixture.company)
        .await
        .unwrap();
    change_harness(&pool, library.id).await.unwrap();
    AgentPersistence::delete(&p, library.id).await.unwrap();
}

#[tokio::test]
async fn task_creation_waits_for_a_competing_harness_change() {
    let _queue_guard = super::super::test_support::UNSCOPED_CLAIM.lock().await;
    let Some(pool) = test_pool().await else {
        return;
    };
    let fixture = fixture(&pool).await;
    let mut editing = pool.begin().await.unwrap();
    sqlx::query("UPDATE agents SET harness_kind = 'rig' WHERE id = $1")
        .bind(fixture.agent)
        .execute(&mut *editing)
        .await
        .unwrap();
    let insert = sqlx::query("INSERT INTO background_tasks (id, correlation_id, company_id, channel_id, task_type, payload) VALUES (gen_random_uuid(),gen_random_uuid(),$1,$2,'test','{}')")
        .bind(fixture.company).bind(fixture.channel).execute(&pool);
    tokio::pin!(insert);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut insert)
            .await
            .is_err()
    );
    editing.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), insert)
        .await
        .unwrap()
        .unwrap();
    assert_fenced(
        sqlx::query("UPDATE agents SET harness_kind = 'ai_agents' WHERE id = $1")
            .bind(fixture.agent)
            .execute(&pool)
            .await,
    );
    CompanyPersistence::delete(&PostgresPersistence::new(pool.clone()), fixture.company)
        .await
        .unwrap();
}
