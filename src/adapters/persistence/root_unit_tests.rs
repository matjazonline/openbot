//! Root membership through real creation, invitation and department services.
use super::OwnedCompany;
use crate::{
    adapters::persistence::test_support::{department_use_cases, own_database},
    app_error::AppError,
    application::ports::department::MAX_DEPARTMENT_ROSTER_ENTRIES,
    domain::workflow::CompanyId,
    entities::{
        access_level::AccessLevel,
        company_member::{CompanyAccessRole, CompanyMembership},
        department::{Department, DepartmentId, DepartmentRole},
        transport::PrincipalId,
    },
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        company_invite::{CompanyInvitePersistence, MemberLevelChange},
    },
};
use serde_json::json;
use uuid::Uuid;

#[path = "root_competing_tests.rs"]
mod competing;

#[derive(sqlx::FromRow)]
struct Membership {
    id: Uuid,
    principal_id: Uuid,
    department_id: Uuid,
    role: String,
    access_level: i16,
}

async fn membership(f: &OwnedCompany, user: Uuid) -> Membership {
    sqlx::query_as(
        "SELECT id, principal_id, department_id, role, access_level::smallint AS access_level \
         FROM membership_of($1, $2)",
    )
    .bind(user)
    .bind(f.company_id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap()
}

async fn root(f: &OwnedCompany) -> Department {
    let id: Uuid = sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
        .bind(f.company_id)
        .fetch_one(f.persistence.pool())
        .await
        .unwrap();
    department_use_cases(&f.persistence)
        .visible_department(f.owner.id, f.company_id, DepartmentId::new(id))
        .await
        .unwrap()
        .department
}

fn constraint(error: sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|error| error.constraint())
        .expect("a named relational refusal, not a syntax/connection failure")
        .to_owned()
}

