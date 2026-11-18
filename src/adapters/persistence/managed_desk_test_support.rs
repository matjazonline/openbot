//! Existing fixtures create desks through the same checked writer as user requests.
use super::*;
use crate::app_error::AppResult;
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::workflow::WorkflowActor;
use crate::entities::channel::Channel;

pub(crate) async fn company_owner_actor(
    persistence: &PostgresPersistence,
    company: Uuid,
) -> WorkflowActor {
    let owner = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(company)
        .fetch_one(persistence.pool())
        .await
        .expect("the fixture's existing company owner");
    WorkflowActor::authenticated(owner).expect("the fixture's current owner actor")
}

pub(crate) async fn create_channel(
    persistence: &PostgresPersistence,
    company: Uuid,
    write: ChannelWrite,
) -> AppResult<Channel> {
    ChannelPersistence::create(
        persistence,
        company_owner_actor(persistence, company).await,
        company,
        write,
    )
    .await
}

/// The exact waiting backend, for checking the next edge of a competing write chain.
pub(crate) async fn wait_until_blocked_by(pool: &PgPool, blocker: i32) -> i32 {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)) ORDER BY pid LIMIT 1")
                .bind(blocker).fetch_optional(pool).await.unwrap();
            if let Some(pid) = blocked { return pid; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("the actual competing caller waits on this exact writer")
}

pub(crate) enum WriteBarrier {
    DefinitionEvents,
    BindingEvents,
    DeskGrants,
    Agents,
}
impl WriteBarrier {
    fn sql(&self) -> &'static str {
        match self {
            Self::DefinitionEvents => "LOCK TABLE workflow_definition_events IN SHARE MODE",
            Self::BindingEvents => "LOCK TABLE workflow_binding_events IN SHARE MODE",
            Self::DeskGrants => "LOCK TABLE channel_principal_grants IN SHARE MODE",
            Self::Agents => "LOCK TABLE agents IN SHARE MODE",
        }
    }
}

pub(crate) struct CompetingWrites<A, B> {
    pub first: A,
    pub second: B,
}

/// Both futures are real public writers. Hold the first after its company lock, observe the
/// second waiting on that exact backend, then release the first to commit in the proved order.
pub(crate) async fn competing_writes<A, B>(
    pool: &PgPool,
    table: WriteBarrier,
    first: impl std::future::Future<Output = A>,
    second: impl std::future::Future<Output = B>,
) -> CompetingWrites<A, B> {
    let mut barrier = pool.begin().await.unwrap();
    sqlx::query(table.sql())
        .execute(&mut *barrier)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    let second = async {
        wait_until_blocked_by(pool, blocker).await;
        second.await
    };
    let release = async {
        let writer = wait_until_blocked_by(pool, blocker).await;
        wait_until_blocked_by(pool, writer).await;
        barrier.commit().await.unwrap();
    };
    let (first, second, ()) = tokio::join!(first, second, release);
    CompetingWrites { first, second }
}

/// A real writer must wait for its actor while leaving the company available.
pub(crate) async fn assert_actor_before_company<T>(
    pool: &PgPool,
    company: Uuid,
    actor: Uuid,
    writer: impl std::future::Future<Output = AppResult<T>>,
) -> AppResult<T> {
    let mut account = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(actor)
        .execute(&mut *account)
        .await
        .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *account)
        .await
        .unwrap();
    let observe = async {
        wait_until_blocked_by(pool, blocker).await;
        let mut company_probe = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE NOWAIT")
            .bind(company)
            .execute(&mut *company_probe)
            .await
            .expect("a writer blocked on its actor has not locked the company");
        company_probe.rollback().await.unwrap();
        account.commit().await.unwrap();
    };
    let (result, ()) = tokio::join!(writer, observe);
    result
}

/// A natural manager-without-View case: lead of the grandparent, two levels above the desk.
pub(crate) async fn delegated_unit(
    persistence: &PostgresPersistence,
    company: crate::domain::workflow::CompanyId,
    owner: WorkflowActor,
    lead: WorkflowActor,
) -> crate::entities::department::DepartmentId {
    use crate::application::ports::department::DepartmentWrite;
    use crate::application::use_cases::department::DepartmentUseCases;
    let principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(company.as_uuid())
    .bind(lead.user_id())
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    let cases = DepartmentUseCases::new(
        std::sync::Arc::new(persistence.clone()),
        std::sync::Arc::new(
            crate::adapters::persistence::department::PostgresDepartments::new(
                persistence.clone(),
                provisioning_templates(),
            ),
        ),
    );
    let mut parent_id = None;
    for (name, selected) in [
        (
            "Delegated",
            Some(crate::entities::transport::PrincipalId::new(principal)),
        ),
        ("Field", None),
        ("Remote", None),
    ] {
        let unit = cases
            .create_department(
                owner.user_id(),
                company.as_uuid(),
                DepartmentWrite {
                    name: name.into(),
                    slug: name.to_lowercase(),
                    parent_id,
                    ..Default::default()
                },
                selected,
            )
            .await
            .unwrap();
        parent_id = Some(unit.id);
    }
    parent_id.unwrap()
}
