use super::*;
use crate::{
    entities::user::User,
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};
use std::sync::Arc;

pub(super) struct Fixture {
    pub pool: sqlx::PgPool,
    pub persistence: Arc<PostgresPersistence>,
    pub company_id: Uuid,
    pub owner: User,
    pub admin: User,
    pub member: User,
}
impl Fixture {
    pub async fn new(pool: sqlx::PgPool) -> Self {
        let persistence = Arc::new(PostgresPersistence::new(pool.clone()));
        let owner = account(&persistence, "owner").await;
        let admin = account(&persistence, "admin").await;
        let member = account(&persistence, "member").await;
        let company_id = CompanyPersistence::create(
            persistence.as_ref(),
            owner.id,
            CompanyWrite {
                name: "Level tests".into(),
                slug: format!("levels-{}", Uuid::new_v4().simple()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        let fixture = Self {
            pool,
            persistence,
            company_id,
            owner,
            admin,
            member,
        };
        fixture
            .join(&fixture.admin, CompanyAccessRole::Admin, AccessLevel::L4)
            .await;
        fixture
            .join(&fixture.member, CompanyAccessRole::Member, AccessLevel::L3)
            .await;
        fixture
    }
    pub async fn invite(&self, user: &User, level: AccessLevel) -> CompanyInvite {
        self.persistence
            .create_invite(CreateCompanyInvite {
                actor_id: self.owner.id,
                company_id: self.company_id,
                email: user.email.clone().into(),
                role: CompanyAccessRole::Member,
                access_level: level,
            })
            .await
            .unwrap()
    }
    pub async fn join(&self, user: &User, role: CompanyAccessRole, level: AccessLevel) {
        let invite = self
            .persistence
            .create_invite(CreateCompanyInvite {
                actor_id: self.owner.id,
                company_id: self.company_id,
                email: user.email.clone().into(),
                role,
                access_level: level,
            })
            .await
            .unwrap();
        self.persistence
            .accept_pending_invite(invite.id, user.id, &user.email)
            .await
            .unwrap()
            .unwrap();
    }
    pub fn change(
        &self,
        actor: Uuid,
        target: Uuid,
        access_level: AccessLevel,
    ) -> MemberLevelChange {
        MemberLevelChange {
            actor_id: actor,
            company_id: self.company_id,
            member_user_id: target,
            access_level,
        }
    }
    pub async fn level(&self, user: Uuid) -> AccessLevel {
        AccessLevel::try_from(sqlx::query_scalar::<_, i16>("SELECT access_level::smallint FROM company_members WHERE company_id=$1 AND user_id=$2")
            .bind(self.company_id).bind(user).fetch_one(&self.pool).await.unwrap()).unwrap()
    }
}
pub(super) async fn account(p: &PostgresPersistence, label: &str) -> User {
    let name = format!("{label}_{}", Uuid::new_v4().simple());
    let user = p
        .create_user(&name, &format!("{name}@example.test"), "hash")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(user.id)
        .execute(p.pool())
        .await
        .unwrap();
    UserPersistence::get_by_id(p, user.id)
        .await
        .unwrap()
        .unwrap()
}
