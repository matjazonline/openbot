//! Replays genuine disposable Doop responses through the production guarded transport.
//! Provenance and synthetic-only acceptance limits live alongside the fixtures.
use super::*;
use crate::services::mcp_client::McpClient;
use crate::services::test_support::{ScriptedExchange, ScriptedResponse, scripted_scenario};
use serde_json::{Value, json};

const KEY: &str = "doop-contract-protected-key";

fn fixture(name: &str) -> Value {
    let source = match name {
        "initialize" => include_str!("fixtures/doop/initialize.json"),
        "tools" => include_str!("fixtures/doop/tools.json"),
        "profile" => include_str!("fixtures/doop/profile.json"),
        "create" => include_str!("fixtures/doop/create-canvas.json"),
        "canvas" => include_str!("fixtures/doop/get-canvas.json"),
        "denied" => include_str!("fixtures/doop/foreign-canvas-error.json"),
        "screenshot" => include_str!("fixtures/doop/screenshot.json"),
        "pdf" => include_str!("fixtures/doop/pdf.json"),
        "private-screenshot" => include_str!("fixtures/doop/private-screenshot.json"),
        "private-png" => include_str!("fixtures/doop/private-png.json"),
        "private-jpg" => include_str!("fixtures/doop/private-jpg.json"),
        "private-pdf" => include_str!("fixtures/doop/private-pdf.json"),
        "private-pdf-small" => include_str!("fixtures/doop/private-pdf-small.json"),
        _ => panic!("unknown fixture"),
    };
    serde_json::from_str(source).unwrap()
}

#[tokio::test]
async fn doop_private_export_actual_results_survive_guarded_transport_without_fetching_urls() {
    let provenance: Value = serde_json::from_str(include_str!(
        "fixtures/doop/private-exports-provenance.json"
    ))
    .unwrap();
    for name in ["screenshot", "png", "jpg", "pdf", "pdf-small"] {
        let call = &provenance["calls"][name];
        let tool = call["tool"].as_str().unwrap();
        let arguments = call["arguments"].clone();
        let captured = fixture(&format!("private-{name}"));
        let mut exchanges = initialization();
        exchanges.push(exchange(
            "tools/call",
            Some(1),
            json!({"name":tool, "arguments":arguments}),
            captured.clone(),
        ));
        let server = scripted_scenario(exchanges).await;
        let adapter = client(&server.base_url);
        let session = adapter
            .connect(
                &server.base_url.clone().try_into().unwrap(),
                Some(KEY.into()),
            )
            .await
            .unwrap();
        let result = session
            .call(&tool.to_owned().try_into().unwrap(), arguments)
            .await
            .unwrap();
        assert!(!result.is_error, "{name}");
        assert_eq!(
            result.content,
            captured["content"].as_array().unwrap().clone()
        );
        assert_eq!(result.envelope()["isError"], false);
        assert!(!result.envelope().to_string().contains(KEY));
        session.close().await.unwrap();
        adapter.shutdown().await.unwrap();
        assert_eq!(server.finish().await, Ok(3), "download URLs remain inert");
    }
}

fn exchange(
    method: &'static str,
    id: Option<u64>,
    params: Value,
    result: Value,
) -> ScriptedExchange {
    let response = match id {
        Some(id) => ScriptedResponse::json(json!({"jsonrpc":"2.0", "id":id, "result":result})),
        None => {
            let mut response = ScriptedResponse::json_body(String::new());
            response.status = 202;
            response
        }
    };
    ScriptedExchange::new(
        move |request| {
            if request.method != "POST"
                || request.path != "/"
                || request.body["method"] != method
                || !request.header_matches("authorization", &format!("Bearer {KEY}"))
            {
                return Err("Doop MCP guarded request mismatch");
            }
            if id.is_some_and(|id| request.body["id"] != id) {
                return Err("Doop MCP request identity mismatch");
            }
            // rmcp adds its own _meta.progressToken to cancellable calls. Match the
            // complete business arguments and tool name, independently of that metadata.
            if !params.is_null()
                && (request.body["params"]["name"] != params["name"]
                    || request.body["params"]["arguments"] != params["arguments"])
            {
                return Err("Doop MCP operation/arguments mismatch");
            }
            Ok(())
        },
        response,
    )
}

