//! Runnable examples with explicit sample/channel configuration, never a live grant.
use super::{ChannelReplyContract, ReplyApprovalPolicy};
use crate::application::workflow::triggers::MessageTriggerInput;
use serde_json::{Value, json};
use uuid::Uuid;

pub struct ResponseExample {
    pub title: &'static str,
    pub source: Value,
    pub contract: ChannelReplyContract,
    pub approval_policy: Option<ReplyApprovalPolicy>,
}

pub fn response_examples() -> Vec<ResponseExample> {
    let ordinary = base(901, "Automatic final reply");
    let mut conditional = base(902, "Conditional finish without reply");
    conditional.source["steps"]["choose"] = json!({"type":"decision.rule","with":{
        "data":{"ref":"/input/message"},"data_schema":{"literal":{"type":"string"}}},
        "rule":{"cases":[{"when":{"eq":[{"ref":"/input/message"},{"literal":"ignore"}]},"choice":"silent"}],"default":"answer"},
        "routes":{"choices":{"silent":"silent","answer":"start"}}});
    conditional.source["steps"]["silent"] =
        response_step("channel.no_reply", json!({"literal":{"body":""}}));
    conditional.source["entry"] = json!("choose");
    let mut explicit = base(903, "Explicit reply, then finish without reply");
    explicit.source["steps"]["send"] = json!({"type":"message.reply","with":{
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"subject":"Response","body":"Sent explicitly"}}},"routes":{"success":"start"}});
    explicit.source["steps"]["start"]["type"] = json!("channel.no_reply");
    explicit.source["entry"] = json!("send");
    let mut question = base(904, "Human input, then automatic final reply");
    question.source["steps"]["ask"] = json!({"type":"message.ask","with":{
        "source_message":{"ref":"/input/source_message"},
        "content":{"literal":{"subject":"Question","body":"What should we do next?"}},
        "deadline":{"ref":"/run/deadline"},"payload_schema":{"literal":MessageTriggerInput::schema()}},
        "routes":{"success":"start"}});
    question.source["steps"]["start"] = response_step(
        "data.map",
        json!({"object":{"body":{"ref":"/steps/ask/output/message"}}}),
    );
    question.source["entry"] = json!("ask");
    let mut protected = base(905, "Protected automatic final reply");
    protected.approval_policy =
        Some(ReplyApprovalPolicy::new(vec![Uuid::from_u128(6)], 300).expect("example policy"));
    vec![ordinary, conditional, explicit, question, protected]
}

fn schema() -> Value {
    json!({"type":"object","properties":{"body":{"type":"string","maxLength":16384}},
        "required":["body"],"additionalProperties":false})
}

fn response_step(kind: &str, value: Value) -> Value {
    json!({"type":kind,"with":{"value":value,"output_schema":{"literal":schema()}},
        "routes":{"success":"$end"}})
}

fn base(id: u128, title: &'static str) -> ResponseExample {
    ResponseExample {
        title,
        source: json!({"format_version":1,"workflow_id":Uuid::from_u128(id),
            "input_schema":MessageTriggerInput::schema(),
            "parameter_schema":{"type":"object","additionalProperties":false},
            "output_schema":schema(),"resources":[],"entry":"start",
            "steps":{"start":response_step("data.map",json!({"literal":{"body":"Completed"}}))},
            "limits":{"max_steps":10,"max_context_bytes":65536}}),
        contract: serde_json::from_value(
            json!({"format_version":1,"subject":{"literal":"Response"},
            "body":{"ref":"/input/body"}}),
        )
        .expect("example contract"),
        approval_policy: None,
    }
}
