//! Credential-explicit model factories; no prompts, tools, tenant cache, or environment lookup.
use super::http::ProviderHttp;
use std::{collections::HashMap, time::Duration};

use rig::{agent::ModelHandle, client::CompletionClient, providers};
use secrecy::{ExposeSecret, SecretString};

use crate::entities::value_objects::{ModelName, ModelProvider};

/// Only the server's resolved connection may supply this request. Deliberately not Debug.
pub struct ResolvedModelRequest<'a> {
    pub provider: &'a ModelProvider,
    pub model: &'a ModelName,
    pub secret: &'a SecretString,
    /// Trusted server configuration, never deserialized from agent JSON.
    pub endpoint: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("Unsupported Rig provider")]
    UnsupportedProvider,
    #[error("Rig provider factories do not cover the application provider catalogue")]
    IncompleteCoverage,
    #[error("Duplicate Rig provider registration")]
    DuplicateProvider,
    #[error("Model credentials are missing or invalid")]
    InvalidCredentials,
    #[error("Model name is missing")]
    MissingModel,
    #[error("Invalid trusted provider endpoint")]
    InvalidEndpoint,
    #[error("Unable to construct provider transport")]
    Transport,
    #[error("Unable to construct provider client")]
    Configuration,
}

pub type ProviderFactory =
    fn(&reqwest::Client, &ResolvedModelRequest<'_>) -> Result<ModelHandle, ProviderError>;

pub struct ProviderRegistry {
    factories: HashMap<ModelProvider, ProviderFactory>,
    transport: reqwest::Client,
}

impl ProviderRegistry {
    pub fn new() -> Result<Self, ProviderError> {
        let transport = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .pool_idle_timeout(Duration::from_secs(60))
            .pool_max_idle_per_host(8)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| ProviderError::Transport)?;
        Ok(Self {
            factories: HashMap::new(),
            transport,
        })
    }

    pub fn standard() -> Result<Self, ProviderError> {
        let registry = Self::new()?
            .register("google".into(), google)?
            .register("openai".into(), openai)?
            .register("anthropic".into(), anthropic)?
            .register("groq".into(), groq)?
            .register("xai".into(), xai)?;
        registry.validate_coverage()?;
        Ok(registry)
    }

    pub fn validate_coverage(&self) -> Result<(), ProviderError> {
        let expected = crate::application::model_providers::SUPPORTED_MODEL_PROVIDERS;
        if self.factories.len() != expected.len()
            || expected
                .iter()
                .any(|key| !self.factories.contains_key(*key))
        {
            return Err(ProviderError::IncompleteCoverage);
        }
        Ok(())
    }

    pub fn register(
        mut self,
        key: ModelProvider,
        factory: ProviderFactory,
    ) -> Result<Self, ProviderError> {
        if self.factories.contains_key(&key) {
            return Err(ProviderError::DuplicateProvider);
        }
        self.factories.insert(key, factory);
        Ok(self)
    }

    pub fn model(&self, request: &ResolvedModelRequest<'_>) -> Result<ModelHandle, ProviderError> {
        let factory = self
            .factories
            .get(request.provider)
            .ok_or(ProviderError::UnsupportedProvider)?;
        validate(request)?;
        factory(&self.transport, request)
    }
}

fn validate(request: &ResolvedModelRequest<'_>) -> Result<(), ProviderError> {
    let secret = request.secret.expose_secret();
    if secret.trim().is_empty() || secret.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ProviderError::InvalidCredentials);
    }
    if request.model.trim().is_empty() {
        return Err(ProviderError::MissingModel);
    }
    if let Some(endpoint) = request.endpoint {
        let url = url::Url::parse(endpoint).map_err(|_| ProviderError::InvalidEndpoint)?;
        let local = url
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "[::1]");
        if !(url.scheme() == "https" || cfg!(test) && local && url.scheme() == "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ProviderError::InvalidEndpoint);
        }
    }
    Ok(())
}

