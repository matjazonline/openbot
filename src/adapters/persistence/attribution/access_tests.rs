use super::*;
use crate::{
    application::use_cases::company_invite::CompanyInviteUseCases,
    entities::{access_level::AccessLevel, company_member::CompanyAccessRole},
};

struct MembershipFixture {
    http: HttpFixture,
    bearer: String,
    owner: Uuid,
    own: Uuid,
    admin: Uuid,
    member: Uuid,
    foreign: Uuid,
    membership: CompanyInviteUseCases,
}
impl MembershipFixture {
    async fn new() -> Self {
        let http = HttpFixture::new().await;
        let own = http.f.company().await;
        let suffix = Uuid::new_v4().simple().to_string();
        let owner = http
            .f
            .store
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap()
            .id;
        let [admin, member, foreign] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
        for company in [admin, member, foreign] {
            insert_company_with_owner(
                &http.f.store.pool,
                company,
                "Other owner",
                &company.simple().to_string(),
                owner,
            )
            .await;
        }
        for (company, role) in [
            (admin, CompanyAccessRole::Admin),
            (member, CompanyAccessRole::Member),
        ] {
            crate::adapters::persistence::test_support::insert_root_member(
                &http.f.store.pool,
                crate::domain::workflow::CompanyId::new(company),
                http.f.user,
                role,
                AccessLevel::PUBLIC,
            )
            .await;
        }
        let code = http.code().await;
        let response = http.redeem(&code, &http.verifier).await;
        assert_eq!(response.status(), StatusCode::OK);
        let token: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let bearer = format!("Bearer {}", token["access_token"].as_str().unwrap());
        let store = Arc::new(PostgresPersistence::new(http.f.store.pool.clone()));
        let membership = CompanyInviteUseCases::new(store.clone(), store);
        Self {
            http,
            bearer,
            owner,
            own,
            admin,
            member,
            foreign,
            membership,
        }
    }
    async fn get(&self, path: &str) -> (StatusCode, serde_json::Value) {
        let response = self
            .http
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::AUTHORIZATION, &self.bearer)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 131072).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }
    async fn permissions(&self, company: Uuid) -> (StatusCode, serde_json::Value) {
        let (status, value) = self
            .get(&format!("/api/attribution/v1/companies/{company}/access"))
            .await;
        (status, value["permissions"].clone())
    }
    async fn suspend(&self, company: Uuid, expected_version: i64) -> StatusCode {
        self.http.app.clone().oneshot(Request::builder()
            .uri(format!("/api/attribution/v1/companies/{company}/status")).method("POST")
            .header(header::COOKIE, &self.http.cookie).header(header::HOST, "localhost:3099")
            .header(header::ORIGIN, "http://localhost:3099").header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::json!({"state":"suspended","expected_version":expected_version}).to_string())).unwrap())
            .await.unwrap().status()
    }
    async fn company_ids(&self) -> Vec<Uuid> {
        let mut path = "/api/attribution/v1/companies?limit=1".to_owned();
        let mut ids = Vec::new();
        for _ in 0..4 {
            let (status, page) = self.get(&path).await;
            assert_eq!(status, StatusCode::OK);
            let items = page["items"].as_array().unwrap();
            assert!(items.len() <= 1);
            ids.extend(
                items
                    .iter()
                    .map(|item| Uuid::parse_str(item["company_id"].as_str().unwrap()).unwrap()),
            );
            let Some(cursor) = page["next_cursor"].as_str() else {
                return ids;
            };
            let query = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("limit", "1")
                .append_pair("cursor", cursor)
                .finish();
            path = format!("/api/attribution/v1/companies?{query}");
        }
        panic!("scoped company pagination did not terminate");
    }
}

#[tokio::test]
async fn attribution_company_access_real_roles_downgrade_removal_and_foreign_status_writes() {
    let f = MembershipFixture::new().await;
    let manager = serde_json::json!(["read", "links:manage", "integrations:manage"]);
    for company in [f.own, f.admin] {
        assert_eq!(
            f.permissions(company).await,
            (StatusCode::OK, manager.clone())
        );
        assert_eq!(f.suspend(company, 1).await, StatusCode::OK);
    }
    assert_eq!(
        f.permissions(f.member).await,
        (StatusCode::OK, serde_json::json!(["read"]))
    );
    assert_eq!(f.suspend(f.member, 1).await, StatusCode::FORBIDDEN);
    assert_eq!(f.permissions(f.foreign).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.suspend(f.foreign, 1).await, StatusCode::NOT_FOUND);
    f.membership
        .update_company_team_member_role(f.owner, f.admin, f.http.f.user, CompanyAccessRole::Member)
        .await
        .unwrap();
    // The same identity grant and browser session must observe the committed downgrade.
    assert_eq!(
        f.permissions(f.admin).await,
        (StatusCode::OK, serde_json::json!(["read"]))
    );
    assert_eq!(f.suspend(f.admin, 2).await, StatusCode::FORBIDDEN);
    f.membership
        .remove_company_team_member(f.owner, f.admin, f.http.f.user)
        .await
        .unwrap();
    assert_eq!(f.permissions(f.admin).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.suspend(f.admin, 2).await, StatusCode::NOT_FOUND);
    assert_eq!(f.permissions(f.own).await, (StatusCode::OK, manager));
}

#[tokio::test]
async fn attribution_company_access_real_multi_company_pagination_tracks_membership() {
    let f = MembershipFixture::new().await;
    let mut expected = vec![f.own, f.admin, f.member];
    expected.sort();
    assert_eq!(f.company_ids().await, expected);
    f.membership
        .remove_company_team_member(f.owner, f.admin, f.http.f.user)
        .await
        .unwrap();
    expected.retain(|id| *id != f.admin);
    assert_eq!(f.company_ids().await, expected);
    assert!(!expected.contains(&f.foreign));
}

#[tokio::test]
async fn attribution_company_access_competing_root_admin_status_writes_have_one_winner() {
    let f = MembershipFixture::new().await;
    let (first, second) = tokio::join!(f.suspend(f.admin, 1), f.suspend(f.admin, 1));
    let mut outcomes = [first, second];
    outcomes.sort_unstable_by_key(|status| status.as_u16());
    assert_eq!(outcomes, [StatusCode::OK, StatusCode::CONFLICT]);
    let status = f.http.f.store.status(f.admin).await.unwrap();
    assert_eq!(status.state, CompanyState::Suspended);
    assert_eq!(status.version, 2);
}
