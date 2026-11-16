//! Bounded contracts for agent creation and explicit shared-desk provisioning.
use super::*;

pub fn provisioning_tool_schemas(kind: ProvisioningKind) -> (Value, Value) {
    let (input, success) = match kind {
        ProvisioningKind::ListAgents => (
            json!({"type":"object","properties":{
            "department":{"type":"string","minLength":1,"maxLength":64}},"additionalProperties":false}),
            listing(),
        ),
        ProvisioningKind::CreateAgent => (agent_input(), agent_result()),
        ProvisioningKind::CreateDeskForAgent => (desk_input(), desk_result()),
    };
    (input, json!({"oneOf":[success, error_result()]}))
}

fn listing() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agents": {"type": "array", "maxItems": MAX_LISTED_AGENTS, "items": {
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                    "name": {"type": "string", "maxLength": MAX_LISTED_NAME_CHARS},
                    "description": {"type": ["string", "null"],
                        "maxLength": MAX_LISTED_DESCRIPTION_CHARS},
                    "provider": {"type": ["string", "null"]},
                    "model": {"type": ["string", "null"]},
                    "departments": {"type": "array", "maxItems": MAX_LISTED_DEPARTMENTS,
                        "items": {
                            "type": "object",
                            "properties": {
                                "slug": {"type": "string"},
                                "name": {"type": "string", "maxLength": MAX_LISTED_NAME_CHARS},
                                "roles": {"type": "array", "items": {"enum": DepartmentRole::ALL
                                    .map(DepartmentRole::as_str)}}
                            },
                            "required": ["slug", "name", "roles"],
                            "additionalProperties": false
                        }},
                    "address": {"type": ["object", "null"],
                        "properties": {"principal_id": {"type": "string"}, "address": {"type": "string"}},
                        "required": ["principal_id", "address"], "additionalProperties": false}
                },
                "required": ["slug", "name", "description", "provider", "model", "departments",
                    "address"],
                "additionalProperties": false
            }},
            "models": {"type": "array", "maxItems": MAX_LISTED_MODELS, "items": {
                "type": "object",
                "properties": {"provider": {"type": "string"}, "model": {"type": "string"}},
                "required": ["provider", "model"],
                "additionalProperties": false
            }},
            // More agents or models exist than the listing holds.
            "truncated": {"type": "boolean"}
        },
        "required": ["agents", "models", "truncated"],
        "additionalProperties": false
    })
}

fn agent_input() -> Value {
    json!({"type":"object","properties":{
        "name":{"type":"string","minLength":1,"maxLength":120},
        "slug":{"type":"string","minLength":1,"maxLength":64},
        "description":{"type":"string","maxLength":500},
        "system_prompt":{"type":"string","minLength":1,"maxLength":32768},
        "provider":{"type":"string","minLength":1,"maxLength":64},
        "model":{"type":"string","minLength":1,"maxLength":128},
        "department":{"type":"string","minLength":1,"maxLength":64,
            "description":"Exact department slug. Omitted, joins the requester's sole department when unambiguous."},
        "role":{"enum":["lead"]},
        "tools":{"type":"array","maxItems":32,"items":{"type":"string","minLength":1,"maxLength":128}},
        "skills":{"type":"array","maxItems":16,"items":{"type":"string","minLength":1,"maxLength":128}}
    },"required":["name","slug","system_prompt","provider","model"],"additionalProperties":false})
}

fn agent_result() -> Value {
    json!({"type":"object","properties":{
        "agent":{"type":"object","properties":{"id":{"type":"string"},"slug":{"type":"string"}},
            "required":["id","slug"],"additionalProperties":false},
        "principal_id":{"type":"string"},"address":{"type":"string"},
        "workflow_id":{"type":"string"},"version_id":{"type":"string"},"binding_id":{"type":"string"}
    },"required":["agent","principal_id","address","workflow_id","version_id","binding_id"],"additionalProperties":false})
}

fn desk_input() -> Value {
    json!({"type":"object","properties":{
        "agent_slug":{"type":"string","minLength":1,"maxLength":64},
        "channel_slug":{"type":"string","minLength":1,"maxLength":64},
        "channel_name":{"type":"string","minLength":1,"maxLength":120},
        "reply_mode":{"enum":["autonomous","reviewed"]},
        "steps":{"type":"array","maxItems":MAX_CUSTOM_STEPS,"items":step_input()}
    },"required":["agent_slug","channel_slug","channel_name","reply_mode"],"additionalProperties":false})
}

fn step_input() -> Value {
    json!({"type":"object","properties":{
        "id":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z_][A-Za-z0-9_-]*$"},
        "slot":{"enum":["before_draft","before_reply","after_reply"]},
        "type":{"enum":BUILDER_STEP_TYPES},"with":{"type":"object"},
        "rule":{"type":"object"},"routes":{"type":"object"}
    },"required":["id","slot","type","with"],"additionalProperties":false})
}

fn desk_result() -> Value {
    json!({"type":"object","properties":{
        "agent":{"type":"object","properties":{"id":{"type":"string"},"slug":{"type":"string"}},
            "required":["id","slug"],"additionalProperties":false},
        "channel":{"type":"object","properties":{"id":{"type":"string"},"slug":{"type":"string"},"address":{"type":"string"}},
            "required":["id","slug","address"],"additionalProperties":false},
        "workflow_id":{"type":"string"},"version_id":{"type":"string"},"binding_id":{"type":"string"},
        "reply_mode":{"enum":["autonomous","reviewed"]}
    },"required":["agent","channel","workflow_id","version_id","binding_id","reply_mode"],"additionalProperties":false})
}

fn error_result() -> Value {
    json!({"type":"object","properties":{"error":{"type":"object","properties":{
        "code":{"enum":[PROVISIONING_FORBIDDEN,PROVISIONING_CONFLICT,PROVISIONING_INVALID]},
        "message":{"type":"string","maxLength":MAX_REFUSAL_MESSAGE}
    },"required":["code","message"],"additionalProperties":false}},"required":["error"],"additionalProperties":false})
}
