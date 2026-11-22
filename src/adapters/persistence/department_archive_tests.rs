//! `unit_archive_` (Plan08 02A, 02B.4–5, 02D): what the schema refuses on an archived unit, the
//! archive and restore round trip, the configuration writes an archive stops and the ones it
//! leaves, who may archive, and the competing writers the unit row locks settle.
use super::*;
use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
use crate::application::ports::department::{
    DepartmentRoleAssignment, DepartmentRoleRemoval, OpenWork,
};
use crate::use_cases::agent::SpamScanning;
use crate::use_cases::channel::ChannelUseCases;
use crate::use_cases::company::CompanyPersistence;

async fn archive(f: &Departments, unit: DepartmentId) -> AppResult<Department> {
    f.use_cases
        .archive_department(f.owner, f.company, unit, OpenWork::Refuse)
        .await
}

async fn restore(f: &Departments, unit: DepartmentId) -> AppResult<Department> {
    f.use_cases
        .restore_department(f.owner, f.company, unit)
        .await
}

async fn archived_at(f: &Departments, unit: DepartmentId) -> Option<chrono::DateTime<chrono::Utc>> {
    sqlx::query_scalar("SELECT archived_at FROM departments WHERE company_id = $1 AND id = $2")
        .bind(f.company)
        .bind(unit.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap()
}

/// The unit's archive and restore events, oldest first, with who acted and as what.
async fn lifecycle_events(f: &Departments, unit: DepartmentId) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT transition_kind, acting_role FROM department_administration_events \
          WHERE company_id = $1 AND department_id = $2 \
            AND transition_kind IN ('department_archived', 'department_restored') \
          ORDER BY created_at, id",
    )
    .bind(f.company)
    .bind(unit.as_uuid())
    .fetch_all(f.pool())
    .await
    .unwrap()
}

fn channels(f: &Departments) -> ChannelUseCases {
    ChannelUseCases::new(
        Arc::new(f.persistence.clone()),
        Arc::new(f.persistence.clone()),
        Arc::new(f.persistence.clone()),
        SpamScanning::Available,
    )
}

fn desk_write(slug: &str, department_id: Option<DepartmentId>) -> ChannelWrite {
    ChannelWrite {
        name: slug.into(),
        slug: slug.into(),
        enabled: true,
        department_id,
        ..ChannelWrite::default()
    }
}

fn role(role: DepartmentRole, principal: PrincipalId, decider: bool) -> DepartmentRoleAssignment {
    DepartmentRoleAssignment {
        role,
        principal,
        decider,
    }
}

async fn management_role(f: &Departments, user: Uuid, unit: DepartmentId) -> Option<String> {
    sqlx::query_scalar("SELECT unit_management_role($1, $2, $3)")
        .bind(user)
        .bind(f.company)
        .bind(unit.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap()
}

fn is_archived_refusal<T: std::fmt::Debug>(result: &AppResult<T>) -> bool {
    matches!(result, Err(AppError::UnitArchived))
}

#[tokio::test]
async fn unit_archive_root_is_refused() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let root = f.root_id().await;
    assert!(matches!(
        archive(&f, root).await,
        Err(AppError::NotFound(_) | AppError::BadRequest(_))
    ));
    // The owner manages the root itself, so the port is the one refusing; the CHECK backs it.
    let scope = DepartmentScope {
        actor: WorkflowActor::authenticated(f.owner).unwrap(),
        company: CompanyId::new(f.company),
    };
    assert!(matches!(
        f.departments.archive(scope, root, OpenWork::Refuse).await,
        Err(AppError::BadRequest(_))
    ));
    let raw = sqlx::query(
        "UPDATE departments SET archived_at = now(), \
         archived_by = '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}' \
         WHERE company_id = $1 AND is_root",
    )
    .bind(f.company)
    .execute(f.pool())
    .await;
    assert_eq!(refused_by(raw), "departments_root_archive");
    assert_eq!(archived_at(&f, root).await, None);
}

