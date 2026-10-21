use super::*;
use crate::{
    adapters::{
        mcp::{HttpMcpClient, policy::EndpointPolicy},
        persistence::{
            PostgresPersistence, credentials::CredentialCipher, test_support::test_pool,
        },
    },
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        mcp::McpUseCases,
        user::UserPersistence,
    },
};
use std::sync::Arc;

struct Fixture {
    service: Arc<McpUseCases>,
    user: Uuid,
    company: Uuid,
    other: Uuid,
}
async fn fixture() -> Option<Fixture> {
    let pool = test_pool().await?;
    let persistence = Arc::new(PostgresPersistence::with_credential_cipher(
        pool,
        CredentialCipher::for_test(),
    ));
    let suffix = Uuid::new_v4().simple().to_string();
    let user = persistence
        .create_user(&suffix, &format!("{suffix}@example.com"), "hash")
        .await
        .unwrap()
        .id;
    let mut companies = Vec::new();
    for i in 0..2 {
        companies.push(
            CompanyPersistence::create(
                persistence.as_ref(),
                user,
                CompanyWrite {
                    name: "MCP API".into(),
                    slug: format!("{suffix}-{i}"),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .id,
        );
    }
    Some(Fixture {
        service: Arc::new(McpUseCases::new(
            persistence.clone(),
            persistence.clone(),
            persistence,
            Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
        )),
        user,
        company: companies[0],
        other: companies[1],
    })
}
fn definition() -> Definition {
    serde_json::from_value(serde_json::json!({"slug":"crm","endpoint_url":"https://example.com/mcp","transport":"streamable_http","enabled":true,"auth":{"type":"none"}})).unwrap()
}
#[tokio::test]
async fn mcp_api_scopes_objects_even_when_the_actor_manages_both_companies() {
    let Some(f) = fixture().await else { return };
    let actor = || AuthenticatedUser { id: f.user };
    let (_, Json(created)) = create(
        State(f.service.clone()),
        actor(),
        Path(f.company),
        Json(definition()),
    )
    .await
    .unwrap();
    let id = serde_json::from_value(created["id"].clone()).unwrap();
    assert_eq!(created["auth"]["secret_set"], false);
    assert!(!created.to_string().contains("token"));
    assert!(
        detail(State(f.service.clone()), actor(), Path((f.other, id)))
            .await
            .is_err()
    );
    assert!(
        delete(
            State(f.service.clone()),
            actor(),
            Path((f.other, id)),
            Json(Revision {
                expected_revision: 1
            })
        )
        .await
        .is_err()
    );
    assert!(
        refresh(
            State(f.service.clone()),
            actor(),
            Path((f.other, id)),
            Json(Revision {
                expected_revision: 1
            })
        )
        .await
        .is_err()
    );
    assert!(
        credential(
            State(f.service.clone()),
            actor(),
            Path((f.other, id)),
            Json(Credential {
                expected_revision: 1,
                token: None
            })
        )
        .await
        .is_err()
    );
    assert!(
        list(
            State(f.service.clone()),
            AuthenticatedUser { id: Uuid::new_v4() },
            Path(f.company)
        )
        .await
        .is_err()
    );
    f.service.shutdown().await.unwrap();
}
