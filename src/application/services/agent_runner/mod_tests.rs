//! What the runner does around a harness: composing, resolving one, and recording the result.
//! The Rig harness itself is exercised end to end by its own adapter tests.

use std::sync::Arc;

use uuid::Uuid;

use super::*;
use crate::entities::company::Company;
use crate::entities::harness::HarnessKind;
use crate::services::harness::HarnessRegistry;
use crate::services::test_support::StubHarness;
use params::ResolvedAgentCapabilities;

fn company() -> Company {
    Company {
        channel_defaults: Default::default(),
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        name: "Test".to_string(),
        slug: "test".into(),
        avatar_url: None,
        memory_provider: None,
        created_at: chrono::Utc::now(),
    }
}

fn registry_of(harness: Arc<dyn AgentHarness>) -> Arc<HarnessRegistry> {
    let kind = harness.kind();
    Arc::new(
        HarnessRegistry::new()
            .register(kind, harness)
            .expect("one harness registers"),
    )
}

/// The no-silent-downgrade rule, from the caller's side: a deployment that wired no harness must
/// stop the run rather than pick one.
#[tokio::test]
async fn a_run_with_no_configured_harness_fails_instead_of_choosing_one() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");

    let error = AgentRunner::new("Hello world", &params)
        .execute()
        .await
        .expect_err("a deployment with no harness cannot run an agent");

    assert!(
        error.to_string().contains("No agent harness is configured"),
        "unexpected error: {error}"
    );
    assert!(matches!(error, AppError::BadRequest(_)));
}

#[tokio::test]
async fn an_agent_with_no_registered_harness_fails_the_run() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");
    let registry = Arc::new(HarnessRegistry::new());

    let error = AgentRunner::new("Hello world", &params)
        .harnesses(registry.clone())
        .execute()
        .await
        .expect_err("an empty registry cannot execute the requested kind");

    assert!(error.to_string().contains(HarnessKind::Rig.as_str()));
    assert!(registry.registered().is_empty());
}

/// A run that reached a harness and failed there is recorded and reported -- with the company's
/// credential removed from the message on the way out.
#[tokio::test]
async fn a_failed_run_is_reported_without_the_credential_that_caused_it() {
    let company = company();
    let params = ResolvedAgentCapabilities::new(Some(&company), None).expect("params resolve");
    let harness = StubHarness::new(HarnessKind::Rig)
        .failing("provider rejected key company-api-key with 401");

    let error = AgentRunner::new("Hello world", &params)
        .harnesses(registry_of(Arc::new(harness)))
        .execute()
        .await
        .expect_err("the harness failed");

    assert!(!error.to_string().contains("company-api-key"), "{error}");
    assert!(error.to_string().contains("[REDACTED]"), "{error}");
    assert!(matches!(error, AppError::Internal(_)));
}