#[tokio::test]
async fn unit_archive_guard_refuses_active_units_under_an_archived_parent() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let east = f.create("East", None).await;
    let west = f.create("West", None).await;
    archive(&f, east.id).await.unwrap();
    // The application refuses each way of putting an active unit under it.
    assert!(is_archived_refusal(
        &f.use_cases
            .create_department(f.owner, f.company, f.write("Inside", Some(east.id)), None)
            .await
    ));
    assert!(is_archived_refusal(
        &f.reparent(west.id, Some(east.id)).await
    ));
    // The schema refuses the same rows from a writer that skips the application.
    assert_eq!(
        refused_by(
            f.insert_raw(f.company, "raw-inside", Some(east.id.as_uuid()))
                .await
        ),
        "departments_archived_parent"
    );
    let moved =
        sqlx::query("UPDATE departments SET parent_id = $3 WHERE company_id = $1 AND id = $2")
            .bind(f.company)
            .bind(west.id.as_uuid())
            .bind(east.id.as_uuid())
            .execute(f.pool())
            .await;
    assert_eq!(refused_by(moved), "departments_archived_parent");
    // Archiving a parent alone, leaving an active child, is refused at commit.
    let child = f.create("Child", Some(west.id)).await;
    let parent_only = sqlx::query(
        "UPDATE departments SET archived_at = now(), \
         archived_by = '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Test\"}' \
         WHERE company_id = $1 AND id = $2",
    )
    .bind(f.company)
    .bind(west.id.as_uuid())
    .execute(f.pool())
    .await;
    assert_eq!(refused_by(parent_only), "unit_archived_subtree");
    assert_eq!(archived_at(&f, west.id).await, None);
    assert_eq!(archived_at(&f, child.id).await, None);
    // A unit cannot be restored while its parent stays archived.
    let inner = f.create("Inner", Some(west.id)).await;
    archive(&f, west.id).await.unwrap();
    assert!(is_archived_refusal(&restore(&f, inner.id).await));
    let raw_restore = sqlx::query(
        "UPDATE departments SET archived_at = NULL, archived_by = NULL \
         WHERE company_id = $1 AND id = $2",
    )
    .bind(f.company)
    .bind(inner.id.as_uuid())
    .execute(f.pool())
    .await;
    assert_eq!(refused_by(raw_restore), "departments_archived_parent");
}

#[tokio::test]
async fn unit_archive_round_trip_keeps_earlier_timestamps_and_restores_one_unit() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let parent = f.create("Parent", None).await;
    let child = f.create("Child", Some(parent.id)).await;
    let desk = f.desk_of(parent.id).await;
    let archived_child = archive(&f, child.id).await.unwrap();
    let child_at = archived_child.archived.as_ref().unwrap().at;
    let by = &archived_child.archived.as_ref().unwrap().by;
    assert_eq!(
        (by.actor_type.clone(), by.actor_id, by.actor_name.as_str()),
        (
            crate::entities::creation::CreatorType::User,
            Some(f.owner),
            "Owner"
        )
    );
    assert!(is_archived_refusal(&archive(&f, child.id).await));
    let archived_parent = archive(&f, parent.id).await.unwrap();
    assert!(archived_parent.is_archived());
    assert_eq!(
        archived_at(&f, child.id).await,
        Some(child_at),
        "an earlier archive keeps its own time"
    );
    assert!(archived_parent.archived.unwrap().at > child_at);
    let enabled = || async {
        sqlx::query_scalar::<_, bool>("SELECT enabled FROM channels WHERE id = $1")
            .bind(desk)
            .fetch_one(f.pool())
            .await
            .unwrap()
    };
    assert!(enabled().await, "an archive leaves the desk's own switch");
    let desk_row = ChannelPersistence::get_by_id(&f.persistence, desk)
        .await
        .unwrap()
        .unwrap();
    assert!(desk_row.unit_archived && !desk_row.takes_traffic());

    let restored = restore(&f, parent.id).await.unwrap();
    assert!(!restored.is_archived());
    assert_eq!(archived_at(&f, child.id).await, Some(child_at));
    assert!(enabled().await);
    assert!(
        ChannelPersistence::get_by_id(&f.persistence, desk)
            .await
            .unwrap()
            .unwrap()
            .takes_traffic()
    );
    assert!(matches!(
        restore(&f, parent.id).await,
        Err(AppError::BadRequest(_))
    ));
    restore(&f, child.id).await.unwrap();
    assert_eq!(archived_at(&f, child.id).await, None);
    let owner = || "owner".to_owned();
    assert_eq!(
        lifecycle_events(&f, child.id).await,
        vec![
            ("department_archived".to_owned(), owner()),
            ("department_restored".to_owned(), owner()),
        ],
        "the parent's archive found the child archived and recorded nothing for it"
    );
    assert_eq!(
        lifecycle_events(&f, parent.id).await,
        vec![
            ("department_archived".to_owned(), owner()),
            ("department_restored".to_owned(), owner()),
        ]
    );
}

