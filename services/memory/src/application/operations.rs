//! The operation registry: one table generates the HTTP routes, the MCP tools and the OpenAPI
//! document, so every client use case has identical behaviour on both transports (D6).
use super::contract::*;
use crate::domain::grant::Operation;
use schemars::{JsonSchema, Schema, SchemaGenerator};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Client credentials; HTTP under `/v1` and an MCP tool.
    Client,
    /// Admin credentials; HTTP under `/admin/v1` only.
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyLimit {
    /// 256 KiB.
    Standard,
    /// 4 MiB: saves, source records and generation requests.
    Large,
}

impl BodyLimit {
    pub const fn bytes(self) -> usize {
        match self {
            Self::Standard => 256 * 1024,
            Self::Large => 4 * 1024 * 1024,
        }
    }
}

pub struct OperationSpec {
    /// Stable identifier, e.g. `observations.save`.
    pub id: &'static str,
    pub surface: Surface,
    pub method: Method,
    pub path: &'static str,
    /// The MCP tool name (client surface only).
    pub tool: Option<&'static str>,
    /// The grant operation it requires (client surface only).
    pub grant: Option<Operation>,
    pub summary: &'static str,
    pub body_limit: BodyLimit,
    /// False: the operation is declared with its schemas but returns `not_implemented`.
    pub implemented: bool,
    pub request: fn(&mut SchemaGenerator) -> Schema,
    pub response: fn(&mut SchemaGenerator) -> Schema,
    pub request_root: fn() -> Schema,
    /// Strict decode of a request body into its declared type.
    pub parse: fn(&serde_json::Value) -> Result<(), String>,
}

fn schema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<T>()
}

fn parse<T: serde::de::DeserializeOwned>(input: &serde_json::Value) -> Result<(), String> {
    T::deserialize(input)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn root<T: JsonSchema>() -> Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
}

macro_rules! op {
    ($id:literal, $surface:ident, $method:ident, $path:literal, $tool:expr, $grant:expr,
     $limit:ident, $implemented:literal, $req:ty, $resp:ty, $summary:literal) => {
        OperationSpec {
            id: $id,
            surface: Surface::$surface,
            method: Method::$method,
            path: $path,
            tool: $tool,
            grant: $grant,
            summary: $summary,
            body_limit: BodyLimit::$limit,
            implemented: $implemented,
            request: schema::<$req>,
            response: schema::<$resp>,
            request_root: root::<$req>,
            parse: parse::<$req>,
        }
    };
}

use Operation as G;

