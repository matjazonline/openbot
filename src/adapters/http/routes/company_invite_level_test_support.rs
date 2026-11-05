use super::*;
use crate::{
    adapters::http::routes::authorization_test_support::{Fixture, denied},
    entities::user::User,
    use_cases::{
        company_invite::{CompanyInvitePersistence, CreateCompanyInvite},
        user::UserPersistence,
    },
};
use axum::response::Response;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    MemberHtml,
    MemberJson,
    InviteHtml,
    InviteJson,
}
pub(super) struct Request<'a> {
    pub actor: &'a User,
    pub company_id: Uuid,
    pub target_id: Uuid,
    pub level: AccessLevel,
}
impl Endpoint {
    pub async fn invoke(self, fixture: &Fixture, request: Request<'_>) -> AppResult<Response> {
        let state = State(fixture.invites.clone());
        let actor = Fixture::auth(request.actor);
        let path = Path((request.company_id, request.target_id));
        let form = LevelForm {
            access_level: request.level,
        };
        match self {
            Self::MemberHtml => set_member_level(state, actor, path, Form(form))
                .await
                .map(IntoResponse::into_response),
            Self::MemberJson => set_member_level_json(state, actor, path, Json(form))
                .await
                .map(IntoResponse::into_response),
            Self::InviteHtml => set_invite_level(state, actor, path, Form(form))
                .await
                .map(IntoResponse::into_response),
            Self::InviteJson => set_invite_level_json(state, actor, path, Json(form))
                .await
                .map(IntoResponse::into_response),
        }
    }
}
pub(super) struct RouteFixture {
    pub base: Fixture,
    pub foreign_member: User,
    pub local_invite: CompanyInvite,
    pub foreign_invite: CompanyInvite,
}
impl RouteFixture {
    pub fn local<'a>(&self, actor: &'a User, target_id: Uuid, level: AccessLevel) -> Request<'a> {
        Request {
            actor,
            company_id: self.base.company.id,
            target_id,
            level,
        }
    }
    pub async fn new() -> Option<Self> {
        let base = Fixture::new().await?;
        let name = format!("foreign_level_{}", Uuid::new_v4().simple());
        let foreign_member = base
            .persistence
            .create_user(&name, &format!("{name}@example.test"), "hash")
            .await
            .unwrap();
        sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
            .bind(foreign_member.id)
            .execute(base.persistence.pool())
            .await
            .unwrap();
        let foreign_invite = base
            .invites
            .create_company_invite(CreateCompanyInvite {
                actor_id: base.stranger.id,
                company_id: base.foreign.id,
                email: foreign_member.email.clone().into(),
                role: CompanyAccessRole::Member,
                access_level: AccessLevel::L2,
            })
            .await
            .unwrap();
        base.persistence
            .accept_pending_invite(foreign_invite.id, foreign_member.id, &foreign_member.email)
            .await
            .unwrap()
            .unwrap();
        // A second pending invite is independently editable and remains outside the active tenant.
        let foreign_invite = base
            .invites
            .create_company_invite(CreateCompanyInvite {
                actor_id: base.stranger.id,
                company_id: base.foreign.id,
                email: format!("pending-{name}@example.test").into(),
                role: CompanyAccessRole::Member,
                access_level: AccessLevel::L2,
            })
            .await
            .unwrap();
        let local_invite = base
            .invites
            .create_company_invite(CreateCompanyInvite {
                actor_id: base.owner.id,
                company_id: base.company.id,
                email: format!("local-{name}@example.test").into(),
                role: CompanyAccessRole::Member,
                access_level: AccessLevel::PUBLIC,
            })
            .await
            .unwrap();
        Some(Self {
            base,
            foreign_member,
            local_invite,
            foreign_invite,
        })
    }
    pub async fn snapshot(&self) -> StoredFacts {
        let f = &self.base;
        let p = f.persistence.as_ref();
        StoredFacts {
            local_member: serde_json::to_value(
                p.get_member_by_user_id(f.company.id, f.member.id)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
            foreign_member: serde_json::to_value(
                p.get_member_by_user_id(f.foreign.id, self.foreign_member.id)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
            local_invite: serde_json::to_value(
                p.get_invite_by_id(f.company.id, self.local_invite.id)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
            foreign_invite: serde_json::to_value(
                p.get_invite_by_id(f.foreign.id, self.foreign_invite.id)
                    .await
                    .unwrap()
                    .unwrap(),
            )
            .unwrap(),
        }
    }
    pub async fn refuse(&self, endpoint: Endpoint, request: Request<'_>, expected: StatusCode) {
        let before = self.snapshot().await;
        denied(
            endpoint.invoke(&self.base, request).await,
            expected,
            &self.base.foreign.name,
        )
        .await;
        assert_eq!(
            self.snapshot().await,
            before,
            "refusal changed a level, role, address or another stored fact"
        );
    }
    pub async fn make_admin(&self) {
        sqlx::query("UPDATE company_members SET role='admin',access_level=4 WHERE company_id=$1 AND user_id=$2")
            .bind(self.base.company.id).bind(self.base.member.id).execute(self.base.persistence.pool()).await.unwrap();
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct StoredFacts {
    pub local_member: Value,
    pub foreign_member: Value,
    pub local_invite: Value,
    pub foreign_invite: Value,
}
pub(super) fn only_level_changed(before: &Value, after: &Value, level: AccessLevel) {
    assert_eq!(after["access_level"], serde_json::json!(level.as_i16()));
    let mut before = before.clone();
    let mut after = after.clone();
    before.as_object_mut().unwrap().remove("access_level");
    after.as_object_mut().unwrap().remove("access_level");
    assert_eq!(
        before, after,
        "level endpoint changed role, email or another fact"
    );
}