/// Plan08 02B.4 (D1.8): each configuration write on an archived unit refuses with
/// `unit.archived` and changes nothing.
#[tokio::test]
async fn unit_archive_refuses_configuration_of_an_archived_unit() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Dormant", None).await;
    let active = f.create("Active", None).await;
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.member_principal)
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            role(DepartmentRole::Reviewer, f.member_principal, false),
        )
        .await
        .unwrap();
    let desk = f.desk_of(unit.id).await;
    archive(&f, unit.id).await.unwrap();
    let roles_before = f.roles(unit.id).await;

    let renamed = f
        .use_cases
        .update_department(
            f.owner,
            f.company,
            unit.id,
            DepartmentEdit {
                name: Some("Renamed".into()),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(is_archived_refusal(&renamed), "{renamed:?}");
    assert!(is_archived_refusal(
        &f.reparent(unit.id, Some(active.id)).await
    ));
    assert!(is_archived_refusal(
        &f.use_cases
            .add_member(f.owner, f.company, unit.id, f.owner_principal)
            .await
    ));
    assert!(is_archived_refusal(
        &f.use_cases
            .remove_member(f.owner, f.company, unit.id, f.member_principal)
            .await
    ));
    let assigned = f
        .use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            role(DepartmentRole::Lead, f.owner_principal, false),
        )
        .await;
    assert!(is_archived_refusal(&assigned), "{assigned:?}");
    let sponsor = f
        .use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            role(DepartmentRole::Sponsor, f.owner_principal, true),
        )
        .await;
    assert!(is_archived_refusal(&sponsor), "{sponsor:?}");
    let cleared = f
        .use_cases
        .clear_role(
            f.owner,
            f.company,
            unit.id,
            DepartmentRoleRemoval {
                role: DepartmentRole::Reviewer,
                principal: f.member_principal,
                successor: None,
            },
        )
        .await;
    assert!(is_archived_refusal(&cleared), "{cleared:?}");
    assert_eq!(f.members(unit.id).await, vec![f.member_principal]);
    assert_eq!(f.roles(unit.id).await, roles_before);
    assert_eq!(
        f.departments
            .get(CompanyId::new(f.company), unit.id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Dormant"
    );

    // Desks: none may be created in or moved into the unit, and its own desk cannot be edited.
    let created = channels(&f)
        .create_channel(
            f.owner,
            f.company,
            desk_write("new-desk", Some(unit.id)),
            false,
        )
        .await;
    assert!(is_archived_refusal(&created), "{created:?}");
    let other = channels(&f)
        .create_channel(
            f.owner,
            f.company,
            desk_write("other-desk", Some(active.id)),
            false,
        )
        .await
        .unwrap();
    let moved_in = channels(&f)
        .update_channel(
            f.owner,
            f.company,
            other.id,
            desk_write("other-desk", Some(unit.id)),
            false,
        )
        .await;
    assert!(is_archived_refusal(&moved_in), "{moved_in:?}");
    let own = ChannelPersistence::get_by_id(&f.persistence, desk)
        .await
        .unwrap()
        .unwrap();
    let edited = channels(&f)
        .update_channel(
            f.owner,
            f.company,
            desk,
            ChannelWrite {
                name: "Edited".into(),
                ..desk_write(own.slug.as_str(), Some(unit.id))
            },
            false,
        )
        .await;
    assert!(is_archived_refusal(&edited), "{edited:?}");

    // A unit's spending caps are kept but cannot be changed.
    assert!(is_archived_refusal(
        &CompanyPersistence::set_unit_budget(
            &f.persistence,
            f.owner,
            f.company,
            unit.id.as_uuid(),
            "day",
            100
        )
        .await
    ));
    assert!(is_archived_refusal(
        &CompanyPersistence::clear_unit_budget(
            &f.persistence,
            f.owner,
            f.company,
            unit.id.as_uuid(),
            "day"
        )
        .await
    ));
}

