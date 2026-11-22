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

/// Plan08 02C.4 (08G-P02-B2-R2-F1): a placement in an archived unit places nothing, so archive
/// and restore move an agent's direct-binding context as a roster write does, and refuse what
/// would leave an active role-bearing binding without exactly one department.
#[tokio::test]
async fn unit_archive_rechecks_direct_role_bindings_on_archive_and_restore() {
    use crate::application::ports::department::OpenWork;
    let direct = DirectRole::new().await;
    let f = &direct.f;
    let use_cases =
        crate::adapters::persistence::test_support::department_use_cases(&f.fixture.persistence);
    let (user, company) = (f.target.actor.user_id(), f.target.company.as_uuid());
    let [sales, support] = [direct.departments[0], direct.departments[1]];
    let refused = |error: AppError| format!("{error:?}").contains("binding.role_needs_department");
    let configured = direct.configure().await;
    use_cases
        .add_member(user, company, sales, direct.principal)
        .await
        .unwrap();
    let active = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();

    // Archiving the agent's only department would leave the active binding without one.
    let error = use_cases
        .archive_department(user, company, sales, OpenWork::Refuse)
        .await
        .err()
        .unwrap();
    assert!(refused(error));
    assert_eq!(direct.contexts().await, vec![sales.as_uuid()]);

    // With the binding off the archive goes through and the agent is placed nowhere.
    f.service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    use_cases
        .archive_department(user, company, sales, OpenWork::Refuse)
        .await
        .unwrap();
    assert!(direct.contexts().await.is_empty());

    // Placed in Support and active again while Sales is archived, a restore of Sales would make
    // the context ambiguous.
    use_cases
        .add_member(user, company, support, direct.principal)
        .await
        .unwrap();
    f.service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: f.state().await.revision,
        })
        .await
        .unwrap();
    let error = use_cases
        .restore_department(user, company, sales)
        .await
        .err()
        .unwrap();
    assert!(refused(error));
    assert_eq!(direct.contexts().await, vec![support.as_uuid()]);
}
