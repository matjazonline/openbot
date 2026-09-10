use super::{PostgresPersistence, credentials::CredentialCipher, test_support::test_pool};
use crate::{
    entities::{harness::HarnessKind, mcp::*},
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
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
        }],
        tool_grants: vec![name],
    }
}
async fn agent(p: &PostgresPersistence, company: Uuid, slug: &str) -> Uuid {
    AgentPersistence::create(
        p,
        company,
        AgentWrite {
            name: slug.into(),
            slug: slug.into(),
            harness_kind: Some(HarnessKind::Rig),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .id
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
    let a = agent(&p, company, "first").await;
    let b = agent(&p, company, "second").await;
    let selected = p
        .replace_agent_mcp_selection(
            p.agent_mcp_selection(company, a).await.unwrap(),
            Some(vec![first.id, second.id]),
        )
        .await
        .unwrap();
    p.replace_agent_mcp_selection(
        p.agent_mcp_selection(company, b).await.unwrap(),
        Some(vec![first.id]),
    )
    .await
    .unwrap();
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
    AgentPersistence::update(
        &p,
        a,
        AgentWrite {
            name: "Renamed".into(),
            slug: "first".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(p.agent_mcp_selection(company, a).await.unwrap(), selected);
    let mut changed = write("crm");
    changed.endpoint = "https://example.org/mcp".to_string().try_into().unwrap();
    let changed = p
        .update_mcp_connection(company, first.id, rev, changed)
        .await
        .unwrap();
    assert!(!changed.secret_set);
    assert!(p.mcp_token(company, first.id, rev).await.is_err());
    assert_eq!(
        p.agent_mcp_selection(company, b)
            .await
            .unwrap()
            .connection_ids,
        vec![first.id]
    );
    assert!(
        AgentPersistence::update(
            &p,
            a,
            AgentWrite {
                name: "Renamed".into(),
                slug: "first".into(),
                harness_kind: Some(HarnessKind::AiAgents),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    p.replace_agent_mcp_selection(selected, Some(vec![]))
        .await
        .unwrap();
    AgentPersistence::delete(&p, a).await.unwrap();
    assert_eq!(p.list_mcp_connections(company).await.unwrap().len(), 2);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn mcp_competing_selection_and_definition_writes_have_one_winner() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::with_credential_cipher(pool.clone(), CredentialCipher::for_test());
    let owner = company(&p).await;
    let foreign = company(&p).await;
    let a = agent(&p, owner, "race").await;
    let connection = p.create_mcp_connection(owner, write("crm")).await.unwrap();
    let other = p
        .create_mcp_connection(foreign, write("foreign"))
        .await
        .unwrap();
    let initial = p.agent_mcp_selection(owner, a).await.unwrap();
    assert!(
        p.replace_agent_mcp_selection(initial.clone(), Some(vec![other.id]))
            .await
            .is_err()
    );
    let err = sqlx::query(
        "INSERT INTO agent_mcp_selections(company_id,agent_id,connection_id) VALUES($1,$2,$3)",
    )
    .bind(owner)
    .bind(a)
    .bind(other.id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_eq!(
        err.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    let (left, right) = tokio::join!(
        p.replace_agent_mcp_selection(initial.clone(), Some(vec![connection.id])),
        p.replace_agent_mcp_selection(initial, Some(vec![]))
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let (left, right) = tokio::join!(
        p.update_mcp_connection(owner, connection.id, connection.revision, write("left")),
        p.update_mcp_connection(owner, connection.id, connection.revision, write("right"))
    );
    assert_ne!(left.is_ok(), right.is_ok());
    sqlx::query("DELETE FROM companies WHERE id=ANY($1)")
        .bind(vec![owner, foreign])
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

#[tokio::test]
async fn mcp_selection_and_effective_grant_limits_reject_the_whole_edit() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool.clone());
    let owner = company(&p).await;
    let a = agent(&p, owner, "bounded").await;
    let mut definition = write("first");
    definition.discovered_tools = (0..4)
        .map(|i| McpDiscoveredTool {
            name: format!("tool-{i}").try_into().unwrap(),
            description: String::new(),
            input_schema: serde_json::json!({"type":"object"}),
        })
        .collect();
    definition.tool_grants = definition
        .discovered_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    let mut ids = vec![];
    for i in 0..9 {
        definition.slug = format!("connection-{i}").into();
        ids.push(
            p.create_mcp_connection(owner, definition.clone())
                .await
                .unwrap()
                .id,
        );
    }
    let initial = p.agent_mcp_selection(owner, a).await.unwrap();
    assert!(
        p.replace_agent_mcp_selection(initial.clone(), Some(ids.clone()))
            .await
            .is_err()
    );
    let selected = p
        .replace_agent_mcp_selection(initial, Some(ids[..8].to_vec()))
        .await
        .unwrap();
    definition.slug = "connection-0".into();
    definition.discovered_tools.push(McpDiscoveredTool {
        name: "extra".to_string().try_into().unwrap(),
        description: String::new(),
        input_schema: serde_json::json!({}),
    });
    definition.tool_grants = definition
        .discovered_tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect();
    assert!(
        p.update_mcp_connection(owner, ids[0], 1, definition)
            .await
            .is_err()
    );
    assert_eq!(p.agent_mcp_selection(owner, a).await.unwrap(), selected);
    assert_eq!(p.list_mcp_connections(owner).await.unwrap()[0].revision, 1);
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
}

#[path = "mcp_flow_tests.rs"]
mod flows;

#[path = "mcp_read_tests.rs"]
mod reads;
