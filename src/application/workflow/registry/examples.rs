use super::*;
use uuid::Uuid;

/// A complete v1 source and the explicit illustrative contracts it requires.
/// Resource declarations/facts here are examples, never authorization grants.
pub struct AuthoringExample {
    pub source: String,
    pub facts: CatalogueFacts,
    pub dependencies: BTreeMap<WorkflowId, BTreeSet<WorkflowId>>,
}
pub fn example(name: &str) -> Option<AuthoringExample> {
    if !TYPES.contains(&name) {
        return None;
    }
    let workflow = WorkflowId::new(Uuid::from_u128(1));
    let child = WorkflowId::new(Uuid::from_u128(2));
    let version = VersionId::new(Uuid::from_u128(3));
    let mut facts = CatalogueFacts::default();
    let mut dependencies = BTreeMap::from([(workflow, BTreeSet::new())]);
    let mut resources = json!([]);
    let mut step = json!({"type":name,"with":{},"routes":{"success":"$end"}});
    let values = values(name);
    for (field, value) in values.as_object().expect("example fields") {
        step["with"][field] = json!({"literal":value});
    }
    if name.starts_with("decision.") {
        step["routes"] = json!({"choices":{"continue":"$end","revise":"$end"}});
    }
    if name == "decision.rule" {
        step["rule"] =
            json!({"cases":[{"when":{"literal":true},"choice":"continue"}],"default":"revise"});
    }
    if ["http.request", "mcp.call"].contains(&name) {
        resources =
            json!([{"slot":"service","kind":if name == "http.request" { "http" } else { "mcp" }}]);
    }
    if ["tool.call", "mcp.call"].contains(&name) {
        facts.tools.push(ToolContract {
            name: TypeName::parse("lookup").expect("example name"),
            connection: if name == "mcp.call" {
                Some(ResourceName::parse("service").expect("example slot"))
            } else {
                None
            },
            input_schema: object(json!({"key":text(128)}), &["key"]),
            output_schema: object(json!({"result":text(128)}), &["result"]),
        });
    }
    if ["workflow.call", "flow.repeat"].contains(&name) {
        step["child_workflow_id"] = json!(child.to_string());
        step["child_version_id"] = json!(version.to_string());
        if name == "flow.repeat" {
            step["max_iterations"] = json!(3);
        }
        facts.children.push(ChildContract {
            workflow_id: child,
            version_id: version,
            input_schema: object(json!({"text":text(128)}), &["text"]),
            output_schema: text(128),
        });
        dependencies.insert(workflow, BTreeSet::from([child]));
        dependencies.insert(child, BTreeSet::new());
    }
    let source = json!({"format_version":1,"workflow_id":workflow.to_string(),"input_schema":true,"parameter_schema":true,"output_schema":true,"resources":resources,"entry":"start","steps":{"start":step},"limits":{"max_steps":100,"max_context_bytes":65536}}).to_string();
    Some(AuthoringExample {
        source,
        facts,
        dependencies,
    })
}
fn values(name: &str) -> Value {
    let schema = json!({"type":"string","maxLength":128});
    let deadline = "2030-01-01T00:00:00Z";
    match name {
        "context.load" => json!({"sources":["conversation"],"max_tokens":1000}),
        "memory.load" => json!({"scope":{"kind":"company"},"query":"preferences","limit":10}),
        "memory.save" => {
            json!({"scope":{"kind":"agent","id":"assistant"},"facts":["Prefers concise replies"]})
        }
        "ai.classify" => {
            json!({"context":{"message":"hello"},"output_schema":{"type":"string","enum":["support","sales"]}})
        }
        "agent.run" => {
            json!({"agent":"assistant","context":{"message":"hello"},"output_schema":schema})
        }
        "decision.rule" => json!({"data":"review","data_schema":schema}),
        "decision.human" => {
            json!({"proposal":{"summary":"review"},"reviewer":{"kind":"user","id":"reviewer"},"deadline":deadline,"data_schema":schema,"feedback_required":["revise"]})
        }
        "decision.agent" => {
            json!({"agent":"reviewer","context":{"message":"hello"},"data_schema":schema})
        }
        "data.map" => json!({"value":"hello","output_schema":schema}),
        "http.request" => {
            json!({"connection":"service","method":"POST","path":"/messages","headers":{"Accept":"application/json"},"body":{"message":"hello"}})
        }
        "tool.call" => json!({"tool":"lookup","arguments":{"key":"hello"}}),
        "mcp.call" => json!({"connection":"service","tool":"lookup","arguments":{"key":"hello"}}),
        "message.send" => {
            json!({"content":{"body":"hello"},"destinations":[{"kind":"channel","id":"support"}]})
        }
        "message.reply" => json!({"content":{"body":"hello"},"source_message":"message-1"}),
        "workflow.call" | "flow.repeat" => json!({"input":{"text":"hello"}}),
        "wait.event" => {
            json!({"event":"review.completed","correlation":"request-1","deadline":deadline,"payload_schema":schema})
        }
        "wait.timer" => json!({"deadline":deadline}),
        _ => unreachable!("registered example"),
    }
}
