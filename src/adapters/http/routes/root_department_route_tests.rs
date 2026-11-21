use super::*;
use crate::{
    adapters::persistence::test_support::insert_root_member,
    domain::workflow::CompanyId,
    entities::{access_level::AccessLevel, company_member::*, user::User},
    use_cases::{company::CompanyPersistence, user::UserPersistence},
};

struct RootRefusals {
    roles: StatusCode,
    membership: StatusCode,
}

struct RootRoutes {
    f: Fixture,
    departments: Arc<DepartmentUseCases>,
    root: Uuid,
    principal: Uuid,
    admin: User,
}

impl RootRoutes {
    async fn new() -> Option<Self> {
        let f = Fixture::new().await?;
        let departments = use_cases(&f);
        let root = sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(f.company.id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
        let principal = sqlx::query_scalar("SELECT principal_id FROM membership_of($1,$2)")
            .bind(f.member.id)
            .bind(f.company.id)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
        let label = format!("root_admin_{}", Uuid::new_v4().simple());
        let admin = f
            .persistence
            .create_user(&label, &format!("{label}@example.test"), "hash")
            .await
            .unwrap();
        insert_root_member(
            f.persistence.pool(),
            CompanyId::new(f.company.id),
            admin.id,
            CompanyAccessRole::Admin,
            AccessLevel::PUBLIC,
        )
        .await;
        Some(Self {
            f,
            departments,
            root,
            principal,
            admin,
        })
    }

    async fn refuse(&self, user: &User, company: Uuid, expected: RootRefusals) {
        denied(
            assign_role_json(
                State(self.departments.clone()),
                Fixture::auth(user),
                Path((company, self.root, "admin".into(), self.principal)),
                Json(RoleAssignmentPayload { decider: false }),
            )
            .await,
            expected.roles,
            &self.f.foreign.name,
        )
        .await;
        denied(
            clear_role_json(
                State(self.departments.clone()),
                Fixture::auth(user),
                Path((company, self.root, "admin".into(), self.principal)),
                axum::extract::Query(RoleRemovalQuery { successor: None }),
            )
            .await,
            expected.roles,
            &self.f.foreign.name,
        )
        .await;
        denied(
            remove_member_json(
                State(self.departments.clone()),
                Fixture::auth(user),
                Path((company, self.root, self.principal)),
            )
            .await,
            expected.membership,
            &self.f.foreign.name,
        )
        .await;
    }

    async fn owner_role_round_trip(&self) {
        let f = &self.f;
        assign_role_json(
            State(self.departments.clone()),
            Fixture::auth(&f.owner),
            Path((f.company.id, self.root, "admin".into(), self.principal)),
            Json(RoleAssignmentPayload { decider: false }),
        )
        .await
        .unwrap();
        assert_eq!(
            f.persistence
                .company_access(f.member.id, f.company.id)
                .await
                .unwrap()
                .unwrap()
                .membership,
            CompanyMembership::Admin
        );
        clear_role_json(
            State(self.departments.clone()),
            Fixture::auth(&f.owner),
            Path((f.company.id, self.root, "admin".into(), self.principal)),
            axum::extract::Query(RoleRemovalQuery { successor: None }),
        )
        .await
        .unwrap();
        assert_eq!(
            f.persistence
                .company_access(f.member.id, f.company.id)
                .await
                .unwrap()
                .unwrap()
                .membership,
            CompanyMembership::Member
        );
    }

    async fn preserve_owner_and_scope(&self) {
        let f = &self.f;
        let owner_principal: Uuid =
            sqlx::query_scalar("SELECT principal_id FROM membership_of($1,$2)")
                .bind(f.owner.id)
                .bind(f.company.id)
                .fetch_one(f.persistence.pool())
                .await
                .unwrap();
        denied(
            clear_role_json(
                State(self.departments.clone()),
                Fixture::auth(&f.owner),
                Path((f.company.id, self.root, "owner".into(), owner_principal)),
                axum::extract::Query(RoleRemovalQuery { successor: None }),
            )
            .await,
            StatusCode::BAD_REQUEST,
            &f.foreign.name,
        )
        .await;
        denied(
            remove_member_json(
                State(self.departments.clone()),
                Fixture::auth(&f.owner),
                Path((f.company.id, self.root, owner_principal)),
            )
            .await,
            StatusCode::BAD_REQUEST,
            &f.foreign.name,
        )
        .await;
        let foreign_root: Uuid =
            sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
                .bind(f.foreign.id)
                .fetch_one(f.persistence.pool())
                .await
                .unwrap();
        denied(
            assign_role_json(
                State(self.departments.clone()),
                Fixture::auth(&f.owner),
                Path((f.company.id, foreign_root, "admin".into(), self.principal)),
                Json(RoleAssignmentPayload { decider: false }),
            )
            .await,
            StatusCode::NOT_FOUND,
            &f.foreign.name,
        )
        .await;
    }

    async fn owner_removes_member(&self) {
        let f = &self.f;
        remove_member_json(
            State(self.departments.clone()),
            Fixture::auth(&f.owner),
            Path((f.company.id, self.root, self.principal)),
        )
        .await
        .unwrap();
        assert!(
            f.persistence
                .company_access(f.member.id, f.company.id)
                .await
                .unwrap()
                .is_none()
        );
        let kind: String = sqlx::query_scalar("SELECT kind FROM principals WHERE id=$1")
            .bind(self.principal)
            .fetch_one(f.persistence.pool())
            .await
            .unwrap();
        assert_eq!(kind, "external");
    }
}

#[tokio::test]
async fn root_membership_department_routes_preserve_owner_only_team_mutations() {
    let Some(routes) = RootRoutes::new().await else {
        return;
    };
    let f = &routes.f;
    for (user, company, expected) in [
        (&routes.admin, f.company.id, StatusCode::FORBIDDEN),
        (&f.member, f.company.id, StatusCode::NOT_FOUND),
        (&f.stranger, f.company.id, StatusCode::NOT_FOUND),
        (&f.owner, f.foreign.id, StatusCode::NOT_FOUND),
    ] {
        routes
            .refuse(
                user,
                company,
                RootRefusals {
                    roles: StatusCode::NOT_FOUND,
                    membership: expected,
                },
            )
            .await;
    }
    routes.owner_role_round_trip().await;
    routes.preserve_owner_and_scope().await;
    routes.owner_removes_member().await;
}