/// U4: an archived unit's desks may be deleted or moved out to an active unit, and the unit
/// itself keeps its delete rules.
#[tokio::test]
async fn unit_archive_allows_desk_delete_desk_move_out_and_unit_delete() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Leaving", None).await;
    let active = f.create("Staying", None).await;
    let extra = channels(&f)
        .create_channel(
            f.owner,
            f.company,
            desk_write("extra-desk", Some(unit.id)),
            false,
        )
        .await
        .unwrap();
    archive(&f, unit.id).await.unwrap();
    let moved = channels(&f)
        .update_channel(
            f.owner,
            f.company,
            extra.id,
            desk_write("extra-desk", Some(active.id)),
            false,
        )
        .await
        .expect("a desk moves out to an active unit");
    assert_eq!(moved.department_id, active.id);
    assert!(moved.takes_traffic());
    f.drop_desk(unit.id).await;
    f.use_cases
        .delete_department(f.owner, f.company, unit.id)
        .await
        .expect("an archived unit without desks deletes");
    assert!(
        f.departments
            .get(CompanyId::new(f.company), unit.id)
            .await
            .unwrap()
            .is_none()
    );
}

/// Plan08 02B.5: the schema refuses new members, roles and desks on an archived unit from any
/// writer, and still lets them be removed, including by a company-level retirement.
#[tokio::test]
async fn unit_archive_refuses_trigger_writes_and_allows_deletes() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Sealed", None).await;
    let active = f.create("Open", None).await;
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.member_principal)
        .await
        .unwrap();
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.agent_principal)
        .await
        .unwrap();
    let other_desk = f.desk_of(active.id).await;
    archive(&f, unit.id).await.unwrap();

    let member = sqlx::query(
        "INSERT INTO department_members (company_id, department_id, principal_id) VALUES ($1, $2, $3)",
    )
    .bind(f.company)
    .bind(unit.id.as_uuid())
    .bind(f.owner_principal.as_uuid())
    .execute(f.pool())
    .await;
    assert_eq!(refused_by(member), "unit_archived");
    let lead = sqlx::query(
        "INSERT INTO department_roles (company_id, department_id, role, principal_id) \
         VALUES ($1, $2, 'lead', $3)",
    )
    .bind(f.company)
    .bind(unit.id.as_uuid())
    .bind(f.owner_principal.as_uuid())
    .execute(f.pool())
    .await;
    assert_eq!(refused_by(lead), "unit_archived");
    let desk =
        sqlx::query("UPDATE channels SET department_id = $3 WHERE company_id = $1 AND id = $2")
            .bind(f.company)
            .bind(other_desk)
            .bind(unit.id.as_uuid())
            .execute(f.pool())
            .await;
    assert_eq!(refused_by(desk), "unit_archived");

    sqlx::query(
        "DELETE FROM department_members WHERE company_id = $1 AND department_id = $2 AND principal_id = $3",
    )
    .bind(f.company)
    .bind(unit.id.as_uuid())
    .bind(f.agent_principal.as_uuid())
    .execute(f.pool())
    .await
    .expect("a member row deletes from an archived unit");
    crate::adapters::persistence::test_support::revoke_membership_now(
        f.pool(),
        f.company,
        f.member,
    )
    .await;
    assert!(
        f.members(unit.id).await.is_empty(),
        "retirement removes placements in archived units"
    );
}

