use super::{
    providers::{ProviderRegistry, ResolvedModelRequest},
    test_support::{KEYS, MODEL, Reply, check_request, model, response},
};
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use rig::{agent::AgentBuilder, completion::Prompt};
use secrecy::SecretString;
use serde_json::json;
use std::{future::IntoFuture, time::Duration};

#[tokio::test]
async fn every_provider_rejects_malformed_rate_limited_and_disconnected_exchanges_without_retry() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        for failure in ["malformed", "rate_limit", "disconnect"] {
            let mut reply = ScriptedResponse::json_body("{malformed fixture-secret".into());
            match failure {
                "rate_limit" => {
                    reply.status = 429;
                    reply.headers.push(("Retry-After".into(), "0".into()));
                }
                "disconnect" => reply.disconnect = true,
                _ => {}
            }
            let llm = scripted_scenario(vec![ScriptedExchange::new(
                move |request| check_request(request, provider, "fixture-secret"),
                reply,
            )])
            .await;
            let agent = AgentBuilder::from_model_handle(model(
                &registry,
                provider,
                "fixture-secret",
                &llm.base_url,
            ))
            .max_tokens(100)
            .build();
            let result = agent
                .prompt("Summarize the fixture record")
                .max_turns(1)
                .await;
            assert_eq!(llm.finish().await, Ok(1), "{provider}/{failure}");
            let error = result.unwrap_err();
            assert!(!format!("{error:?} {error}").contains("fixture-secret"));
        }
    }
}

#[tokio::test]
async fn every_provider_cancels_a_held_response_without_later_requests() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let (arrived, arrival) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let mut reply = ScriptedResponse::json(response(provider, Reply::Text));
        reply.barrier = Some((arrived, released));
        reply.disconnect = true;
        let llm = scripted_scenario(vec![ScriptedExchange::new(
            move |request| check_request(request, provider, "fixture-secret"),
            reply,
        )])
        .await;
        let agent = AgentBuilder::from_model_handle(model(
            &registry,
            provider,
            "fixture-secret",
            &llm.base_url,
        ))
        .max_tokens(100)
        .build();
        // Arrival, rather than elapsed wall time, makes cancellation happen during the request.
        let mut prompt = Box::pin(agent.prompt("Read the fixture").max_turns(1).into_future());
        tokio::select! {
            result = &mut prompt => panic!("provider returned before release: {result:?}"),
            arrival = tokio::time::timeout(Duration::from_secs(2), arrival) => arrival.unwrap().unwrap(),
        }
        drop(prompt);
        release.send(()).unwrap();
        assert_eq!(llm.finish().await, Ok(1), "{provider}");
    }
}

#[tokio::test]
async fn unregistered_model_destinations_fail_before_any_provider_connection() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        let handle = registry
            .model(&ResolvedModelRequest {
                provider: &provider.into(),
                model: &MODEL.into(),
                secret: &SecretString::from("fixture-secret"),
                endpoint: None,
            })
            .unwrap();
        let result = AgentBuilder::from_model_handle(handle)
            .max_tokens(100)
            .build()
            .prompt("No endpoint was registered")
            .await;
        assert!(result.is_err());
        assert!(
            format!("{:?}", result.unwrap_err())
                .contains("model fixture endpoint is not registered")
        );
    }
}

#[tokio::test]
async fn every_provider_decodes_missing_and_partial_usage_without_inventing_tokens() {
    let registry = ProviderRegistry::standard().unwrap();
    for provider in KEYS {
        for missing in [false, true] {
            let mut reply = response(provider, Reply::Text);
            match provider {
                "google" => reply["usageMetadata"] = json!({"promptTokenCount":2}),
                "anthropic" | "xai" => reply["usage"] = json!({"input_tokens":2}),
                _ => reply["usage"] = json!({"prompt_tokens":2}),
            }
            if missing {
                reply
                    .as_object_mut()
                    .unwrap()
                    .remove(if provider == "google" {
                        "usageMetadata"
                    } else {
                        "usage"
                    });
            }
            let llm = scripted_scenario(vec![ScriptedExchange::new(
                move |request| check_request(request, provider, "fixture-secret"),
                ScriptedResponse::json(reply),
            )])
            .await;
            let agent = AgentBuilder::from_model_handle(model(
                &registry,
                provider,
                "fixture-secret",
                &llm.base_url,
            ))
            .max_tokens(100)
            .build();
            let result = agent.prompt("Read the fixture").extended_details().await;
            assert_eq!(llm.finish().await, Ok(1));
            let output = result.unwrap();
            assert_eq!(output.output, "verified");
            assert_eq!(
                output.usage.input_tokens,
                if missing { 0 } else { 2 },
                "{provider}"
            );
            assert_eq!(output.usage.output_tokens, 0, "{provider}");
        }
    }
}
