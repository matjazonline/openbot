use super::{PostgresPersistence, credentials::CredentialCipher, test_support::test_pool};
use crate::{
    app_error::AppError,
    entities::mcp::*,
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        mcp::*,
        user::UserPersistence,
    },
};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

async fn company(p: &PostgresPersistence) -> Uuid {
    let suffix = Uuid::new_v4().simple().to_string();
    let user = p
        .create_user(&suffix, &format!("{suffix}@example.com"), "hash")
        .await
        .unwrap();
    CompanyPersistence::create(
        p,
        user.id,
        CompanyWrite {
            name: "MCP test".into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
}
fn write(slug: &str) -> McpConnectionWrite {
    let name: McpToolName = "search".to_string().try_into().unwrap();
    McpConnectionWrite {
        slug: slug.into(),
        endpoint: "https://example.com/mcp".to_string().try_into().unwrap(),
        enabled: true,
        auth: McpAuth::Bearer,
        discovered_tools: vec![McpDiscoveredTool {
            name: name.clone(),
            description: "Search".into(),
            input_schema: serde_json::json!({"type":"object"}),
            output_schema: None,
        }],
        tool_grants: vec![name],
    }
}
#[tokio::test]
async fn mcp_shared_configuration_is_tenant_scoped_revisioned_and_preserved() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::with_credential_cipher(pool.clone(), CredentialCipher::for_test());
    let company = company(&p).await;
    let first = p
        .create_mcp_connection(company, write("crm"))
        .await
        .unwrap();
    let second = p
        .create_mcp_connection(company, write("knowledge"))
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    let rev = p
        .replace_mcp_token(
            company,
            first.id,
            first.revision,
            Some(SecretString::from("synthetic-token")),
        )
        .await
        .unwrap();
    assert_eq!(
        p.mcp_token(company, first.id, rev)
            .await
            .unwrap()
            .unwrap()
            .expose_secret(),
        "synthetic-token"
    );
    let ciphertext: String = sqlx::query_scalar(
        "SELECT envelope FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(company)
    .bind(first.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!ciphertext.contains("synthetic-token"));
    assert!(
        !serde_json::to_string(&p.list_mcp_connections(company).await.unwrap())
            .unwrap()
            .contains("synthetic-token")
    );
    // 04.10: an endpoint is fixed for the connection's lifetime (frozen workflow actions
    // target the connection id). The writer refuses, and the trigger backs it up.
    let mut moved = write("crm");
    moved.endpoint = "https://example.org/mcp".to_string().try_into().unwrap();
    assert!(matches!(
        p.update_mcp_connection(company, first.id, rev, moved).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(
        sqlx::query("UPDATE company_mcp_connections SET endpoint_url='https://example.org/mcp' WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(first.id)
            .execute(&pool)
            .await
            .is_err()
    );
    // Changing authentication still rebinds: the stored credential is discarded.
    let mut rebound = write("crm");
    rebound.auth = McpAuth::None;
    let changed = p
        .update_mcp_connection(company, first.id, rev, rebound)
        .await
        .unwrap();
    assert!(!changed.secret_set);
    assert!(p.mcp_token(company, first.id, rev).await.is_err());
    assert_eq!(p.list_mcp_connections(company).await.unwrap().len(), 2);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn mcp_key_rotation_retains_shared_token_and_rejects_row_substitution() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let old = PostgresPersistence::with_credential_cipher(
        pool.clone(),
        CredentialCipher::for_test_with_keys(&[(1, [7; 32])], 1),
    );
    let owner = company(&old).await;
    let first = old
        .create_mcp_connection(owner, write("first"))
        .await
        .unwrap();
    let second = old
        .create_mcp_connection(owner, write("second"))
        .await
        .unwrap();
    let token = "x".repeat(MAX_MCP_SECRET_BYTES);
    let rev = old
        .replace_mcp_token(owner, first.id, 1, Some(SecretString::from(token.clone())))
        .await
        .unwrap();
    assert!(
        old.replace_mcp_token(
            owner,
            first.id,
            rev,
            Some(SecretString::from("x".repeat(MAX_MCP_SECRET_BYTES + 1)))
        )
        .await
        .is_err()
    );
    let cipher = CredentialCipher::for_test_with_keys(&[(1, [7; 32]), (2, [8; 32])], 2);
    let context =
        super::credentials::envelope::CredentialContext::company_mcp_credential(owner, first.id);
    let envelope: String = sqlx::query_scalar(
        "SELECT envelope FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(owner)
    .bind(first.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let rotated = cipher
        .rotate_to_active(
            &super::credentials::CredentialFormat::Envelope(context.clone()),
            &envelope,
        )
        .unwrap();
    assert_eq!(
        cipher
            .open_envelope(&context, &rotated)
            .unwrap()
            .expose_secret(),
        &token
    );
    assert!(
        cipher
            .open_envelope(
                &super::credentials::envelope::CredentialContext::company_mcp_credential(
                    owner, second.id
                ),
                &rotated
            )
            .is_err()
    );
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn mcp_connection_capacity_is_atomic_for_competing_creators() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool.clone());
    let owner = company(&p).await;
    sqlx::query("INSERT INTO company_mcp_connections(company_id,id,slug,endpoint_url,auth_type) SELECT $1,gen_random_uuid(),'seed-' || value,'https://example.com/mcp','none' FROM generate_series(1,63) AS value")
        .bind(owner).execute(&pool).await.unwrap();
    let (left, right) = tokio::join!(
        p.create_mcp_connection(owner, write("left")),
        p.create_mcp_connection(owner, write("right"))
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(p.list_mcp_connections(owner).await.unwrap().len(), 64);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
}

#[path = "mcp_read_tests.rs"]
mod reads;
