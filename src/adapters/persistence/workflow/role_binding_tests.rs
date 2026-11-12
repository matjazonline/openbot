use super::binding_tests::BindingFixture;
use super::*;
use crate::application::ports::channel::{
    ChannelOwnershipWrite, ChannelPersistence, ChannelUpdate, ChannelWrite,
};
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
        ownership: Some(ChannelOwnershipWrite {
            department_id: Some(department),
            members_only: Some(false),
        }),
        ..Default::default()
    }
}

async fn desk(f: &BindingFixture, department: Option<DepartmentId>) -> Uuid {
    ChannelPersistence::create(
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
async fn workflow_role_binding_needs_a_department_only_while_active() {
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
}

#[tokio::test]
async fn workflow_role_binding_blocks_both_detach_writers_until_deactivation() {
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
        company_id: f.target.company.as_uuid(),
        channel_id: desk,
        write: write(None),
    };
    let error = ChannelPersistence::update(p, update.clone())
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    assert!(format!("{error:?}").contains("start"));
    let mut tx = p.pool().begin().await.unwrap();
    let error = crate::adapters::persistence::department_desks::set_desk_ownership_on(
        &mut tx,
        f.target.company,
        desk,
        &ChannelOwnershipWrite {
            department_id: Some(None),
            members_only: Some(false),
        },
    )
    .await
    .unwrap_err();
    assert!(format!("{error:?}").contains("binding.role_needs_department"));
    tx.rollback().await.unwrap();
    let retained = ChannelPersistence::get_by_id(p, desk)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained.department_id, Some(department));
    f.service()
        .deactivate(SetBindingActivity {
            target: f.target,
            expected: active.revision,
        })
        .await
        .unwrap();
    ChannelPersistence::update(p, update).await.unwrap();
    let detached = ChannelPersistence::get_by_id(p, desk)
        .await
        .unwrap()
        .unwrap();
    assert!(detached.department_id.is_none());
    assert!(!f.state().await.active);
}

#[tokio::test]
async fn workflow_role_binding_competing_activation_and_detachment_preserve_eligibility() {
    let f = fixture().await;
    let department = department(&f).await;
    let desk = desk(&f, Some(department)).await;
    let configured = configure(&f, desk).await;
    let activate = f.activity(configured.revision).await;
    let p = &f.fixture.persistence;
    let mut gate = p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(f.target.company.as_uuid())
        .execute(&mut *gate)
        .await
        .unwrap();
    let update = ChannelUpdate {
        company_id: f.target.company.as_uuid(),
        channel_id: desk,
        write: write(None),
    };
    let (activated, detached, ()) = tokio::join!(
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
    assert_ne!(
        activated.is_ok(),
        detached.is_ok(),
        "one succeeds, the conflicting write refuses"
    );
    let active = f.state().await.active;
    let owned = ChannelPersistence::get_by_id(p, desk)
        .await
        .unwrap()
        .unwrap()
        .department_id
        .is_some();
    assert!(
        !active || owned,
        "no active role binding on an unowned desk"
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
    sqlx::query(
        "INSERT INTO company_members (id,company_id,user_id,role) VALUES ($1,$2,$3,'member')",
    )
    .bind(Uuid::new_v4())
    .bind(f.target.company.as_uuid())
    .bind(other.id)
    .execute(p.pool())
    .await
    .unwrap();
    let principal = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (id,company_id,kind,user_id,display_label) VALUES ($1,$2,'person',$3,'Second')")
        .bind(principal).bind(f.target.company.as_uuid()).bind(other.id).execute(p.pool()).await.unwrap();
    let use_cases = crate::adapters::persistence::test_support::department_use_cases(p);
    for holder in [owner, principal] {
        use_cases
            .assign_role(
                f.target.actor.user_id(),
                f.target.company.as_uuid(),
                department,
                DepartmentRole::Reviewer,
                PrincipalId::new(holder),
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
