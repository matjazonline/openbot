use super::providers::*;
use rig::tool::{DynamicTool, ToolOutput};
use secrecy::SecretString;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub(super) use crate::services::test_support::provider::{
    KEYS, MODEL, Reply, check_request, check_result, check_tools, response,
};

pub(super) fn model(
    registry: &ProviderRegistry,
    provider: &str,
    secret: &str,
    endpoint: &str,
) -> rig::agent::ModelHandle {
    registry
        .model(&ResolvedModelRequest {
            provider: &provider.into(),
            model: &MODEL.into(),
            secret: &SecretString::from(secret),
            endpoint: Some(endpoint),
        })
        .unwrap()
}
pub(super) fn lookup(calls: Arc<AtomicUsize>) -> DynamicTool {
    DynamicTool::new(
        "lookup",
        "Read a record",
        json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false}),
        move |_, args| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(args, json!({"key":"record"}));
            Box::pin(async { Ok(ToolOutput::text("record-value")) })
        },
    )
}
