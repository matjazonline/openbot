//! Production binding and roster writers; the shared Preflight fixture replaces only the
//! outer authoring lookup. Persistence must enforce the current target context itself.
use super::*;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use std::time::Duration;

struct DirectRole {
    f: BindingFixture,
    principal: PrincipalId,
    departments: Vec<DepartmentId>,
}

impl DirectRole {
    async fn new() -> Self {
        let f = fixture().await;
        let p = &f.fixture.persistence;
        let agent = AgentPersistence::create(
            p,
            f.target.company.as_uuid(),
            AgentWrite {
                name: "Direct role agent".into(),
                slug: "direct-role-agent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let principal = PrincipalId::new(
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
                .bind(f.target.company.as_uuid())
                .bind(agent.id)
                .fetch_one(p.pool())
                .await
                .unwrap(),
        );
        let use_cases = crate::adapters::persistence::test_support::department_use_cases(p);
        let mut departments = Vec::new();
        for slug in ["sales", "support"] {
            departments.push(
                use_cases
                    .create_department(
                        f.target.actor.user_id(),
                        f.target.company.as_uuid(),
                        DepartmentWrite {
                            name: slug.into(),
                            slug: slug.into(),
                            ..Default::default()
                        },
                        None,
                    )
                    .await
                    .unwrap()
                    .id,
            );
        }
        Self {
            f,
            principal,
            departments,
        }
    }

    async fn configure(&self) -> BindingState {
        let mut request = self.f.request(None);
        request.association = RelatedAssociation::Principal(self.principal);
        self.f.service().configure(request).await.unwrap()
    }

    async fn contexts(&self) -> Vec<Uuid> {
        sqlx::query_scalar(
            "SELECT department FROM workflow_target_departments($1,NULL,$2) AS department",
        )
        .bind(self.f.target.company.as_uuid())
        .bind(self.principal.as_uuid())
        .fetch_all(self.f.fixture.persistence.pool())
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn direct_binding_role_activation_and_roster_mutations_require_exactly_one_department() {
    let direct = DirectRole::new().await;
    let f = &direct.f;
    let p = &f.fixture.persistence;
    let use_cases = crate::adapters::persistence::test_support::department_use_cases(p);
    let configured = direct.configure().await;
    let activity = || SetBindingActivity {
        target: f.target,
        expected: configured.revision,
    };
    let error = f.service().activate(activity()).await.err().unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    use_cases
        .add_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            direct.departments[0],
            direct.principal,
        )
        .await
        .unwrap();
    let active = f.service().activate(activity()).await.unwrap();
    let error = use_cases
        .add_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            direct.departments[1],
            direct.principal,
        )
        .await
        .err()
        .unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    assert_eq!(
        direct.contexts().await,
        vec![direct.departments[0].as_uuid()]
    );
    let error = use_cases
        .remove_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            direct.departments[0],
            direct.principal,
        )
        .await
        .err()
        .unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    assert_eq!(
        direct.contexts().await,
        vec![direct.departments[0].as_uuid()]
    );
    f.service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    use_cases
        .add_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            direct.departments[1],
            direct.principal,
        )
        .await
        .unwrap();
    assert_eq!(direct.contexts().await.len(), 2);
    let error = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: f.state().await.revision,
        })
        .await
        .err()
        .unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
}

#[tokio::test]
async fn direct_binding_competing_activation_and_roster_write_preserve_unambiguous_context() {
    let direct = DirectRole::new().await;
    let f = &direct.f;
    let p = &f.fixture.persistence;
    let use_cases = crate::adapters::persistence::test_support::department_use_cases(p);
    use_cases
        .add_member(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            direct.departments[0],
            direct.principal,
        )
        .await
        .unwrap();
    let configured = direct.configure().await;
    let prepared = f.activity(configured.revision).await;
    let mut barrier = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.target.company.as_uuid())
        .execute(&mut *barrier)
        .await
        .unwrap();
    let contenders = async {
        tokio::join!(
            p.save_binding(prepared),
            use_cases.add_member(
                f.target.actor.user_id(),
                f.target.company.as_uuid(),
                direct.departments[1],
                direct.principal
            )
        )
    };
    let release = async {
        barrier.commit().await.unwrap();
    };
    let ((activation, placement), ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(contenders, release)
    })
    .await
    .expect("both production writers complete without a deadlock/timeout refusal");
    assert_ne!(
        activation.is_ok(),
        placement.is_ok(),
        "exactly one incompatible edit may commit"
    );
    let state = f.state().await;
    assert_eq!(state.active, activation.is_ok());
    assert_eq!(
        direct.contexts().await.len(),
        if state.active { 1 } else { 2 }
    );
    let error = activation.err().or_else(|| placement.err()).unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
}
