use super::MAX_WORKFLOW_TEXT_BYTES;
use serde_json::{Value, json};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}
fn external() -> Value {
    json!({"type":"string","pattern":"^[A-Za-z0-9_.:-]{1,128}$"})
}

pub fn input_schema() -> Value {
    object(
        json!({"profile_id":uuid(),"prompt":{"type":"string","minLength":1,"maxLength":32768},
        "session_id":uuid(),"approval":crate::application::workflow::registry::action_approval_schema()}),
        &["profile_id", "prompt"],
    )
}
pub fn frozen_schema() -> Value {
    let policy = object(
        json!({"workspace":{"const":"project_directory"},"permissions":{"enum":["full","auto","accept_edits"]},
        "reasoning":{"enum":["none","low","medium","high","xhigh","max","ultra","ultracode"]}}),
        &["workspace", "permissions", "reasoning"],
    );
    let selection = object(
        json!({"machine_id":uuid(),"project_id":external(),
        "provider":{"enum":["claude-code","codex","acp-opencode"]},"model":{"type":"string","minLength":1,"maxLength":128},
        "policy":policy}),
        &["machine_id", "project_id", "provider", "model", "policy"],
    );
    let mut input = input_schema();
    input["properties"]["selection"] = selection;
    input["properties"]["harness"] = json!({"oneOf":[
        object(json!({"kind":{"const":"unconfigured"}}), &["kind"]),
        object(json!({"kind":{"const":"agent"},"agent_id":uuid(),"revision_id":uuid(),"digest":{"type":"string","pattern":"^[0-9a-f]{64}$"}}), &["kind","agent_id","revision_id","digest"])
    ]});
    input["properties"]["deadline"] = json!({"type":"string","maxLength":64});
    // The frozen internal form always serializes the two optional authored fields.
    input["properties"]["session_id"] = json!({"anyOf":[uuid(),{"type":"null"}]});
    input["properties"]["approval"] = json!({"anyOf":[crate::application::workflow::registry::action_approval_schema(),{"type":"null"}]});
    input["required"] = json!([
        "profile_id",
        "prompt",
        "session_id",
        "approval",
        "selection",
        "harness",
        "deadline"
    ]);
    input
}
pub fn result_schema() -> Value {
    let usage = object(
        json!({"input_tokens":{"type":"integer","minimum":0,"maximum":9007199254740991_u64},
        "output_tokens":{"type":"integer","minimum":0,"maximum":9007199254740991_u64}}),
        &["input_tokens", "output_tokens"],
    );
    object(
        json!({"status":{"enum":["succeeded","failed","cancelled","expired","uncertain"]},
        "operation_id":uuid(),"session_id":uuid(),"thread_id":{"anyOf":[external(),{"type":"null"}]},
        "text":{"type":"string","maxLength":MAX_WORKFLOW_TEXT_BYTES},"truncated":{"type":"boolean"},
        "usage":{"anyOf":[usage,{"type":"null"}]}}),
        &[
            "status",
            "operation_id",
            "session_id",
            "thread_id",
            "text",
            "truncated",
            "usage",
        ],
    )
}
