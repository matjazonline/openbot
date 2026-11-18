//! Definition management uses current active bindings, with immutable creating-unit fallback.
use super::*;
use crate::application::ports::channel::ChannelPersistence;
use crate::application::ports::department::DepartmentWrite;
use crate::application::use_cases::administration::{AdministrationPersistence, ManagementTarget};
use crate::domain::workflow::WorkflowId;
use crate::entities::{department::DepartmentId, transport::PrincipalId};

struct ManagedDefinitions {
    f: Fixture,
    a: DepartmentId,
    b: DepartmentId,
    a_actor: WorkflowActor,
    b_actor: WorkflowActor,
}

struct ActiveBinding {
    id: Uuid,
    revision: u64,
    desk: Uuid,
}

impl ManagedDefinitions {
    async fn new() -> Option<Self> {
        let f = Fixture::new().await?;
        let a_actor = WorkflowActor::authenticated(f.member).unwrap();
        let b_actor =
            WorkflowActor::authenticated(teammate(&f.persistence, f.company, "member").await)
                .unwrap();
        let a = unit(&f, a_actor, "research", None).await;
        let b = unit(&f, b_actor, "sales", None).await;
        Some(Self {
            f,
            a,
            b,
            a_actor,
            b_actor,
        })
    }

    async fn published(&self) -> (Uuid, Uuid) {
        self.f
            .published(
                self.a_actor.user_id(),
                self.f.company,
                example("data.map", Uuid::new_v4()),
            )
            .await
    }

