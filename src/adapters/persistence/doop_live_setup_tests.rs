//! Explicit live smoke helper. Credentials arrive only through private stdin.
use super::{PostgresPersistence, credentials::CredentialCipher, test_support::own_database};
use crate::{
    adapters::mcp::{HttpMcpClient, policy::EndpointPolicy},
    entities::mcp::{McpAuth, McpEndpoint},
    services::mcp_client::{McpCallResult, McpClient, McpSession},
    use_cases::{
        company::{CompanyPersistence, CompanyWrite},
        mcp::{McpConnectionWrite, McpCredentialPersistence, McpUseCases},
        user::UserPersistence,
    },
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    sync::Arc,
};
use uuid::Uuid;

use super::doop_live_input::*;

#[path = "doop_live_lifecycle_tests.rs"]
mod lifecycle;
#[path = "doop_live_observation.rs"]
mod observation;
#[path = "doop_live_workflow.rs"]
mod workflow;

pub(super) struct Provisioned {
    pub(super) actor: Uuid,
    pub(super) connection: crate::entities::mcp::CompanyMcpConnection,
    mapping: Value,
    verification: workflow::Verification,
}

pub(super) struct RegisteredCompany {
    pub(super) actor: Uuid,
    pub(super) connection: crate::entities::mcp::CompanyMcpConnection,
}

