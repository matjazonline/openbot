use super::*;
use std::io::Cursor;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LifecycleInput {
    base: Input,
    retired_tokens: [SecretString; 2],
}

fn lifecycle_input(reader: impl Read) -> LifecycleInput {
    let input: LifecycleInput = bounded_input(reader);
    validate_input(&input.base);
    for (retired, company) in input.retired_tokens.iter().zip(&input.base.companies) {
        let retired = retired.expose_secret();
        assert!(!retired.is_empty() && retired.len() <= 8192);
        assert!(retired != company.token.expose_secret());
    }
    input
}

async fn set_enabled(
    service: &McpUseCases,
    current: &crate::entities::mcp::CompanyMcpConnection,
    actor: Uuid,
    enabled: bool,
) -> crate::entities::mcp::CompanyMcpConnection {
    service
        .update(
            actor,
            current,
            McpConnectionWrite {
                slug: current.slug.clone(),
                endpoint: current.endpoint.clone(),
                enabled,
                auth: current.auth.clone(),
                discovered_tools: current.discovered_tools.clone(),
                tool_grants: current.tool_grants.clone(),
            },
        )
        .await
        .unwrap()
}

async fn refused(
    frozen: &workflow::Verification,
    p: &PostgresPersistence,
    engine: &crate::infra::workflow::WorkflowEngine,
) {
    if let Ok((_, view)) = Box::pin(frozen.execute(p, engine)).await {
        assert_eq!(
            view.detail.summary.state, "failed",
            "revoked authority cannot complete the frozen workflow"
        );
    }
}

struct LiveLifecycle<'a> {
    p: &'a Arc<PostgresPersistence>,
    service: &'a McpUseCases,
    endpoint: &'a McpEndpoint,
    engine: &'a crate::infra::workflow::WorkflowEngine,
    client: &'a observation::ObservedClient,
}

async fn disconnect(
    live: &LiveLifecycle<'_>,
    actor: Uuid,
    current: &crate::entities::mcp::CompanyMcpConnection,
    frozen: &workflow::Verification,
    index: usize,
) -> i64 {
    live.service.delete(actor, current).await.unwrap();
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=disconnected");
    let disconnected_calls = live.client.observation().tool_calls;
    Box::pin(refused(frozen, live.p, live.engine)).await;
    assert_eq!(live.client.observation().tool_calls, disconnected_calls);
    let credentials: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM company_mcp_credentials WHERE company_id=$1 AND connection_id=$2",
    )
    .bind(current.company_id)
    .bind(current.id)
    .fetch_one(live.p.pool())
    .await
    .unwrap();
    assert_eq!(credentials, 0);
    credentials
}

async fn company_lifecycle(
    live: &LiveLifecycle<'_>,
    company: &Company,
    retired: &SecretString,
    index: usize,
) -> Value {
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=provision");
    let owned = Box::pin(provision(
        live.p,
        live.service,
        live.endpoint,
        company,
        live.engine,
    ))
    .await;
    let mut current = owned.connection;
    // Provisioning has already executed this exact frozen binding with the current key.
    let frozen = owned.verification;
    let before = owned.mapping["verification"].clone();
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=retired-key");
    let initial_revision = current.revision;
    let retired_revision = live
        .service
        .replace_token(owned.actor, &current, Some(retired.clone()))
        .await
        .unwrap();
    assert!(retired_revision > initial_revision);
    current = live
        .service
        .get(owned.actor, current.company_id, current.id)
        .await
        .unwrap();
    Box::pin(refused(&frozen, live.p, live.engine)).await;
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=restore-current-key");
    let restored_revision = live
        .service
        .replace_token(owned.actor, &current, Some(company.token.clone()))
        .await
        .unwrap();
    assert!(restored_revision > retired_revision);
    current = live
        .service
        .get(owned.actor, current.company_id, current.id)
        .await
        .unwrap();
    let rotated = Box::pin(frozen.verified(live.p, live.engine, company)).await;
    assert_eq!(before["version_id"], rotated["version_id"]);
    assert_eq!(before["binding_id"], rotated["binding_id"]);
    current = set_enabled(live.service, &current, owned.actor, false).await;
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=disabled");
    let disabled_calls = live.client.observation().tool_calls;
    Box::pin(refused(&frozen, live.p, live.engine)).await;
    assert_eq!(
        live.client.observation().tool_calls,
        disabled_calls,
        "disabled connection makes no MCP call"
    );
    current = set_enabled(live.service, &current, owned.actor, true).await;
    println!("DOOP_LIFECYCLE_STAGE company={index} stage=reenabled");
    let reenabled = Box::pin(frozen.verified(live.p, live.engine, company)).await;
    assert_eq!(before["binding_id"], reenabled["binding_id"]);
    // Box the runtime re-entry at the disconnect phase.
    let credentials = Box::pin(disconnect(live, owned.actor, &current, &frozen, index)).await;
    json!({"connection_id":current.id,"openbot_company":current.company_id,
        "workspace_id":company.workspace,"bot_user_id":company.bot,
        "initial_revision":initial_revision,"retired_revision":retired_revision,"restored_revision":restored_revision,
        "frozen_verification":reenabled,"retired_key_refused":true,"disabled_without_dispatch":true,
        "disconnected_without_dispatch":true,"credential_rows_after_disconnect":credentials})
}

