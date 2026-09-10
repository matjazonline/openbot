use super::*;
use crate::entities::harness::HarnessKind;

#[test]
fn bootstrap_resolves_absent_and_explicit_harness_environment_in_isolated_processes() {
    const CHILD: &str = "RIG_BOOTSTRAP_EXPECTED";
    if let Ok(expected) = std::env::var(CHILD) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let config = AppConfig::from_env();
            assert_eq!(config.default_agent_harness.as_str(), expected);
            verify_registry(config.default_agent_harness);
        });
        return;
    }
    for configured in [None, Some("ai_agents")] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.env_clear()
            .env("JWT_SECRET", "synthetic-bootstrap-secret-at-least-32-characters")
            .env(CHILD, configured.unwrap_or("rig"))
            .env("RUST_MIN_STACK", "2097152")
            .arg("--exact")
            .arg("infra::config::bootstrap_tests::bootstrap_resolves_absent_and_explicit_harness_environment_in_isolated_processes");
        if let Some(value) = configured {
            child.env("DEFAULT_AGENT_HARNESS", value);
        }
        let output = child.output().unwrap();
        assert!(
            output.status.success(),
            "bootstrap: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}

fn verify_registry(default: HarnessKind) {
    use crate::adapters::{
        harness::deployment_registry,
        mcp::{HttpMcpClient, policy::EndpointPolicy},
        persistence::PostgresPersistence,
    };
    use std::sync::Arc;
    // Bootstrap constructs adapters without probing a provider or database. Execution is
    // covered by the database-backed runner contract; this pool must never be connected.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://fixture@127.0.0.1:1/bootstrap_test")
        .unwrap();
    let persistence = Arc::new(PostgresPersistence::new(pool));
    let mcp = Arc::new(crate::services::mcp_runtime::McpRuntime::new(
        persistence.clone(),
        persistence.clone(),
        Arc::new(HttpMcpClient::new(
            EndpointPolicy::new(std::iter::empty::<String>()).unwrap(),
        )),
    ));
    let registry = deployment_registry(default, persistence, mcp).unwrap();
    assert_eq!(registry.require(default).unwrap().kind(), default);
    for explicit in HarnessKind::ALL {
        assert_eq!(registry.require(explicit).unwrap().kind(), explicit);
    }
}