/// Plan08 02D.1/02D.2: the parent's manager archives and restores; a unit's own lead does not
/// manage its archive, and no role held on an archived unit manages anything.
#[tokio::test]
async fn unit_archive_authority_parent_manager_archives_and_archived_roles_do_not_manage() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let parent = f.create("Parent", None).await;
    let child = f.create("Child", Some(parent.id)).await;
    let (parent_lead, parent_principal) = teammate(f.pool(), f.company, "member").await;
    let (child_lead, child_principal) = teammate(f.pool(), f.company, "member").await;
    for (unit, principal) in [(parent.id, parent_principal), (child.id, child_principal)] {
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                unit,
                role(DepartmentRole::Lead, principal, false),
            )
            .await
            .unwrap();
    }
    assert!(matches!(
        f.use_cases
            .archive_department(child_lead, f.company, child.id, OpenWork::Refuse)
            .await,
        Err(AppError::NotFound(_) | AppError::Forbidden(_))
    ));
    assert_eq!(archived_at(&f, child.id).await, None);
    f.use_cases
        .archive_department(parent_lead, f.company, child.id, OpenWork::Refuse)
        .await
        .expect("the parent's lead archives the child");
    assert_eq!(
        lifecycle_events(&f, child.id).await,
        vec![(
            "department_archived".to_owned(),
            format!("lead:{}", parent.id)
        )]
    );
    assert_eq!(management_role(&f, child_lead, child.id).await, None);
    assert_eq!(
        management_role(&f, parent_lead, child.id).await,
        Some(format!("lead:{}", parent.id))
    );
    let refused = f
        .use_cases
        .update_department(
            child_lead,
            f.company,
            child.id,
            DepartmentEdit {
                name: Some("Mine".into()),
                ..DepartmentEdit::default()
            },
        )
        .await;
    assert!(
        matches!(refused, Err(AppError::NotFound(_) | AppError::Forbidden(_))),
        "{refused:?}"
    );
    assert!(matches!(
        f.use_cases
            .restore_department(child_lead, f.company, child.id)
            .await,
        Err(AppError::NotFound(_) | AppError::Forbidden(_))
    ));
    f.use_cases
        .restore_department(parent_lead, f.company, child.id)
        .await
        .unwrap();
    assert_eq!(
        management_role(&f, child_lead, child.id).await,
        Some(format!("lead:{}", child.id))
    );
    // Once the parent is archived its lead manages neither it nor the child.
    archive(&f, parent.id).await.unwrap();
    assert_eq!(management_role(&f, parent_lead, child.id).await, None);
    assert_eq!(management_role(&f, parent_lead, parent.id).await, None);
    assert_eq!(
        sqlx::query_scalar::<_, Option<String>>("SELECT any_unit_management_role($1, $2)")
            .bind(parent_lead)
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        management_role(&f, f.owner, child.id).await,
        Some("owner".into())
    );
}