pub static OPERATIONS: &[OperationSpec] = &[
    op!(
        "credential.get",
        Client,
        Get,
        "/v1/credential",
        Some("memory_credential"),
        None,
        Standard,
        true,
        Empty,
        CredentialView,
        "The calling credential's tenant, active namespace grants and expiry"
    ),
    op!(
        "observations.save",
        Client,
        Post,
        "/v1/observations/save",
        Some("memory_save"),
        Some(G::Save),
        Large,
        true,
        SaveRequest,
        SaveResponse,
        "Save explicit observations at their derived access level"
    ),
    op!(
        "observations.search",
        Client,
        Post,
        "/v1/observations/search",
        Some("memory_search"),
        Some(G::Search),
        Standard,
        true,
        SearchRequest,
        SearchResponse,
        "Hybrid keyword and semantic search returning a compact index"
    ),
    op!(
        "observations.timeline",
        Client,
        Post,
        "/v1/observations/timeline",
        Some("memory_timeline"),
        Some(G::Timeline),
        Standard,
        true,
        TimelineRequest,
        TimelineResponse,
        "A bounded authorized neighbourhood around one observation"
    ),
    op!(
        "observations.fetch",
        Client,
        Post,
        "/v1/observations/fetch",
        Some("memory_fetch"),
        Some(G::Fetch),
        Standard,
        true,
        FetchRequest,
        FetchResponse,
        "Selected observation details, re-checking every id"
    ),
    op!(
        "observations.correct",
        Client,
        Post,
        "/v1/observations/correct",
        Some("memory_correct"),
        Some(G::Correct),
        Standard,
        true,
        CorrectRequest,
        SavedObservation,
        "Correct an observation at an expected revision"
    ),
    op!(
        "observations.delete",
        Client,
        Post,
        "/v1/observations/delete",
        Some("memory_delete"),
        Some(G::Delete),
        Standard,
        true,
        DeleteRequest,
        DeleteResponse,
        "Delete an observation and its derived parts"
    ),
    op!(
        "context.pack",
        Client,
        Post,
        "/v1/context/pack",
        Some("memory_context_pack"),
        Some(G::Context),
        Standard,
        true,
        ContextPackRequest,
        ContextPack,
        "A token- and byte-bounded context pack"
    ),
    op!(
        "context.validate",
        Client,
        Post,
        "/v1/context/validate",
        Some("memory_context_validate"),
        Some(G::Context),
        Standard,
        true,
        ContextValidateRequest,
        ContextValidateResponse,
        "Check whether frozen pack items are still valid and authorized"
    ),
    op!(
        "sources.record",
        Client,
        Post,
        "/v1/sources/record",
        Some("memory_record_sources"),
        Some(G::Sources),
        Large,
        true,
        SourcesRecordRequest,
        SourcesRecordResponse,
        "Record immutable source revisions as evidence"
    ),
    op!(
        "sessions.open",
        Client,
        Post,
        "/v1/sessions/open",
        Some("memory_session_open"),
        Some(G::Sessions),
        Standard,
        true,
        SessionOpenRequest,
        SessionView,
        "Open a session boundary"
    ),
    op!(
        "sessions.close",
        Client,
        Post,
        "/v1/sessions/close",
        Some("memory_session_close"),
        Some(G::Sessions),
        Standard,
        true,
        SessionCloseRequest,
        SessionView,
        "Close a session boundary"
    ),
    op!(
        "generations.request",
        Client,
        Post,
        "/v1/generations/request",
        Some("memory_generation_request"),
        Some(G::Generate),
        Large,
        true,
        GenerationRequest,
        GenerationStatus,
        "Request generation over a complete bounded window"
    ),
    op!(
        "generations.status",
        Client,
        Post,
        "/v1/generations/status",
        Some("memory_generation_status"),
        Some(G::GenerationStatus),
        Standard,
        true,
        GenerationRef,
        GenerationStatus,
        "Generation operation status"
    ),
    op!(
        "generations.cancel",
        Client,
        Post,
        "/v1/generations/cancel",
        Some("memory_generation_cancel"),
        Some(G::Generate),
        Standard,
        true,
        GenerationCancelRequest,
        GenerationStatus,
        "Cancel a generation operation"
    ),
    op!(
        "namespaces.purge",
        Client,
        Post,
        "/v1/namespaces/purge",
        Some("memory_purge_namespace"),
        Some(G::Purge),
        Standard,
        true,
        PurgeRequest,
        PurgeResponse,
        "Delete every memory in a namespace and tombstone it"
    ),
    op!(
        "admin.tenants.create",
        Admin,
        Post,
        "/admin/v1/tenants/create",
        None,
        None,
        Standard,
        true,
        TenantCreateRequest,
        TenantView,
        "Create a tenant managed by the caller"
    ),
    op!(
        "admin.tenants.disable",
        Admin,
        Post,
        "/admin/v1/tenants/disable",
        None,
        None,
        Standard,
        true,
        TenantRef,
        TenantView,
        "Disable a tenant and cancel its queued work"
    ),
    op!(
        "admin.tenants.enable",
        Admin,
        Post,
        "/admin/v1/tenants/enable",
        None,
        None,
        Standard,
        true,
        TenantRef,
        TenantView,
        "Enable a disabled tenant; never revives cancelled work"
    ),
    op!(
        "admin.tenants.delete",
        Admin,
        Post,
        "/admin/v1/tenants/delete",
        None,
        None,
        Standard,
        true,
        TenantRef,
        TenantView,
        "Tombstone a tenant permanently"
    ),
    op!(
        "admin.tenants.settings",
        Admin,
        Post,
        "/admin/v1/tenants/settings",
        None,
        None,
        Standard,
        true,
        TenantSettingsRequest,
        TenantView,
        "Operator-only tenant settings"
    ),
    op!(
        "admin.tenants.reassign",
        Admin,
        Post,
        "/admin/v1/tenants/reassign",
        None,
        None,
        Standard,
        true,
        TenantReassignRequest,
        TenantReassignResponse,
        "Operator-only: move tenants to another provisioner's management"
    ),
    op!(
        "admin.namespaces.create",
        Admin,
        Post,
        "/admin/v1/namespaces/create",
        None,
        None,
        Standard,
        true,
        NamespaceRequest,
        NamespaceView,
        "Create a namespace"
    ),
    op!(
        "admin.namespaces.delete",
        Admin,
        Post,
        "/admin/v1/namespaces/delete",
        None,
        None,
        Standard,
        true,
        NamespaceRequest,
        NamespaceView,
        "Tombstone a namespace and remove its grants"
    ),
    op!(
        "admin.credentials.issue",
        Admin,
        Post,
        "/admin/v1/credentials/issue",
        None,
        None,
        Standard,
        true,
        CredentialIssueRequest,
        IssuedCredential,
        "Issue a client credential with namespace grants"
    ),
    op!(
        "admin.credentials.list",
        Admin,
        Post,
        "/admin/v1/credentials/list",
        None,
        None,
        Standard,
        true,
        CredentialListRequest,
        CredentialList,
        "List a tenant's client credentials, one page in id order"
    ),
    op!(
        "admin.credentials.revoke",
        Admin,
        Post,
        "/admin/v1/credentials/revoke",
        None,
        None,
        Standard,
        true,
        CredentialRevokeRequest,
        CredentialView,
        "Revoke a client credential and cancel work it authorized"
    ),
    op!(
        "admin.admin_credentials.issue",
        Admin,
        Post,
        "/admin/v1/admin-credentials/issue",
        None,
        None,
        Standard,
        true,
        AdminCredentialIssueRequest,
        IssuedCredential,
        "Operator-only: issue an operator or provisioner credential"
    ),
    op!(
        "admin.admin_credentials.revoke",
        Admin,
        Post,
        "/admin/v1/admin-credentials/revoke",
        None,
        None,
        Standard,
        true,
        AdminCredentialRevokeRequest,
        AdminCredentialView,
        "Operator-only: revoke an admin credential"
    ),
];

pub fn by_id(id: &str) -> Option<&'static OperationSpec> {
    OPERATIONS.iter().find(|spec| spec.id == id)
}

pub fn by_tool(tool: &str) -> Option<&'static OperationSpec> {
    OPERATIONS.iter().find(|spec| spec.tool == Some(tool))
}