    async fn active(
        &self,
        actor: WorkflowActor,
        unit: DepartmentId,
        version: Uuid,
        slug: &str,
    ) -> ActiveBinding {
        let desk = ChannelPersistence::create(
            &self.f.persistence,
            WorkflowActor::authenticated(self.f.owner).unwrap(),
            self.f.company,
            ChannelWrite {
                name: slug.into(),
                slug: slug.into(),
                department_id: Some(unit),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let id = Uuid::new_v4();
        let (status, state) = self
            .f
            .configure_on(
                actor.user_id(),
                self.f.company,
                id,
                None,
                version,
                json!({"kind":"channel","channel_id":desk.id}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{state}");
        let (status, state) = self
            .f
            .activate(
                actor.user_id(),
                self.f.company,
                id,
                state["state_revision"].as_u64().unwrap(),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{state}");
        ActiveBinding {
            id,
            revision: state["state_revision"].as_u64().unwrap(),
            desk: desk.id,
        }
    }

    async fn pause(&self, actor: WorkflowActor, binding: &ActiveBinding) {
        let (status, body) = answer(
            bindings::deactivate(
                self.f.state(),
                by(actor.user_id()),
                Path((self.f.company, binding.id)),
                body(json!({"expected_state_revision":binding.revision})),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    async fn manages(&self, actor: WorkflowActor, workflow: Uuid) -> bool {
        self.f
            .persistence
            .management_role(
                actor.user_id(),
                company(self.f.company),
                ManagementTarget::Definition(WorkflowId::new(workflow)),
            )
            .await
            .unwrap()
            .is_some()
    }

    async fn creating_unit(&self, workflow: Uuid) -> Uuid {
        sqlx::query_scalar(
            "SELECT managing_department_id FROM workflow_definitions WHERE company_id=$1 AND id=$2",
        )
        .bind(self.f.company)
        .bind(workflow)
        .fetch_one(self.f.persistence.pool())
        .await
        .unwrap()
    }

    async fn events(&self, workflow: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM workflow_definition_events WHERE company_id=$1 AND workflow_id=$2")
            .bind(self.f.company).bind(workflow).fetch_one(self.f.persistence.pool()).await.unwrap()
    }
}

async fn unit(
    f: &Fixture,
    lead: WorkflowActor,
    slug: &str,
    parent: Option<DepartmentId>,
) -> DepartmentId {
    let principal: Uuid = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(f.company)
    .bind(lead.user_id())
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    crate::adapters::persistence::test_support::department_use_cases(&f.persistence)
        .create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                name: slug.into(),
                slug: slug.into(),
                parent_id: parent,
                ..Default::default()
            },
            Some(PrincipalId::new(principal)),
        )
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn delegated_admin_definition_unbound_crud_uses_the_recorded_creating_unit() {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let (workflow, _) = m.published().await;
    assert_eq!(m.creating_unit(workflow).await, m.a.as_uuid());
    let source = example("data.map", workflow);
    assert_eq!(
        m.f.save(m.b_actor.user_id(), m.f.company, workflow, Some(1), &source)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, saved) =
        m.f.save(m.a_actor.user_id(), m.f.company, workflow, Some(1), &source)
            .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["revision"], 2);
    let (status, published) =
        m.f.publish(m.a_actor.user_id(), m.f.company, workflow, 2, "second")
            .await;
    assert_eq!(status, StatusCode::OK, "{published}");
    let (status, read) = answer(
        definitions::read(
            m.f.state(),
            by(m.a_actor.user_id()),
            Path((m.f.company, workflow)),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert!(read["source"].as_str().unwrap().contains("data.map"));
    assert_eq!(
        answer(
            definitions::read(
                m.f.state(),
                by(m.b_actor.user_id()),
                Path((m.f.company, workflow))
            )
            .await
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        answer(
            definitions::archive(
                m.f.state(),
                by(m.b_actor.user_id()),
                Path((m.f.company, workflow)),
                body(json!({"expected_revision":2}))
            )
            .await
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        answer(
            definitions::archive(
                m.f.state(),
                by(m.a_actor.user_id()),
                Path((m.f.company, workflow)),
                body(json!({"expected_revision":2}))
            )
            .await
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(m.creating_unit(workflow).await, m.a.as_uuid());
}

#[tokio::test]
async fn manages_unit_definition_uses_any_active_binding_and_recorded_unit_only_when_unbound() {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let (workflow, version) = m.published().await;
    let b = m.active(m.b_actor, m.b, version, "b-binding").await;
    assert!(!m.manages(m.a_actor, workflow).await);
    assert!(m.manages(m.b_actor, workflow).await);
    let source = example("data.map", workflow);
    assert_eq!(
        m.f.save(m.a_actor.user_id(), m.f.company, workflow, Some(1), &source)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        m.f.save(m.b_actor.user_id(), m.f.company, workflow, Some(1), &source)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        m.f.publish(
            m.b_actor.user_id(),
            m.f.company,
            workflow,
            2,
            "bound-manager"
        )
        .await
        .0,
        StatusCode::OK
    );
    let a = m.active(m.a_actor, m.a, version, "a-binding").await;
    assert!(m.manages(m.a_actor, workflow).await && m.manages(m.b_actor, workflow).await);
    m.pause(m.b_actor, &b).await;
    assert!(!m.manages(m.b_actor, workflow).await);
    m.pause(m.a_actor, &a).await;
    assert!(m.manages(m.a_actor, workflow).await && !m.manages(m.b_actor, workflow).await);
    assert_eq!(m.creating_unit(workflow).await, m.a.as_uuid());
}

#[tokio::test]
async fn delegated_admin_definition_old_binding_revisions_confer_no_authority() {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let (workflow, version) = m.published().await;
    let active = m.active(m.b_actor, m.b, version, "moved-version").await;
    let (_, other_version) =
        m.f.published(m.f.owner, m.f.company, example("data.map", Uuid::new_v4()))
            .await;
    let (status, result) =
        m.f.configure_on(
            m.b_actor.user_id(),
            m.f.company,
            active.id,
            Some(active.revision),
            other_version,
            json!({"kind":"channel","channel_id":active.desk}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(!m.manages(m.b_actor, workflow).await);
    assert!(m.manages(m.a_actor, workflow).await);
    assert_eq!(m.creating_unit(workflow).await, m.a.as_uuid());
}

#[tokio::test]
async fn delegated_admin_definition_sibling_foreign_missing_and_agent_writes_hide_revision_conflicts()
 {
    let Some(m) = ManagedDefinitions::new().await else {
        return;
    };
    let (workflow, _) = m.published().await;
    let source = example("data.map", workflow);
    for expected in [None, Some(1), Some(100)] {
        assert_eq!(
            m.f.save(
                m.b_actor.user_id(),
                m.f.company,
                workflow,
                expected,
                &source
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            m.f.save(m.a_actor.user_id(), m.f.other, workflow, expected, &source)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        m.f.save(
            m.a_actor.user_id(),
            m.f.company,
            Uuid::new_v4(),
            Some(1),
            &source
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        m.f.save(
            m.a_actor.user_id(),
            m.f.company,
            workflow,
            Some(100),
            &source
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let agent = AgentPersistence::create(
        &m.f.persistence,
        m.f.company,
        AgentWrite {
            name: "Not a person".into(),
            slug: "not-a-person".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        m.f.save(agent.id, m.f.company, workflow, Some(1), &source)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[path = "delegated_definition_boundary_tests.rs"]
mod boundaries;
#[path = "delegated_definition_integrity_tests.rs"]
mod integrity;
#[path = "delegated_definition_race_tests.rs"]
mod races;
#[path = "delegated_definition_replay_tests.rs"]
mod replays;