fn initialization() -> Vec<ScriptedExchange> {
    vec![
        exchange("initialize", Some(0), Value::Null, fixture("initialize")),
        exchange("notifications/initialized", None, Value::Null, Value::Null),
    ]
}

fn client(url: &str) -> HttpMcpClient {
    HttpMcpClient::new(EndpointPolicy::new([format!("{}/", url.trim_end_matches('/'))]).unwrap())
}

#[tokio::test]
async fn doop_contract_actual_catalog_compiles_with_existing_limits_and_fixed_workspace_arguments()
{
    let mut exchanges = initialization();
    exchanges.push(exchange(
        "tools/list",
        Some(1),
        Value::Null,
        fixture("tools"),
    ));
    let server = scripted_scenario(exchanges).await;
    let adapter = client(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(KEY.into()),
        )
        .await
        .unwrap();
    let tools = session.discover().await.unwrap();
    assert_eq!(tools.len(), 48);
    assert!(tools.len() <= MAX_MCP_DISCOVERED_TOOLS);
    let create = tools
        .iter()
        .find(|tool| tool.name.as_str() == "create_canvas")
        .unwrap();
    let validator = crate::services::tool_schema::compile(&create.input_schema).unwrap();
    assert!(validator.is_valid(&json!({"name":"Synthetic", "workspace_id":"workspace-a"})));
    assert!(!validator.is_valid(&json!({"name":"Synthetic", "workspace_id":42})));
    assert!(!validator.is_valid(&json!({"workspace_id":"workspace-a"})));
    // The schema accepts omission; server policy and the trusted workflow enforce placement.
    assert!(validator.is_valid(&json!({"name":"Synthetic"})));
    let profile = tools
        .iter()
        .find(|tool| tool.name.as_str() == "get_profile")
        .unwrap();
    assert_eq!(
        fixture("profile")["structuredContent"]["managed_workspace_id"],
        "workspace-a"
    );
    assert_eq!(
        profile.output_schema.as_ref().unwrap()["properties"]["managed_workspace_id"]["type"],
        "string"
    );
    assert!(
        crate::services::tool_schema::compile(profile.output_schema.as_ref().unwrap())
            .unwrap()
            .is_valid(&fixture("profile")["structuredContent"])
    );
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
}

#[tokio::test]
async fn doop_contract_actual_results_preserve_structure_images_resources_and_error_status() {
    for (name, tool, arguments, expected_error) in [
        ("profile", "get_profile", json!({}), false),
        (
            "create",
            "create_canvas",
            json!({"name":"Synthetic contract", "workspace_id":"workspace-a"}),
            false,
        ),
        (
            "denied",
            "get_canvas",
            json!({"canvas_id":"canvas-a"}),
            true,
        ),
        (
            "screenshot",
            "get_frame_screenshot",
            json!({"frame_id":"frame-a", "scale":1}),
            false,
        ),
        (
            "pdf",
            "export_frame",
            json!({"frame_id":"frame-a", "format":"pdf"}),
            false,
        ),
    ] {
        let mut exchanges = initialization();
        exchanges.push(exchange(
            "tools/call",
            Some(1),
            json!({"name":tool, "arguments":arguments}),
            fixture(name),
        ));
        let server = scripted_scenario(exchanges).await;
        let adapter = client(&server.base_url);
        let session = adapter
            .connect(
                &server.base_url.clone().try_into().unwrap(),
                Some(KEY.into()),
            )
            .await
            .unwrap();
        let result = session
            .call(&tool.to_owned().try_into().unwrap(), arguments)
            .await
            .unwrap();
        assert_eq!(result.is_error, expected_error, "{name}");
        assert_eq!(
            result.content,
            fixture(name)["content"].as_array().unwrap().clone(),
            "{name}"
        );
        if matches!(name, "profile" | "create") {
            assert_eq!(
                result.structured_content,
                Some(fixture(name)["structuredContent"].clone())
            );
        }
        assert!(!result.envelope().to_string().contains(KEY));
        session.close().await.unwrap();
        adapter.shutdown().await.unwrap();
        // Embedded resources remain data: no automatic fetch or business replay occurs.
        assert_eq!(server.finish().await, Ok(3));
    }
}