pub(super) async fn register_company(
    persistence: &Arc<PostgresPersistence>,
    service: &McpUseCases,
    endpoint: &McpEndpoint,
    company: &Company,
) -> RegisteredCompany {
    let suffix = Uuid::new_v4().simple().to_string();
    let actor = persistence
        .create_user(&suffix, &format!("{suffix}@example.test"), "fixture-hash")
        .await
        .unwrap()
        .id;
    let owned = CompanyPersistence::create(
        persistence.as_ref(),
        actor,
        CompanyWrite {
            name: "Disposable Doop company".into(),
            slug: suffix,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let connection = service
        .create(
            actor,
            owned.id,
            McpConnectionWrite {
                slug: "doop".to_owned().into(),
                endpoint: endpoint.clone(),
                enabled: true,
                auth: McpAuth::Bearer,
                discovered_tools: vec![],
                tool_grants: vec![],
            },
        )
        .await
        .unwrap();
    service
        .replace_token(actor, &connection, Some(company.token.clone()))
        .await
        .unwrap();
    let connection = service.get(actor, owned.id, connection.id).await.unwrap();
    let connection = service
        .refresh(
            actor,
            &connection,
            crate::application::integrations::VerifiedOwner::from_verified_client(
                crate::adapters::integrations::owner::owner_for_user(actor),
            ),
        )
        .await
        .unwrap();
    assert!(connection.secret_set && connection.tool_grants.is_empty());
    let token = persistence
        .mcp_token(owned.id, connection.id, connection.revision)
        .await
        .unwrap()
        .unwrap();
    let encrypted: String = sqlx::query_scalar(
        "SELECT envelope FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(owned.id)
    .bind(connection.id)
    .fetch_one(persistence.pool())
    .await
    .unwrap();
    assert!(!encrypted.contains(company.token.expose_secret()));
    assert!(token.expose_secret() == company.token.expose_secret());
    RegisteredCompany { actor, connection }
}

pub(super) async fn provision(
    persistence: &Arc<PostgresPersistence>,
    service: &McpUseCases,
    endpoint: &McpEndpoint,
    company: &Company,
    engine: &crate::infra::workflow::WorkflowEngine,
) -> Provisioned {
    // Box shared normal registration so native helpers retain their stock-stack budget.
    let registered = Box::pin(register_company(persistence, service, endpoint, company)).await;
    let actor = registered.actor;
    let connection = registered.connection;
    let connection = workflow::prepare(service, engine, actor, &connection).await;
    let verification_handle = Box::pin(workflow::freeze_verification(
        engine,
        actor,
        &connection,
        company,
    ))
    .await;
    let verification = Box::pin(verification_handle.verified(persistence, engine, company)).await;
    let record = json!({"openbot_company":connection.company_id,"connection_id":connection.id,"connection_slug":"doop",
        "workspace_id":company.workspace,"bot_user_id":company.bot,"intended_humans":company.humans,
        "approved_audience":"entire configured workspace; synthetic data only", "verification":verification});
    Provisioned {
        actor,
        connection,
        mapping: record,
        verification: verification_handle,
    }
}

fn write_record(input: &Input, provisioned: &[Provisioned], observed: &observation::Observation) {
    let mappings = provisioned
        .iter()
        .map(|value| &value.mapping)
        .collect::<Vec<_>>();
    let record = json!({"version":1,"disposable":true,"doop_origin":input.origin,"source_identity":input.source,
        "mcp_tool_calls":observed.tool_calls,"catalog_discoveries":observed.catalog_discoveries,"mappings":mappings,
        "teardown":"owned Openbot database dropped when helper exits; Doop data belongs to the disposable runner"});
    let encoded = serde_json::to_vec_pretty(&record).unwrap();
    assert!(encoded.len() <= 16384);
    for company in &input.companies {
        assert!(!String::from_utf8_lossy(&encoded).contains(company.token.expose_secret()));
    }
    let output = std::env::var_os("DOOP_TEST_SETUP_RECORD").expect("private setup report path");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap()
        .write_all(&encoded)
        .unwrap();
}

async fn assert_scoped(
    service: &McpUseCases,
    persistence: &PostgresPersistence,
    provisioned: &[Provisioned],
    input: &Input,
) {
    let [left, right] = provisioned else {
        unreachable!()
    };
    assert!(
        service
            .get(left.actor, left.connection.company_id, right.connection.id)
            .await
            .is_err()
    );
    assert!(
        service
            .list(left.actor, right.connection.company_id)
            .await
            .is_err()
    );
    assert!(
        service
            .replace_token(
                left.actor,
                &right.connection,
                Some(input.companies[0].token.clone())
            )
            .await
            .is_err()
    );
    assert!(!matches!(
        persistence
            .mcp_token(
                left.connection.company_id,
                right.connection.id,
                right.connection.revision
            )
            .await,
        Ok(Some(_))
    ));
}

pub(super) fn guarded_client(input: &Input) -> (McpEndpoint, Arc<HttpMcpClient>) {
    let endpoint: McpEndpoint = format!("{}/mcp", input.origin.trim_end_matches('/'))
        .try_into()
        .unwrap();
    let mut policy = EndpointPolicy::new([endpoint.as_str().to_owned()]).unwrap();
    if let Some(path) = std::env::var_os("DOOP_TEST_OPENBOT_CA") {
        use std::io::Read;
        let mut pem = Vec::new();
        std::fs::File::open(path)
            .unwrap()
            .take(16_385)
            .read_to_end(&mut pem)
            .unwrap();
        assert!(!pem.is_empty() && pem.len() <= 16_384);
        policy = policy.with_test_root_certificate(reqwest::Certificate::from_pem(&pem).unwrap());
    }
    (endpoint, Arc::new(HttpMcpClient::new(policy)))
}

fn live_client(input: &Input) -> (McpEndpoint, Arc<observation::ObservedClient>) {
    let (endpoint, client) = guarded_client(input);
    (endpoint, Arc::new(observation::ObservedClient::new(client)))
}

#[tokio::test]
#[ignore = "explicit disposable live Doop and owned PostgreSQL; private stdin required"]
async fn doop_live_company_setup() {
    let input = input();
    let database = own_database().await.expect("owned PostgreSQL is required");
    let persistence = Arc::new(PostgresPersistence::with_credential_cipher(
        database.pool.clone(),
        CredentialCipher::for_test(),
    ));
    let (endpoint, client) = live_client(&input);
    let engine = crate::infra::workflow::WorkflowEngine::for_tests_with_mcp(
        persistence.as_ref().clone(),
        client.clone(),
    );
    let service = McpUseCases::new(
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        client.clone(),
        Arc::new(crate::use_cases::mcp::UnavailableMcpDiscovery),
    );
    let mut provisioned = Vec::new();
    for company in &input.companies {
        let created = Box::pin(provision(
            &persistence,
            &service,
            &endpoint,
            company,
            &engine,
        ))
        .await;
        provisioned.push(created);
    }
    assert_scoped(&service, &persistence, &provisioned, &input).await;
    service.shutdown().await.unwrap();
    let observed = client.observation();
    assert_eq!(observed.tool_calls, 4);
    assert_eq!(observed.catalog_discoveries, 6);
    write_record(&input, &provisioned, &observed);
    println!(
        "DOOP_OPENBOT_SETUP_PASS companies=2 tool_calls={}",
        observed.tool_calls
    );
}