/// Plan08 02D.3 (proposal 2): a person deciding sponsor manages the unit and its descendants; an
/// agent decider, an informed sponsor and a decider of an archived unit do not.
#[tokio::test]
async fn unit_archive_authority_sponsor_decider_manages_unit_and_descendants() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Sponsored", None).await;
    let below = f.create("Below", Some(unit.id)).await;
    let (informed, informed_principal) = teammate(f.pool(), f.company, "member").await;
    for (principal, decider) in [(f.member_principal, true), (informed_principal, false)] {
        f.use_cases
            .assign_role(
                f.owner,
                f.company,
                unit.id,
                role(DepartmentRole::Sponsor, principal, decider),
            )
            .await
            .unwrap();
    }
    let expected = Some(format!("sponsor:{}", unit.id));
    assert_eq!(management_role(&f, f.member, unit.id).await, expected);
    assert_eq!(management_role(&f, f.member, below.id).await, expected);
    assert_eq!(management_role(&f, informed, unit.id).await, None);
    assert_eq!(
        sqlx::query_scalar::<_, Option<String>>("SELECT any_unit_management_role($1, $2)")
            .bind(f.member)
            .bind(f.company)
            .fetch_one(f.pool())
            .await
            .unwrap(),
        expected
    );
    // The decider archives the child they manage; the audit records the sponsor role.
    f.use_cases
        .archive_department(f.member, f.company, below.id, OpenWork::Refuse)
        .await
        .expect("the deciding sponsor archives a unit below");
    assert_eq!(
        lifecycle_events(&f, below.id).await,
        vec![("department_archived".to_owned(), expected.clone().unwrap())]
    );
    f.use_cases
        .restore_department(f.member, f.company, below.id)
        .await
        .unwrap();
    // An agent decider manages nothing.
    let agent_unit = f.create("Agent sponsored", None).await;
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            agent_unit.id,
            role(DepartmentRole::Sponsor, f.agent_principal, true),
        )
        .await
        .unwrap();
    let agent_user: Option<Uuid> =
        sqlx::query_scalar("SELECT user_id FROM principals WHERE id = $1")
            .bind(f.agent_principal.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap();
    assert_eq!(
        agent_user, None,
        "an agent principal has no user to manage as"
    );
    // A decider of an archived unit manages neither it nor what lies below.
    archive(&f, unit.id).await.unwrap();
    assert_eq!(management_role(&f, f.member, unit.id).await, None);
    assert_eq!(management_role(&f, f.member, below.id).await, None);
}

/// Plan08 02D.4: an agent placed only in archived units is managed from the root, not by the
/// lead of its archived unit.
#[tokio::test]
async fn unit_archive_authority_agent_in_only_archived_units_is_managed_by_root() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Agents", None).await;
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.agent_principal)
        .await
        .unwrap();
    f.use_cases
        .assign_role(
            f.owner,
            f.company,
            unit.id,
            role(DepartmentRole::Lead, f.member_principal, false),
        )
        .await
        .unwrap();
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id = $1")
        .bind(f.agent_principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    let agent_role = |user: Uuid| {
        sqlx::query_scalar::<_, Option<String>>("SELECT agent_management_role($1, $2, $3)")
            .bind(user)
            .bind(f.company)
            .bind(agent)
            .fetch_one(f.pool())
    };
    assert_eq!(
        agent_role(f.member).await.unwrap(),
        Some(format!("lead:{}", unit.id))
    );
    archive(&f, unit.id).await.unwrap();
    assert_eq!(agent_role(f.member).await.unwrap(), None);
    assert_eq!(agent_role(f.owner).await.unwrap(), Some("owner".into()));
    assert_eq!(agent_role(f.admin).await.unwrap(), Some("admin".into()));
}

/// An archive of a parent and of its child at once: both units end archived, each with exactly
/// one archive event, whichever commits first.
#[tokio::test]
async fn unit_archive_competing_overlapping_archives_agree() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let parent = f.create("Parent", None).await;
    let child = f.create("Child", Some(parent.id)).await;
    let blocker = super::competing::hold_company(&f).await;
    let tasks: Vec<_> = [parent.id, child.id]
        .into_iter()
        .map(|unit| {
            let f = f.clone();
            tokio::spawn(async move { archive(&f, unit).await })
        })
        .collect();
    wait_until_backends_are_blocked(f.pool(), 2).await;
    blocker.commit().await.unwrap();
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap());
    }
    // The parent always succeeds; the child either archived first or found itself archived.
    assert!(outcomes[0].is_ok(), "{outcomes:?}");
    assert!(
        outcomes[1].is_ok() || is_archived_refusal(&outcomes[1]),
        "{outcomes:?}"
    );
    for unit in [parent.id, child.id] {
        assert!(archived_at(&f, unit).await.is_some());
        assert_eq!(lifecycle_events(&f, unit).await.len(), 1, "{outcomes:?}");
    }
}