#[test]
fn doop_catalog_real_minimal_operations_and_identity_results_pass_workflow_contracts() {
    use crate::application::agent::ActionEffect;
    use crate::application::workflow::mcp::McpOperation;
    use crate::domain::workflow::CompanyId;

    let catalog = fixture("tools");
    let company = CompanyId::new(uuid::Uuid::new_v4());
    for (name, effect) in [
        ("get_guide", ActionEffect::Read),
        ("get_profile", ActionEffect::Read),
        ("list_canvases", ActionEffect::Read),
        ("create_canvas", ActionEffect::Write),
        ("get_canvas", ActionEffect::Read),
        ("get_frame", ActionEffect::Read),
        ("create_frame", ActionEffect::Write),
        ("edit_frame_html", ActionEffect::Write),
        ("get_frame_screenshot", ActionEffect::Read),
    ] {
        let raw = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        let tool = crate::entities::mcp::McpDiscoveredTool {
            name: name.to_owned().try_into().unwrap(),
            description: raw["description"].as_str().unwrap_or_default().to_owned(),
            input_schema: raw["inputSchema"].clone(),
            output_schema: raw.get("outputSchema").cloned(),
        };
        let reviewed =
            McpOperation::review(company, &tool, effect, effect == ActionEffect::Write, 1)
                .unwrap_or_else(|error| panic!("actual {name} workflow contract: {error}"));
        assert_eq!(reviewed.tool.contract.input_schema, tool.input_schema);
        if let Some(captured) = match name {
            "get_profile" => Some("profile"),
            "create_canvas" => Some("create"),
            "get_canvas" => Some("canvas"),
            _ => None,
        } {
            let validator =
                crate::services::tool_schema::compile(&reviewed.tool.contract.output_schema)
                    .unwrap();
            let result = fixture(captured);
            assert!(
                validator.is_valid(&result["structuredContent"]),
                "actual {name} declared structured output"
            );
            if name != "get_profile" {
                assert_eq!(result["structuredContent"]["workspace_id"], "workspace-a");
                let mut invalid = result;
                invalid["structuredContent"]["workspace_id"] = json!(42);
                assert!(
                    !validator.is_valid(&invalid["structuredContent"]),
                    "wrong workspace field type must fail"
                );
            }
        }
    }
}

#[tokio::test]
async fn doop_contract_oversized_pdf_is_rejected_before_unbounded_result_parsing() {
    let mut pdf = fixture("pdf");
    pdf["content"][0]["resource"]["blob"] = json!("x".repeat(MAX_MCP_DISCOVERY_BYTES + 1));
    let arguments = json!({"frame_id":"frame-a", "format":"pdf"});
    let mut exchanges = initialization();
    exchanges.push(exchange(
        "tools/call",
        Some(1),
        json!({"name":"export_frame", "arguments":arguments}),
        pdf,
    ));
    let server = scripted_scenario(exchanges).await;
    let adapter = client(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(KEY.into()),
        )
        .await
        .unwrap();
    let error = session
        .call(&"export_frame".to_owned().try_into().unwrap(), arguments)
        .await
        .unwrap_err();
    assert!(!error.to_string().contains(KEY));
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
}

#[tokio::test]
async fn doop_contract_profile_secret_echo_is_redacted_in_both_result_representations() {
    let mut profile = fixture("profile");
    profile["structuredContent"]["name"] = json!(KEY);
    profile["content"][0]["text"] = json!(format!("protected {KEY}"));
    let mut exchanges = initialization();
    exchanges.push(exchange(
        "tools/call",
        Some(1),
        json!({"name":"get_profile", "arguments":{}}),
        profile,
    ));
    let server = scripted_scenario(exchanges).await;
    let adapter = client(&server.base_url);
    let session = adapter
        .connect(
            &server.base_url.clone().try_into().unwrap(),
            Some(KEY.into()),
        )
        .await
        .unwrap();
    let result = session
        .call(&"get_profile".to_owned().try_into().unwrap(), json!({}))
        .await
        .unwrap();
    assert!(!result.envelope().to_string().contains(KEY));
    assert!(result.structured_content.is_some());
    session.close().await.unwrap();
    adapter.shutdown().await.unwrap();
    assert_eq!(server.finish().await, Ok(3));
}