#[tokio::test]
#[ignore = "explicit disposable live Doop and owned PostgreSQL; private lifecycle stdin required"]
async fn doop_lifecycle_live_company_keys() {
    let input = lifecycle_input(std::io::stdin().lock());
    let database = own_database().await.expect("owned PostgreSQL is required");
    let p = Arc::new(PostgresPersistence::with_credential_cipher(
        database.pool.clone(),
        CredentialCipher::for_test(),
    ));
    let (endpoint, client) = live_client(&input.base);
    let engine = crate::infra::workflow::WorkflowEngine::for_tests_with_mcp(
        p.as_ref().clone(),
        client.clone(),
    );
    let service = McpUseCases::new(
        p.clone(),
        p.clone(),
        p.clone(),
        client.clone(),
        Arc::new(crate::use_cases::mcp::UnavailableMcpDiscovery),
    );
    let mut outcomes = Vec::new();
    let live = LiveLifecycle {
        p: &p,
        service: &service,
        endpoint: &endpoint,
        engine: &engine,
        client: &client,
    };
    for (index, (company, retired)) in input
        .base
        .companies
        .iter()
        .zip(&input.retired_tokens)
        .enumerate()
    {
        // Box each real external-runtime iteration to keep this helper's poll frame shallow.
        outcomes.push(Box::pin(company_lifecycle(&live, company, retired, index)).await);
    }
    service.shutdown().await.unwrap();
    let record = json!({"version":1,"source_identity":input.base.source,"companies":outcomes,
        "remote_documents_deleted":false,"mcp_tool_calls":client.observation().tool_calls});
    let encoded = serde_json::to_vec_pretty(&record).unwrap();
    assert!(encoded.len() <= 16384);
    for token in input
        .base
        .companies
        .iter()
        .map(|c| &c.token)
        .chain(input.retired_tokens.iter())
    {
        assert!(!String::from_utf8_lossy(&encoded).contains(token.expose_secret()));
    }
    let path = std::env::var_os("DOOP_TEST_SETUP_RECORD").expect("private lifecycle record path");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(&encoded).unwrap();
    println!("DOOP_OPENBOT_LIFECYCLE_PASS companies=2 retired_refused=2 disconnected=2");
}

fn boundary_input() -> Vec<u8> {
    let scheme = if std::env::var_os("DOOP_TEST_OPENBOT_CA").is_some() {
        "https"
    } else {
        "http"
    };
    serde_json::to_vec(&json!({
        "origin":format!("{scheme}://127.0.0.1:4400"),
        "source":{"revision":"01584b34f6cadcf21767127280cc6e320a6d748a","fingerprint":"a".repeat(64)},
        "companies":(["a","b"].map(|id| json!({
            "verification_canvas":format!("canvas-{id}"),"workspace":format!("workspace-{id}"),
            "bot":format!("bot-{id}"),"token":id.repeat(8192),
            "humans":[{"id":format!("human-{id}"),"email":format!("human-{id}@example.test")}]
        })))
    })).unwrap()
}

#[test]
fn doop_lifecycle_input_accepts_valid_company_keys_above_the_separate_output_cap() {
    let encoded = boundary_input();
    assert!(encoded.len() > 16384 && encoded.len() < 65536);
    let input: Input = bounded_input(Cursor::new(encoded));
    validate_input(&input);
    assert_eq!(input.companies.len(), 2);
}

#[test]
fn doop_lifecycle_input_accepts_exactly_the_existing_64k_byte_limit() {
    let mut encoded = boundary_input();
    encoded.resize(65536, b' ');
    let input: Input = bounded_input(Cursor::new(encoded));
    validate_input(&input);
    assert_eq!(input.companies.len(), 2);
}

#[test]
#[should_panic(expected = "64 KiB")]
fn doop_lifecycle_input_refuses_one_byte_over_before_database_or_network_work() {
    let mut encoded = boundary_input();
    encoded.resize(65537, b' ');
    let _: Input = bounded_input(Cursor::new(encoded));
}

#[test]
#[should_panic(expected = "unknown field")]
fn doop_lifecycle_input_refuses_undeclared_credential_selectors() {
    let mut value: Value = serde_json::from_slice(&boundary_input()).unwrap();
    value["credential_selector"] = json!("foreign");
    let _: Input = bounded_input(Cursor::new(serde_json::to_vec(&value).unwrap()));
}

#[test]
fn doop_lifecycle_envelope_uses_the_same_64k_reader_and_strict_base_validation() {
    let base: Value = serde_json::from_slice(&boundary_input()).unwrap();
    let mut encoded = serde_json::to_vec(
        &json!({"base":base,"retired_tokens":["c".repeat(8192),"d".repeat(8192)]}),
    )
    .unwrap();
    assert!(encoded.len() > 16384 && encoded.len() < 65536);
    encoded.resize(65536, b' ');
    let parsed = lifecycle_input(Cursor::new(encoded));
    assert_eq!(parsed.retired_tokens.len(), 2);
}

#[test]
#[should_panic(expected = "64 KiB")]
fn doop_lifecycle_envelope_refuses_one_byte_over_before_database_or_network_work() {
    let mut encoded = serde_json::to_vec(
        &json!({"base":serde_json::from_slice::<Value>(&boundary_input()).unwrap(),
        "retired_tokens":["retired-a","retired-b"]}),
    )
    .unwrap();
    encoded.resize(65537, b' ');
    lifecycle_input(Cursor::new(encoded));
}
