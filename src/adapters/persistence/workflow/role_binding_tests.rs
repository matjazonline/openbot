use super::binding_tests::BindingFixture;
use super::*;
use crate::application::ports::channel::{ChannelPersistence, ChannelUpdate, ChannelWrite};
use crate::application::ports::department::DepartmentWrite;
use crate::entities::department::{DepartmentId, DepartmentRole};
use crate::entities::transport::PrincipalId;

async fn fixture() -> BindingFixture {
    let mut source: serde_json::Value =
        serde_json::from_str(&registry::example("agent.run").unwrap().source).unwrap();
    source["steps"]["start"]["with"]
        .as_object_mut()
        .unwrap()
        .remove("agent");
    source["steps"]["start"]["with"]["role"] = serde_json::json!({"literal":"lead"});
    BindingFixture::from_source(source).await
}

async fn department(f: &BindingFixture) -> DepartmentId {
    crate::adapters::persistence::test_support::department_use_cases(&f.fixture.persistence)
        .create_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            DepartmentWrite {
                name: "Team".into(),
                slug: "team".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

fn write(department: Option<DepartmentId>) -> ChannelWrite {
    ChannelWrite {
        name: "Intake".into(),
        slug: "intake".into(),
        enabled: true,
        department_id: department,
        ..Default::default()
    }
}

async fn desk(f: &BindingFixture, department: Option<DepartmentId>) -> Uuid {
    crate::adapters::persistence::test_support::create_channel(
        &f.fixture.persistence,
        f.target.company.as_uuid(),
        write(department),
    )
    .await
    .unwrap()
    .id
}

async fn configure(f: &BindingFixture, desk: Uuid) -> BindingState {
    let mut request = f.request(None);
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(desk));
    f.service().configure(request).await.unwrap()
}

#[tokio::test]
async fn workflow_role_binding_company_requires_context_but_default_desk_has_root_context() {
    let f = fixture().await;
    let configured = f.service().configure(f.request(None)).await.unwrap();
    let error = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .err()
        .unwrap();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    assert!(format!("{error:?}").contains("start"));
    assert!(!f.state().await.active);
    let f = fixture().await;
    let desk = desk(&f, None).await;
    let configured = configure(&f, desk).await;
    f.service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    assert!(
        f.state().await.active,
        "a default desk has its root context"
    );
    let owned = ChannelPersistence::get_by_id(&f.fixture.persistence, desk)
        .await
        .unwrap()
        .unwrap();
    let is_root: bool =
        sqlx::query_scalar("SELECT is_root FROM departments WHERE company_id=$1 AND id=$2")
            .bind(f.target.company.as_uuid())
            .bind(owned.department_id.as_uuid())
            .fetch_one(f.fixture.persistence.pool())
            .await
            .unwrap();
    assert!(is_root);
}

#[tokio::test]
async fn workflow_role_binding_preserves_required_owner_and_allows_reassignment() {
    let f = fixture().await;
    let department = department(&f).await;
    let desk = desk(&f, Some(department)).await;
    let configured = configure(&f, desk).await;
    let active = f
        .service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    let p = &f.fixture.persistence;
    let update = ChannelUpdate {
        actor: f.target.actor,
        company_id: f.target.company.as_uuid(),
        channel_id: desk,
        write: write(None),
    };
    let kept = ChannelPersistence::update(p, update.clone()).await.unwrap();
    assert_eq!(kept.department_id, department);
    let error = sqlx::query("UPDATE channels SET department_id=NULL WHERE company_id=$1 AND id=$2")
        .bind(f.target.company.as_uuid())
        .bind(desk)
        .execute(p.pool())
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23502")
    );
    let root = crate::entities::department::DepartmentId::new(
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.target.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap(),
    );
    let mut tx = p.pool().begin().await.unwrap();
    crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut tx,
        f.target.company,
        desk,
        root,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        ChannelPersistence::get_by_id(p, desk)
            .await
            .unwrap()
            .unwrap()
            .department_id,
        root
    );
    assert!(f.state().await.active);
    f.service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    assert_eq!(
        ChannelPersistence::update(p, update)
            .await
            .unwrap()
            .department_id,
        root
    );
    assert!(!f.state().await.active);
}

#[tokio::test]
async fn workflow_role_binding_competing_activation_and_reassignment_keep_required_context() {
    let f = fixture().await;
    let department = department(&f).await;
    let desk = desk(&f, Some(department)).await;
    let configured = configure(&f, desk).await;
    let activate = f.activity(configured.revision).await;
    let p = &f.fixture.persistence;
    let root = crate::entities::department::DepartmentId::new(
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.target.company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap(),
    );
    let mut gate = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.target.company.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let update = ChannelUpdate {
        actor: f.target.actor,
        company_id: f.target.company.as_uuid(),
        channel_id: desk,
        write: write(Some(root)),
    };
    let (activated, reassigned, ()) = tokio::join!(
        p.save_binding(activate),
        ChannelPersistence::update(p, update),
        async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                p.pool(),
                2,
            )
            .await;
            gate.commit().await.unwrap();
        }
    );
    activated.unwrap();
    assert_eq!(reassigned.unwrap().department_id, root);
    assert!(f.state().await.active);
    assert_eq!(
        ChannelPersistence::get_by_id(p, desk)
            .await
            .unwrap()
            .unwrap()
            .department_id,
        root
    );
}

#[tokio::test]
async fn workflow_role_binding_reviewer_vocabulary_round_trips_multiple_holders() {
    let f = fixture().await;
    let department = department(&f).await;
    let p = &f.fixture.persistence;
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.target.company.as_uuid())
            .bind(f.target.actor.user_id())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let other = p
        .create_user("second", "second@example.test", "hash")
        .await
        .unwrap();
    let principal = crate::adapters::persistence::test_support::insert_root_member(
        p.pool(),
        crate::domain::workflow::CompanyId::new(f.target.company.as_uuid()),
        other.id,
        crate::entities::company_member::CompanyAccessRole::Member,
        crate::entities::access_level::AccessLevel::PUBLIC,
    )
    .await
    .as_uuid();

    let use_cases = crate::adapters::persistence::test_support::department_use_cases(p);
    for holder in [owner, principal] {
        use_cases
            .assign_role(
                f.target.actor.user_id(),
                f.target.company.as_uuid(),
                department,
                crate::application::ports::department::DepartmentRoleAssignment {
                    role: DepartmentRole::Reviewer,
                    principal: PrincipalId::new(holder),
                    decider: false,
                },
            )
            .await
            .unwrap();
    }
    let roles = use_cases
        .visible_department(
            f.target.actor.user_id(),
            f.target.company.as_uuid(),
            department,
        )
        .await
        .unwrap();
    assert_eq!(
        roles
            .roles
            .iter()
            .filter(|holder| holder.role == DepartmentRole::Reviewer)
            .count(),
        2
    );
    assert!(sqlx::query("INSERT INTO department_roles (company_id,department_id,role,principal_id) VALUES ($1,$2,'boss',$3)")
        .bind(f.target.company.as_uuid()).bind(department.as_uuid()).bind(owner).execute(p.pool()).await.is_err());
}

#[path = "escalation_binding_tests.rs"]
mod escalation;

#[path = "direct_role_binding_tests.rs"]
mod direct;