/// A raw member write racing an archive in progress waits for the unit row and is refused once
/// the archive commits (D1.9a): the archive's `FOR UPDATE` conflicts with the guard's
/// `FOR KEY SHARE`.
#[tokio::test]
async fn unit_archive_competing_trigger_write_waits_for_the_archive() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = Arc::new(Departments::on(database.pool.clone()).await);
    let unit = f.create("Racing", None).await;
    let scope = DepartmentScope {
        actor: WorkflowActor::authenticated(f.owner).unwrap(),
        company: CompanyId::new(f.company),
    };
    let mut archiving = f.pool().begin().await.unwrap();
    super::archive::archive_department_on(
        &mut archiving,
        scope,
        ManagementRole::Owner,
        unit.id,
        OpenWork::Refuse,
    )
    .await
    .unwrap();
    let writer = tokio::spawn({
        let f = f.clone();
        async move {
            sqlx::query(
                "INSERT INTO department_members (company_id, department_id, principal_id) \
                 VALUES ($1, $2, $3)",
            )
            .bind(f.company)
            .bind(unit.id.as_uuid())
            .bind(f.member_principal.as_uuid())
            .execute(f.pool())
            .await
        }
    });
    wait_until_backends_are_blocked(f.pool(), 1).await;
    archiving.commit().await.unwrap();
    assert_eq!(refused_by(writer.await.unwrap()), "unit_archived");
    assert!(f.members(unit.id).await.is_empty());
}

/// Plan08 02B.3: the internal-relay SQL refuses an archived unit's desk as a target and accepts
/// it again after the restore.
#[tokio::test]
async fn unit_archive_refuses_relay_to_an_archived_units_desk() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Relay", None).await;
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.agent_principal)
        .await
        .unwrap();
    let desk = f.desk_of(unit.id).await;
    let participates = || async {
        sqlx::query_scalar::<_, bool>("SELECT relay_desk_can_participate($1, $2, $3)")
            .bind(f.company)
            .bind(desk)
            .bind(f.agent_principal.as_uuid())
            .fetch_one(f.pool())
            .await
            .unwrap()
    };
    assert!(
        participates().await,
        "a member agent may relay to its unit's desk"
    );
    archive(&f, unit.id).await.unwrap();
    assert!(!participates().await);
    restore(&f, unit.id).await.unwrap();
    assert!(participates().await);
}

/// Plan08 02B.4: an archived unit's desk responder cannot be reconfigured.
#[tokio::test]
async fn unit_archive_refuses_responder_configuration() {
    use crate::application::workflow::channel_reply::{ChannelReplies, ConfigureChannelReply};
    use crate::entities::address::AddressTarget;
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Responding", None).await;
    let desk = f.desk_of(unit.id).await;
    let owner = WorkflowActor::authenticated(f.owner).unwrap();
    let company = CompanyId::new(f.company);
    let settings = f
        .persistence
        .channel_reply_settings(company, AddressTarget::Desk(desk), owner)
        .await
        .unwrap();
    let request = || ConfigureChannelReply {
        company,
        target: AddressTarget::Desk(desk),
        actor: owner,
        expected: settings.as_ref().map(|settings| settings.revision),
        responder: None,
        approval_policy: None,
    };
    archive(&f, unit.id).await.unwrap();
    let refused = f.persistence.configure_channel_reply(request()).await;
    assert!(is_archived_refusal(&refused), "{refused:?}");
    restore(&f, unit.id).await.unwrap();
    assert!(
        !is_archived_refusal(&f.persistence.configure_channel_reply(request()).await),
        "the restored desk's responder is configurable again"
    );
}

