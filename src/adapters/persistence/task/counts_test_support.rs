//! Fixtures shared by the aggregate and HTTP stream tests. No globally claimable work.
use crate::{
    adapters::persistence::{PostgresPersistence, test_support::test_pool},
    entities::{task::TaskStatus, user::Viewer},
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct CountsFixture {
    pub persistence: Arc<PostgresPersistence>,
    pub company_id: Uuid,
    pub channels: [Uuid; 2],
    pub agents: [Uuid; 2],
    pub principals: [Uuid; 2],
    pub owner: Viewer,
}

impl CountsFixture {
    pub async fn new() -> Option<Self> {
        let persistence = Arc::new(PostgresPersistence::new(test_pool().await?));
        let suffix = Uuid::new_v4().simple().to_string();
        let email = format!("counts-{suffix}@example.com");
        persistence
            .create_user(&format!("counts-{suffix}"), &email, "hash")
            .await
            .unwrap();
        let user = UserPersistence::get_by_email(persistence.as_ref(), &email)
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            persistence.as_ref(),
            user.id,
            CompanyWrite {
                name: "Counts".into(),
                slug: format!("counts-{suffix}"),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut channels = Vec::new();
        let mut agents = Vec::new();
        let mut principals = Vec::new();
        for index in 0..2 {
            let channel = ChannelPersistence::create(
                persistence.as_ref(),
                company.id,
                ChannelWrite {
                    name: format!("Channel {index}"),
                    slug: format!("channel-{index}"),
                    enabled: false,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            channels.push(channel.id);
            let agent = AgentPersistence::create(
                persistence.as_ref(),
                company.id,
                AgentWrite {
                    name: format!("Agent {index}"),
                    slug: format!("agent-{index}"),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            agents.push(agent.id);
            principals.push(
                sqlx::query_scalar(
                    "SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2",
                )
                .bind(company.id)
                .bind(agent.id)
                .fetch_one(persistence.pool())
                .await
                .unwrap(),
            );
        }
        Some(Self {
            persistence,
            company_id: company.id,
            channels: channels.try_into().unwrap(),
            agents: agents.try_into().unwrap(),
            principals: principals.try_into().unwrap(),
            owner: Viewer {
                user_id: user.id,
                email: email.into(),
            },
        })
    }

    pub async fn task(
        &self,
        channel_id: Uuid,
        owner: Option<(Uuid, &str)>,
        status: TaskStatus,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let generation = (status == TaskStatus::Processing).then(Uuid::new_v4);
        sqlx::query("INSERT INTO background_tasks
            (id, company_id, channel_id, correlation_id, task_type, status, owner_principal_id,
             owner_principal_kind, run_at, worker_id, execution_generation, locked_at, lock_expires_at,
             transition_reason, transition_actor_kind, transition_actor_id)
            VALUES ($1, $2, $3, gen_random_uuid(), 'agent_run', $4, $5, $6,
             CURRENT_TIMESTAMP + INTERVAL '1 year', $7, $7,
             CASE WHEN $7::uuid IS NULL THEN NULL ELSE CURRENT_TIMESTAMP END,
             CASE WHEN $7::uuid IS NULL THEN NULL ELSE CURRENT_TIMESTAMP + INTERVAL '1 year' END,
             CASE WHEN $7::uuid IS NULL THEN NULL ELSE 'claimed' END,
             CASE WHEN $7::uuid IS NULL THEN NULL ELSE 'worker' END, $7)")
            .bind(id).bind(self.company_id).bind(channel_id).bind(status.as_str())
            .bind(owner.map(|o| o.0)).bind(owner.map(|o| o.1)).bind(generation)
            .execute(self.persistence.pool()).await.unwrap();
        id
    }
}
