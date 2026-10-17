//! Company HTTP resources and registered HTTP operations. Credentials never enter actions.
use super::{authorization::WorkflowActor, publication::ToolSnapshot};
use crate::{
    app_error::{AppError, AppResult},
    domain::workflow::{CompanyId, TypeName},
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;
#[path = "http_runtime.rs"]
mod runtime;
pub use runtime::*;

pub const MAX_HTTP_BODY: usize = 32_768;
pub const MAX_HTTP_RESPONSE: usize = 32_768;
pub const MAX_HTTP_HEADERS: usize = 32;
pub const MAX_HTTP_SECRET: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HttpOrigin(String);
impl HttpOrigin {
    pub fn parse(value: &str) -> AppResult<Self> {
        if value.len() > 2048 {
            return Err(invalid());
        }
        let url = url::Url::parse(value).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        Ok(Self(url.origin().ascii_serialization()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for HttpOrigin {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug)]
pub struct HttpConnection {
    pub company: CompanyId,
    pub id: Uuid,
    pub origin: HttpOrigin,
    pub enabled: bool,
    pub bearer: bool,
    pub secret_set: bool,
    pub revision: i64,
}

pub struct HttpConnectionWrite {
    pub origin: HttpOrigin,
    pub bearer: bool,
}

/// Policies are installed by authorized company administrators, never discovered remotely.
#[derive(Clone)]
pub struct HttpOperation {
    pub tool: ToolSnapshot,
    pub approval_required: bool,
}
impl HttpOperation {
    pub fn validate(&self, company: CompanyId) -> AppResult<()> {
        use super::{
            compiler::{Schema, SourceSpan},
            publication::ActionRecovery,
        };
        if self.tool.company_id != company
            || self.tool.contract.connection.is_some()
            || self.tool.policy.recovery != ActionRecovery::Reconcile
            || self.tool.policy.policy_revision == 0
            || serde_json::to_vec(&self.tool).map_err(|_| invalid())?.len() > 65_536
        {
            return Err(invalid());
        }
        let span = SourceSpan {
            start: 0,
            end: 0,
            line: 1,
            column: 1,
        };
        for schema in [
            &self.tool.contract.input_schema,
            &self.tool.contract.output_schema,
        ] {
            Schema::compile(schema.clone(), "/http-operation", span).map_err(|_| invalid())?;
        }
        Ok(())
    }
}

#[async_trait]
pub trait HttpConnections: Send + Sync {
    async fn create_http(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        write: HttpConnectionWrite,
    ) -> AppResult<HttpConnection>;
    async fn set_http_enabled(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        enabled: bool,
    ) -> AppResult<HttpConnection>;
    async fn replace_http_secret(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        secret: Option<SecretString>,
    ) -> AppResult<HttpConnection>;
    async fn register_http_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        operation: HttpOperation,
    ) -> AppResult<HttpConnection>;
    async fn revoke_http_operation(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: Uuid,
        revision: i64,
        name: TypeName,
    ) -> AppResult<HttpConnection>;
}

pub fn validate_secret(secret: &SecretString) -> AppResult<()> {
    let value = secret.expose_secret();
    if value.is_empty()
        || value.len() > MAX_HTTP_SECRET
        || !value.bytes().all(|b| (33..=126).contains(&b))
    {
        return Err(invalid());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}
impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}
pub struct HttpPath(String);
impl HttpPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for HttpPath {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let path = String::deserialize(decoder)?;
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.len() > 2048
            || path.contains(['\\', '#'])
            || path.contains("://")
            || path.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(serde::de::Error::custom("Invalid HTTP path"));
        }
        Ok(Self(path))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: HttpPath,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}
impl HttpRequest {
    pub fn parse(arguments: &Value) -> AppResult<Self> {
        if serde_json::to_vec(arguments).map_err(|_| invalid())?.len() > 65_536 {
            return Err(invalid());
        }
        let request: Self = serde_json::from_value(arguments.clone()).map_err(|_| invalid())?;
        request.validate()?;
        Ok(request)
    }
    fn validate(&self) -> AppResult<()> {
        if self.headers.len() > MAX_HTTP_HEADERS {
            return Err(invalid());
        }
        let mut bytes = 0;
        for (name, value) in &self.headers {
            bytes += name.len() + value.len();
            if !safe_request_header(name)
                || value.len() > 2048
                || value.bytes().any(|b| !(32..=126).contains(&b))
            {
                return Err(invalid());
            }
        }
        if bytes > 8192
            || self
                .body
                .as_ref()
                .map(serde_json::to_vec)
                .transpose()
                .map_err(|_| invalid())?
                .is_some_and(|v| v.len() > MAX_HTTP_BODY)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Deliberately small allowlist: extension headers cannot smuggle credentials or routing.
pub fn safe_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "accept" | "accept-language" | "content-type" | "if-match" | "if-none-match"
    )
}
pub fn invalid() -> AppError {
    AppError::BadRequest("Invalid HTTP action configuration or input".into())
}