/// Plan08 02A.5: every read that lists units takes the argument, and leaves archived units out
/// unless asked.
#[tokio::test]
async fn unit_archive_listing_leaves_archived_units_out_unless_asked() {
    use crate::application::ports::administration::AdministrationPersistence;
    use crate::entities::department::ArchivedUnits::{Exclude, Include};
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let active = f.create("Active", None).await;
    let dormant = f.create("Dormant", None).await;
    for unit in [active.id, dormant.id] {
        f.use_cases
            .add_member(f.owner, f.company, unit, f.agent_principal)
            .await
            .unwrap();
        f.use_cases
            .add_member(f.owner, f.company, unit, f.member_principal)
            .await
            .unwrap();
    }
    archive(&f, dormant.id).await.unwrap();
    let company = CompanyId::new(f.company);
    let ids = |units: &[Department]| units.iter().map(|unit| unit.id).collect::<Vec<_>>();

    let listed = f
        .departments
        .list_by_company(company, Exclude)
        .await
        .unwrap();
    assert!(ids(&listed).contains(&active.id) && !ids(&listed).contains(&dormant.id));
    let listed = f
        .departments
        .list_by_company(company, Include)
        .await
        .unwrap();
    assert!(ids(&listed).contains(&dormant.id));
    let placed = f
        .departments
        .list_for_user(company, f.member, Exclude)
        .await
        .unwrap();
    assert!(ids(&placed).contains(&active.id) && !ids(&placed).contains(&dormant.id));
    let placed = f
        .departments
        .list_for_user(company, f.member, Include)
        .await
        .unwrap();
    assert!(ids(&placed).contains(&dormant.id));
    for (archived, expected) in [(Exclude, false), (Include, true)] {
        let visible = f
            .use_cases
            .visible_departments(f.member, f.company, archived)
            .await
            .unwrap();
        assert_eq!(ids(&visible).contains(&dormant.id), expected);
        let managed = f
            .use_cases
            .list_departments(f.owner, f.company, archived)
            .await
            .unwrap();
        assert_eq!(ids(&managed).contains(&dormant.id), expected);
        let choices = f
            .persistence
            .managed_unit_choices(f.owner, company, archived)
            .await
            .unwrap();
        assert_eq!(
            choices.iter().any(|choice| choice.id == dormant.id),
            expected
        );
    }
    // The desk lookup keeps an archived owner for labels and leaves it out of a filter's options.
    let desks = f
        .use_cases
        .desk_departments(f.owner, f.company, &[], Include)
        .await
        .unwrap();
    assert!(
        desks
            .department(dormant.id)
            .is_some_and(Department::is_archived)
    );
    assert_eq!(
        desks.department(dormant.id).unwrap().label(),
        "Dormant (archived)"
    );
    assert!(desks.choices(None).iter().all(|unit| unit.id != dormant.id));
    assert!(
        desks
            .choices(Some(dormant.id))
            .iter()
            .any(|unit| unit.id == dormant.id)
    );
    // An agent's departments name its active units only.
    let agent: Uuid = sqlx::query_scalar("SELECT agent_id FROM principals WHERE id = $1")
        .bind(f.agent_principal.as_uuid())
        .fetch_one(f.pool())
        .await
        .unwrap();
    for (archived, expected) in [(Exclude, 1), (Include, 2)] {
        let held = f
            .use_cases
            .agent_departments(f.owner, f.company, &[agent], archived)
            .await
            .unwrap();
        assert_eq!(held[&agent].0.len(), expected);
    }
}

/// Plan08 02C.4: desk history stays readable under the desk grants that existed, so a member of
/// an archived unit still reads its desk.
#[tokio::test]
async fn unit_archive_keeps_desk_history_readable_by_the_units_member() {
    let Some(f) = Box::pin(Departments::new()).await else {
        return;
    };
    let unit = f.create("Kept", None).await;
    f.use_cases
        .add_member(f.owner, f.company, unit.id, f.member_principal)
        .await
        .unwrap();
    let desk = f.desk_of(unit.id).await;
    let viewer = crate::entities::user::Viewer {
        user_id: f.member,
        email: sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id=$1")
            .bind(f.member)
            .fetch_one(f.pool())
            .await
            .unwrap()
            .into(),
        email_verified_at: None,
    };
    let uc = channels(&f);
    let readable = || uc.get_readable_channel(&viewer, f.company, desk);
    assert!(readable().await.unwrap().is_some());
    archive(&f, unit.id).await.unwrap();
    assert!(
        readable().await.unwrap().is_some(),
        "the member still reads the archived unit's desk"
    );
}
