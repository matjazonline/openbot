//! A company with workflow jobs at every association scope, for the projections that count them.
use super::*;

async fn scoped_fixture() -> (JobFixture, Uuid) {
    let f = JobFixture::new().await;
    let pool = f.binding.fixture.persistence.pool();
    let mut db = pool.acquire().await.unwrap();
    super::job_association_tests::assert_nullable_gate(&f).await;
    for association in [
        RunAssociation::default(),
        RunAssociation {
            channel: Some(f.channel),
            thread: None,
        },
    ] {
        let execution = super::job_association_tests::scoped_execution(&f, association).await;
        super::job_association_tests::scoped_job(
            &mut db,
            f.binding.target.company.as_uuid(),
            execution,
            association,
        )
        .await
        .unwrap();
    }
    let workflow = f.insert(&mut db, f.payload()).await.unwrap();
    let agent = Uuid::new_v4();
    sqlx::query("INSERT INTO agents (id, company_id, name, slug, created_by) VALUES ($1, $2, 'Worker', 'worker', $3)")
        .bind(agent).bind(f.binding.target.company.as_uuid())
        .bind(json!({"actor_type":"system", "actor_id":null, "actor_name":"Test"}))
        .execute(&mut *db).await.unwrap();
    sqlx::query("INSERT INTO principals (id, company_id, kind, agent_id, display_label) VALUES (gen_random_uuid(), $1, 'agent', $2, 'Worker')")
        .bind(f.binding.target.company.as_uuid()).bind(agent).execute(&mut *db).await.unwrap();
    drop(db);
    (f, workflow)
}

#[path = "job_projection_tests.rs"]
mod job_projection_tests;