// Provider builders have distinct extension types. Keep that type-specific construction here.
macro_rules! factory {
    ($name:ident, $provider:ident $(, $conversion:ident)?) => {
        fn $name(transport: &reqwest::Client, request: &ResolvedModelRequest<'_>) -> Result<ModelHandle, ProviderError> {
            let mut builder = providers::$provider::Client::builder()
                .api_key(request.secret.expose_secret())
                .http_client(ProviderHttp::new(transport.clone()));
            if let Some(endpoint) = request.endpoint { builder = builder.base_url(endpoint.trim_end_matches('/')); }
            let client = builder.build().map_err(|_| ProviderError::Configuration)?;
            $(let client = client.$conversion();)?
            Ok(ModelHandle::new(client.completion_model(request.model.as_str())))
        }
    };
}

fn google(
    transport: &reqwest::Client,
    request: &ResolvedModelRequest<'_>,
) -> Result<ModelHandle, ProviderError> {
    let mut builder = providers::gemini::Client::builder()
        .api_key("adapter-managed")
        .http_client(ProviderHttp::google(transport.clone(), request.secret));
    if let Some(endpoint) = request.endpoint {
        builder = builder.base_url(endpoint.trim_end_matches('/'));
    }
    let client = builder.build().map_err(|_| ProviderError::Configuration)?;
    Ok(ModelHandle::new(
        client.completion_model(request.model.as_str()),
    ))
}
factory!(openai, openai, completions_api);
factory!(anthropic, anthropic);
factory!(groq, groq);
factory!(xai, xai);

/// Rig 0.42 also emits raw wire JSON and provider error text outside its content-telemetry flag.
/// Suppress that instrumentation at each poll, including conversion of the wire response. Our
/// diagnostics count the normalized usage; the surrounding application trace keeps its IDs.
pub(super) async fn complete(
    model: &ModelHandle,
    mut request: rig::completion::CompletionRequest,
) -> crate::app_error::AppResult<rig::completion::CompletionResponse> {
    use rig::completion::CompletionModel;
    use tracing::instrument::WithSubscriber;
    request.record_telemetry_content = false;
    model
        .completion(request)
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await
        .map_err(completion_error)
}

fn completion_error(error: rig::completion::CompletionError) -> crate::app_error::AppError {
    use rig::{completion::CompletionError, http_client::Error};
    if matches!(&error, CompletionError::HttpError(Error::Instance(source))
        if source.is::<super::http::ProviderTimeout>())
    {
        crate::app_error::AppError::Timeout("Rig provider request timed out".into())
    } else {
        crate::app_error::AppError::Internal("Rig provider request failed".into())
    }
}

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
    use rig::completion::{CompletionRequest, Message};

    #[tokio::test]
    async fn a_stalled_provider_returns_a_typed_timeout_without_url_or_credentials() {
        let (arrived, arrival) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let mut reply = ScriptedResponse::json(serde_json::Value::Null);
        reply.barrier = Some((arrived, released));
        reply.disconnect = true;
        let llm = scripted_scenario(vec![ScriptedExchange::new(|_| Ok(()), reply)]).await;
        let transport = reqwest::Client::builder()
            .timeout(Duration::from_millis(25))
            .no_proxy()
            .build()
            .unwrap();
        let model = openai(
            &transport,
            &ResolvedModelRequest {
                provider: &"openai".into(),
                model: &"fixture".into(),
                secret: &SecretString::from("private-timeout-key"),
                endpoint: Some(&llm.base_url),
            },
        )
        .unwrap();
        let request = CompletionRequest {
            model: None,
            preamble: None,
            chat_history: vec![Message::user("private-prompt")],
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: Some(10),
            tool_choice: None,
            additional_params: None,
            output_schema: None,
            record_telemetry_content: false,
        };
        // Bound the regression even if the transport accidentally loses its own deadline.
        let result = tokio::time::timeout(Duration::from_secs(2), complete(&model, request)).await;
        tokio::time::timeout(Duration::from_secs(2), arrival)
            .await
            .unwrap()
            .unwrap();
        release.send(()).unwrap();
        assert_eq!(llm.finish().await, Ok(1));
        let error = result.unwrap().unwrap_err();
        assert!(matches!(error, crate::app_error::AppError::Timeout(_)));
        assert!(!error.to_string().contains("private-"));
        assert!(!error.to_string().contains("127.0.0.1"));
    }
}
