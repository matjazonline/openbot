use super::*;
use crate::adapters::persistence::test_support::assert_actor_before_company;

#[tokio::test]
async fn delegated_admin_competing_schedule_writes_lock_the_actor_before_the_company() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = ManagedSchedules::on(database.pool.clone()).await;
    let p = f.p();
    let actor = f.d.scope(f.d.f.member).actor;
    let company = f.d.company();
    let pool = p.pool();
    let schedule = assert_actor_before_company(
        pool,
        company.as_uuid(),
        actor.user_id(),
        SchedulePersistence::create(p, actor, company, f.desk, write(f.binding, "Account-first")),
    )
    .await
    .unwrap();
    assert_actor_before_company(
        pool,
        company.as_uuid(),
        actor.user_id(),
        SchedulePersistence::update(
            p,
            actor,
            company,
            schedule.id,
            f.desk,
            write(f.binding, "Edited"),
        ),
    )
    .await
    .unwrap();
    assert_actor_before_company(
        pool,
        company.as_uuid(),
        actor.user_id(),
        p.set_enabled(actor, company, schedule.id, false),
    )
    .await
    .unwrap();
    let owner = f.d.scope(f.d.f.owner).actor;
    assert_actor_before_company(
        pool,
        company.as_uuid(),
        owner.user_id(),
        p.capture_now(owner, company, f.schedule.id, "account-first"),
    )
    .await
    .unwrap();
    assert_actor_before_company(
        pool,
        company.as_uuid(),
        actor.user_id(),
        SchedulePersistence::delete(p, actor, company, schedule.id),
    )
    .await
    .unwrap();
    assert!(p.get(company, schedule.id).await.unwrap().is_none());
}
