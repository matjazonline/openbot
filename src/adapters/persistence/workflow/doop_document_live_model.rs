//! Deterministic model protocol with actual MCP receipt IDs; no MCP results are substituted.
use super::*;
use crate::services::test_support::{
    LlmTurn, ScriptedExchange, ScriptedLlm, ScriptedRequest, ScriptedResponse,
    register_scripted_agent_base_url, scripted_scenario,
};

pub(super) fn before(index: usize) -> String {
    format!("Company {} draft before targeted edit", index + 1)
}
pub(super) fn after(index: usize) -> String {
    format!("Company {} actual targeted edit persisted", index + 1)
}

fn checked_request(request: &ScriptedRequest) -> Result<(), &'static str> {
    if request.method != "POST" || !request.body["messages"].is_array() {
        return Err("normal model request required");
    }
    let mut actual: Vec<_> = request.body["tools"]
        .as_array()
        .ok_or("effective tools required")?
        .iter()
        .map(|tool| {
            tool["function"]["name"]
                .as_str()
                .ok_or("tool name required")
        })
        .collect::<Result<_, _>>()?;
    actual.sort();
    let mut expected: Vec<_> = editor_tools().iter().map(|tool| tool.to_string()).collect();
    expected.sort();
    if actual != expected {
        return Err("exact five effective tools required");
    }
    Ok(())
}

fn has_receipt(request: &ScriptedRequest, needle: &str) -> bool {
    request.body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .any(|message| message.to_string().contains(needle))
}

fn response(
    request: &ScriptedRequest,
    observed: &client::LiveClient,
    company: usize,
    turn: usize,
) -> Result<ScriptedResponse, &'static str> {
    let effects = observed.company(company);
    let canvas = &effects
        .canvases
        .first()
        .ok_or("actual canvas receipt required")?
        .0;
    if !request.body.to_string().contains(canvas) {
        return Err("actual canvas missing from model context");
    }
    let reply = match turn {
        0 => first_turn(request, canvas)?,
        1 => {
            if !has_receipt(request, canvas) {
                return Err("actual canvas readback missing");
            }
            LlmTurn::tool_call(
                "create_frame",
                json!({"canvas_id":canvas,"name":format!("Draft {}",company+1),
                "html":format!("<!doctype html><html><body><h1>{}</h1></body></html>",before(company)),
                "width":800,"height":1000,"agent_name":"Openbot document editor"}),
            )
        }
        2 => {
            let frame = &effects
                .frame
                .as_ref()
                .ok_or("actual frame receipt required")?
                .0;
            if !has_receipt(request, frame) {
                return Err("actual frame receipt missing from model context");
            }
            LlmTurn::tool_call("get_frame", json!({"frame_id":frame}))
        }
        3 => {
            let frame = &effects.frame.as_ref().ok_or("actual frame required")?.0;
            if !effects
                .html
                .as_ref()
                .is_some_and(|html| html.contains(&before(company)))
                || !has_receipt(request, &before(company))
            {
                return Err("actual pre-edit HTML required");
            }
            LlmTurn::tool_call(
                "edit_frame_html",
                json!({"frame_id":frame,"old_str":before(company),
                "new_str":after(company),"agent_name":"Openbot document editor"}),
            )
        }
        4 => {
            // Identical reads in this execution reuse their immutable semantic receipt.
            // The mandatory external consumer performs the fresh post-edit HTML check.
            if effects.accepted_edits != 1 {
                return Err("actual accepted edit receipt required");
            }
            LlmTurn::text(
                json!({"canvas_id":canvas,"status":"draft",
                "summary":"Targeted edit accepted; visual review not performed."})
                .to_string(),
            )
        }
        _ => return Err("unexpected model turn"),
    };
    Ok(ScriptedResponse::turn(reply, turn))
}

fn first_turn(request: &ScriptedRequest, canvas: &str) -> Result<LlmTurn, &'static str> {
    let first = request.body["messages"].to_string();
    for guide in GUIDES {
        let encoded = serde_json::to_string(guide.trim()).unwrap();
        if !first.contains(&encoded[1..encoded.len() - 1]) {
            return Err("complete guide Skills required");
        }
    }
    Ok(LlmTurn::tool_call(
        "get_canvas",
        json!({"canvas_id":canvas}),
    ))
}

pub(super) async fn start(
    agent: Uuid,
    observed: Arc<client::LiveClient>,
    company: usize,
) -> ScriptedLlm {
    let exchanges = (0..5)
        .map(|turn| {
            let observed = observed.clone();
            ScriptedExchange::computed(checked_request, move |request| {
                let result = response(request, &observed, company, turn);
                if result.is_err() {
                    eprintln!("DOOP_E2E_MODEL_REJECTED company={company} turn={turn}");
                }
                result
            })
        })
        .collect();
    let listener = scripted_scenario(exchanges).await;
    register_scripted_agent_base_url(agent, &listener.base_url);
    listener
}
