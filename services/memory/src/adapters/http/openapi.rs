//! OpenAPI 3.1 assembled from the registry and the schemars types (D6). The committed
//! `openapi.json` must equal `memory-service openapi`; `memory_contract_openapi_is_current`
//! enforces it.
use crate::{
    application::operations::{Method, OPERATIONS, Surface},
    domain::error::ErrorBody,
};
use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

pub fn document() -> Value {
    let mut generator = SchemaSettings::draft2020_12()
        .with(|settings| settings.definitions_path = "/components/schemas/".into())
        .into_generator();
    let error = generator.subschema_for::<ErrorBody>().to_value();
    let mut paths = Map::new();
    for spec in OPERATIONS {
        let response = (spec.response)(&mut generator).to_value();
        let mut operation = json!({
            "operationId": spec.id,
            "summary": spec.summary,
            "tags": [match spec.surface { Surface::Client => "client", Surface::Admin => "admin" }],
            "security": [{"bearer": []}],
            "x-memory-implemented": spec.implemented,
            "parameters": [correlation_parameter()],
            "responses": {
                "200": {"description": "Success", "headers": request_id_header(),
                        "content": {"application/json": {"schema": response}}},
                "default": {"description": "Error", "headers": request_id_header(),
                            "content": {"application/json": {"schema": error}}}
            }
        });
        if let Some(tool) = spec.tool {
            operation["x-mcp-tool"] = json!(tool);
        }
        let method = match spec.method {
            Method::Get => "get",
            Method::Post => {
                let request = (spec.request)(&mut generator).to_value();
                operation["requestBody"] = json!({
                    "required": true,
                    "content": {"application/json": {"schema": request}}
                });
                "post"
            }
        };
        paths.insert(spec.path.to_owned(), json!({ method: operation }));
    }
    for (path, summary) in [
        ("/healthz", "Liveness"),
        (
            "/readyz",
            "Readiness: database reachable and not shutting down",
        ),
    ] {
        paths.insert(
            path.to_owned(),
            json!({"get": {"operationId": path.trim_start_matches('/'), "summary": summary,
                "responses": {"200": {"description": "OK"},
                    "default": {"description": "Error", "content": {"application/json": {"schema": error}}}}}}),
        );
    }
    paths.insert(
        "/admin/v1/metrics".to_owned(),
        json!({"get": {"operationId": "admin.metrics", "tags": ["admin"],
            "security": [{"bearer": []}],
            "summary": "Operator-only Prometheus text: per-process counters and database-wide gauges",
            "responses": {
                "200": {"description": "Prometheus text format 0.0.4", "headers": request_id_header(),
                        "content": {"text/plain": {"schema": {"type": "string"}}}},
                "default": {"description": "Error", "headers": request_id_header(),
                            "content": {"application/json": {"schema": error}}}}}}),
    );
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Memory service",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Tenant- and namespace-scoped memory with ordered 0-9 access levels. \
                Operations marked x-memory-implemented: false are declared but return 501 not_implemented."
        },
        "paths": paths,
        "components": {
            "schemas": generator.take_definitions(true),
            "securitySchemes": {"bearer": {"type": "http", "scheme": "bearer"}}
        }
    })
}

/// The optional client correlation id every operation accepts (06A A2-2).
fn correlation_parameter() -> Value {
    json!({"name": "x-correlation-id", "in": "header", "required": false,
        "description": "Opaque, non-secret correlation id recorded in logs and audit; a malformed value is 400 invalid_request. Never authority.",
        "schema": {"type": "string", "pattern": "^[A-Za-z0-9._:-]{1,128}$"}})
}

/// The service-minted id every answer carries (06A A2-2).
fn request_id_header() -> Value {
    json!({"x-request-id": {"description": "The service-minted request id (also in error bodies and audit)",
        "schema": {"type": "string", "format": "uuid"}}})
}

pub fn render() -> String {
    let mut text = serde_json::to_string_pretty(&document()).expect("OpenAPI serializes");
    text.push('\n');
    text
}