#[tokio::test]
async fn root_department_creation_and_company_rename_round_trip() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let initial = root(&f).await;
    let company = f
        .persistence
        .get_by_id(f.company_id)
        .await
        .unwrap()
        .unwrap();
    assert!(initial.is_root);
    assert_eq!(initial.parent_id, None);
    assert_eq!(initial.slug.as_str(), company.slug.as_str());
    let owner = membership(&f, f.owner.id).await;
    assert_eq!(owner.department_id, initial.id.as_uuid());
    assert_eq!((owner.role.as_str(), owner.access_level), ("owner", 9));
    let principal_user: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM principals WHERE company_id=$1 AND id=$2 AND kind='person'",
    )
    .bind(f.company_id)
    .bind(owner.principal_id)
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(principal_user, f.owner.id);
    let slug = format!("renamed-{}", Uuid::new_v4().simple());
    f.persistence
        .update_for_user(
            f.owner.id,
            f.company_id,
            CompanyWrite {
                name: "Renamed company".into(),
                slug: slug.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let renamed = root(&f).await;
    assert_eq!(renamed.id, initial.id);
    assert_eq!(renamed.slug.as_str(), slug);
    assert_eq!(renamed.name, "Renamed company");
    let roots: i64 =
        sqlx::query_scalar("SELECT count(*) FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.company_id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
    assert_eq!(roots, 1);
}

#[tokio::test]
async fn root_department_database_guards_preserve_shape_level_and_incarnation() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let initial = root(&f).await;
    let owner = membership(&f, f.owner.id).await;
    let pool = f.persistence.pool();
    let refused = sqlx::query("INSERT INTO departments(id,company_id,name,slug,is_root,created_by) VALUES(gen_random_uuid(),$1,'Second','second',true,$2)")
        .bind(f.company_id).bind(json!({"actor_type":"system","actor_id":null,"actor_name":"Test"})).execute(pool).await.unwrap_err();
    assert_eq!(constraint(refused), "departments_one_root");
    let refused =
        sqlx::query("UPDATE departments SET parent_id=id WHERE company_id=$1 AND is_root")
            .bind(f.company_id)
            .execute(pool)
            .await
            .unwrap_err();
    assert_eq!(constraint(refused), "departments_root_shape");
    let refused = sqlx::query("DELETE FROM departments WHERE company_id=$1 AND is_root")
        .bind(f.company_id)
        .execute(pool)
        .await
        .unwrap_err();
    assert_eq!(constraint(refused), "department_roles_owner_mirror");
    let refused = sqlx::query(
        "UPDATE department_members SET access_level=8 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(f.company_id)
    .bind(f.owner.id)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(constraint(refused), "department_members_owner_level");
    let refused = sqlx::query("UPDATE department_members SET id=gen_random_uuid() WHERE company_id=$1 AND root_user_id=$2")
        .bind(f.company_id).bind(f.owner.id).execute(pool).await.unwrap_err();
    assert_eq!(constraint(refused), "department_members_incarnation");
    let stranger = OwnedCompany::account(&f.persistence, "stranger").await;
    let refused = sqlx::query(
        "UPDATE department_members SET root_user_id=$3 WHERE company_id=$1 AND root_user_id=$2",
    )
    .bind(f.company_id)
    .bind(f.owner.id)
    .bind(stranger.id)
    .execute(pool)
    .await
    .unwrap_err();
    assert_eq!(constraint(refused), "department_members_root_user");
    let child: Uuid = sqlx::query_scalar("INSERT INTO departments(id,company_id,name,slug,parent_id,created_by) VALUES(gen_random_uuid(),$1,'Child','child',$2,$3) RETURNING id")
        .bind(f.company_id).bind(initial.id.as_uuid()).bind(json!({"actor_type":"system","actor_id":null,"actor_name":"Test"})).fetch_one(pool).await.unwrap();
    let refused = sqlx::query("INSERT INTO department_roles(company_id,department_id,principal_id,role) VALUES($1,$2,$3,'owner')")
        .bind(f.company_id).bind(child).bind(owner.principal_id).execute(pool).await.unwrap_err();
    assert_eq!(constraint(refused), "department_roles_root_only");
    let refused = sqlx::query("INSERT INTO department_members(company_id,department_id,principal_id,access_level) VALUES($1,$2,$3,0)")
        .bind(f.company_id).bind(child).bind(owner.principal_id).execute(pool).await.unwrap_err();
    assert_eq!(constraint(refused), "department_members_level_on_root");
    assert_eq!(membership(&f, f.owner.id).await.id, owner.id);
    assert!(
        f.persistence
            .company_access(stranger.id, f.company_id)
            .await
            .unwrap()
            .is_none()
    );
}

async fn assert_derived_roles(
    f: &OwnedCompany,
    member: &crate::entities::user::User,
    admin: &crate::entities::user::User,
    stranger: &crate::entities::user::User,
) {
    for (user, expected) in [
        (f.owner.id, CompanyMembership::Owner),
        (admin.id, CompanyMembership::Admin),
        (member.id, CompanyMembership::Member),
    ] {
        assert_eq!(
            f.persistence
                .company_access(user, f.company_id)
                .await
                .unwrap()
                .unwrap()
                .membership,
            expected
        );
    }
    assert!(
        f.persistence
            .company_access(stranger.id, f.company_id)
            .await
            .unwrap()
            .is_none()
    );
}

async fn assert_admin_changes(
    f: &OwnedCompany,
    root: &Department,
    member: &crate::entities::user::User,
    admin: &crate::entities::user::User,
    member_principal: PrincipalId,
) {
    let departments = department_use_cases(&f.persistence);
    assert!(matches!(
        departments
            .assign_role(
                admin.id,
                f.company_id,
                root.id,
                DepartmentRole::Admin,
                member_principal
            )
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        departments
            .assign_role(
                member.id,
                f.company_id,
                root.id,
                DepartmentRole::Admin,
                member_principal
            )
            .await,
        Err(AppError::Forbidden(_) | AppError::NotFound(_))
    ));
    departments
        .assign_role(
            f.owner.id,
            f.company_id,
            root.id,
            DepartmentRole::Admin,
            member_principal,
        )
        .await
        .unwrap();
    assert_eq!(membership(f, member.id).await.role, "admin");
    assert!(matches!(
        departments
            .clear_role(
                admin.id,
                f.company_id,
                root.id,
                DepartmentRole::Admin,
                member_principal
            )
            .await,
        Err(AppError::Forbidden(_))
    ));
    departments
        .clear_role(
            f.owner.id,
            f.company_id,
            root.id,
            DepartmentRole::Admin,
            member_principal,
        )
        .await
        .unwrap();
    assert_eq!(membership(f, member.id).await.role, "member");
}

#[tokio::test]
async fn root_membership_roles_and_equivalent_department_mutations_keep_owner_authority() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let member = f.member("member", CompanyAccessRole::Member).await;
    let admin = f.member("admin", CompanyAccessRole::Admin).await;
    let stranger = OwnedCompany::account(&f.persistence, "stranger").await;
    assert_derived_roles(&f, &member, &admin, &stranger).await;
    let root = root(&f).await;
    let member_principal = PrincipalId::new(membership(&f, member.id).await.principal_id);
    let departments = department_use_cases(&f.persistence);
    assert_admin_changes(&f, &root, &member, &admin, member_principal).await;
    assert!(matches!(
        departments
            .remove_member(admin.id, f.company_id, root.id, member_principal)
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        departments
            .remove_member(member.id, f.company_id, root.id, member_principal)
            .await,
        Err(AppError::Forbidden(_) | AppError::NotFound(_))
    ));
    departments
        .assign_role(
            f.owner.id,
            f.company_id,
            root.id,
            DepartmentRole::Lead,
            member_principal,
        )
        .await
        .unwrap();
    assert!(matches!(
        departments
            .clear_role(
                f.owner.id,
                f.company_id,
                root.id,
                DepartmentRole::Owner,
                PrincipalId::new(membership(&f, f.owner.id).await.principal_id)
            )
            .await,
        Err(AppError::BadRequest(_))
    ));
    departments
        .remove_member(f.owner.id, f.company_id, root.id, member_principal)
        .await
        .unwrap();
    assert!(
        f.persistence
            .company_access(member.id, f.company_id)
            .await
            .unwrap()
            .is_none()
    );
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM department_administration_events WHERE company_id=$1 AND principal_id=$2 AND role='lead' AND transition_kind='demoted'")
        .bind(f.company_id).bind(member_principal.as_uuid()).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(
        audit, 1,
        "delegated root removal snapshots held roles before clearing them"
    );
}

#[tokio::test]
async fn root_membership_reacceptance_preserves_level_and_rejoin_changes_only_membership_identity()
{
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let user = f.member("member", CompanyAccessRole::Member).await;
    let original = membership(&f, user.id).await;
    f.persistence
        .set_member_access_level(MemberLevelChange {
            actor_id: f.owner.id,
            company_id: f.company_id,
            member_user_id: user.id,
            access_level: AccessLevel::L3,
        })
        .await
        .unwrap()
        .unwrap();
    let cases = f.use_cases();
    let invite = cases
        .create_company_invite(crate::use_cases::company_invite::CreateCompanyInvite {
            actor_id: f.owner.id,
            company_id: f.company_id,
            email: user.email.clone().into(),
            role: CompanyAccessRole::Admin,
            access_level: AccessLevel::L8,
        })
        .await
        .unwrap();
    cases.accept_invite(&user, invite.id).await.unwrap();
    let accepted = membership(&f, user.id).await;
    assert_eq!(accepted.id, original.id);
    assert_eq!(accepted.access_level, 3);
    assert_eq!(accepted.role, "admin");
    cases
        .remove_company_team_member(f.owner.id, f.company_id, user.id)
        .await
        .unwrap();
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM department_administration_events WHERE company_id=$1 AND principal_id=$2 AND role='admin' AND transition_kind='demoted'")
        .bind(f.company_id).bind(original.principal_id).fetch_one(f.persistence.pool()).await.unwrap();
    assert_eq!(audit, 1);
    let next = cases
        .create_company_invite(crate::use_cases::company_invite::CreateCompanyInvite {
            actor_id: f.owner.id,
            company_id: f.company_id,
            email: user.email.clone().into(),
            role: CompanyAccessRole::Member,
            access_level: AccessLevel::L4,
        })
        .await
        .unwrap();
    cases.accept_invite(&user, next.id).await.unwrap();
    let rejoined = membership(&f, user.id).await;
    assert_ne!(rejoined.id, original.id);
    assert_eq!(rejoined.principal_id, original.principal_id);
    assert_eq!(
        (rejoined.role.as_str(), rejoined.access_level),
        ("member", 4)
    );
    let immutable = sqlx::query("UPDATE department_administration_events SET role='lead' WHERE company_id=$1 AND principal_id=$2")
        .bind(f.company_id).bind(original.principal_id).execute(f.persistence.pool()).await.unwrap_err();
    assert_eq!(
        constraint(immutable),
        "department_retirement_audit_immutable"
    );
}

#[tokio::test]
async fn root_membership_roster_is_bounded_and_does_not_bound_authorization() {
    let Some(database) = own_database().await else {
        return;
    };
    let f = OwnedCompany::new(database.pool.clone()).await;
    let root = root(&f).await;
    let users: Vec<Uuid> = (0..MAX_DEPARTMENT_ROSTER_ENTRIES + 1)
        .map(|_| Uuid::new_v4())
        .collect();
    let mut tx = f.persistence.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,username,email,password_hash,email_verified_at) SELECT id,'roster_'||id::text,'roster_'||id::text||'@example.test','hash',clock_timestamp() FROM unnest($1::uuid[]) AS account(id)")
        .bind(&users).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO principals(id,company_id,kind,user_id,display_label) SELECT gen_random_uuid(),$1,'person',id,'Roster member' FROM unnest($2::uuid[]) AS account(id)")
        .bind(f.company_id).bind(&users).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO department_members(company_id,department_id,principal_id,access_level) SELECT $1,$2,id,0 FROM principals WHERE company_id=$1 AND user_id=ANY($3)")
        .bind(f.company_id).bind(root.id.as_uuid()).bind(&users).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO department_roles(company_id,department_id,principal_id,role) SELECT $1,$2,id,'reviewer' FROM principals WHERE company_id=$1 AND user_id=ANY($3)")
        .bind(f.company_id).bind(root.id.as_uuid()).bind(&users).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let detail = department_use_cases(&f.persistence)
        .visible_department(f.owner.id, f.company_id, root.id)
        .await
        .unwrap();
    assert!(detail.members_truncated && detail.roles_truncated);
    assert_eq!(detail.members.len(), MAX_DEPARTMENT_ROSTER_ENTRIES);
    assert_eq!(detail.roles.len(), MAX_DEPARTMENT_ROSTER_ENTRIES);
    assert_eq!(detail.roles[0].role, DepartmentRole::Owner);
    let visible: Vec<Uuid> = detail
        .members
        .iter()
        .map(|member| member.principal_id.as_uuid())
        .collect();
    let absent: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM principals WHERE company_id=$1 AND user_id=ANY($2) AND NOT(id=ANY($3)) ORDER BY id LIMIT 1",
    ).bind(f.company_id).bind(&users).bind(visible).fetch_one(f.persistence.pool()).await.unwrap();
    let access = f
        .persistence
        .company_access(absent, f.company_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(access.membership, CompanyMembership::Member);
    assert_eq!(access.access_level, AccessLevel::PUBLIC);
    let actual: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM department_members WHERE company_id=$1 AND department_id=$2",
    )
    .bind(CompanyId::new(f.company_id).as_uuid())
    .bind(root.id.as_uuid())
    .fetch_one(f.persistence.pool())
    .await
    .unwrap();
    assert_eq!(actual, (MAX_DEPARTMENT_ROSTER_ENTRIES + 2) as i64);
}
