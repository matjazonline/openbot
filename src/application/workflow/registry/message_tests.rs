//! 04.11-D2 §1.1 (F003) bounds: byte-exact subjects, destination counts, duplicates, groups.
use super::tests::*;
use super::*;

fn send_with(edit: impl FnOnce(&mut Value)) -> Result<CompiledWorkflow, Diagnostic> {
    let mut example = example("message.send").unwrap();
    changed(&mut example, |v| edit(&mut v["steps"]["start"]["with"]));
    compile_example(&example)
}
fn subject(subject: &str) -> Result<CompiledWorkflow, Diagnostic> {
    send_with(|with| with["content"] = json!({"literal":{"subject":subject,"body":"hello"}}))
}
fn destinations(count: usize, kind: &str) -> Value {
    json!({"literal":(0..count).map(|i| json!({"kind":kind,"id":format!("00000000-0000-0000-0000-{i:012}")})).collect::<Vec<_>>()})
}

#[test]
fn message_registry_subject_is_bounded_in_utf8_bytes_at_publication() {
    // 332 three-byte characters and one two-byte character: exactly 998 bytes.
    let exact = format!("{}é", "€".repeat(332));
    assert_eq!(exact.len(), 998);
    assert!(subject(&exact).is_ok());
    assert_eq!(
        subject(&format!("{exact}a")).err().unwrap().code,
        "registry.message_subject"
    );
    // A four-byte character ending at exactly 998 is accepted; crossing 998 is refused.
    let ending = format!("{}{}", "a".repeat(994), "😀");
    assert_eq!(ending.len(), 998);
    assert!(subject(&ending).is_ok());
    let crossing = format!("{}{}", "a".repeat(995), "😀");
    assert_eq!(crossing.len(), 999);
    assert_eq!(
        subject(&crossing).err().unwrap().code,
        "registry.message_subject"
    );
}

#[test]
fn message_registry_runtime_check_refuses_what_the_schema_admits() {
    let mut example = example("message.send").unwrap();
    changed(&mut example, |v| {
        v["input_schema"] = json!({"type":"object","properties":{"subject":{"type":"string"}},"required":["subject"]});
        v["steps"]["start"]["with"]["content"] = json!({"literal":{"body":"hello"}});
        v["steps"]["start"]["with"]["content"] = json!({"ref":"/input/content"});
        v["input_schema"] =
            json!({"type":"object","properties":{"content":true},"required":["content"]});
    });
    let compiled = compile_example(&example).ok().unwrap();
    let prepare = |content: Value| {
        let input = json!({"content":content});
        let (params, run, outputs) = (json!({}), json!({}), BTreeMap::new());
        compiled
            .prepare_step_inputs(&step(), &context(&input, &params, &outputs, &run))
            .map(|p| p.value().clone())
    };
    // 998 code points of a three-byte character satisfy `maxLength` (code points) but are
    // 2994 bytes: refused before any marker could be written.
    let wide = "€".repeat(998);
    assert_eq!(
        prepare(json!({"subject":wide,"body":"hello"}))
            .unwrap_err()
            .code,
        "registry.message_subject"
    );
    prepare(json!({"subject":"€".repeat(332),"body":"hello"})).unwrap();
}

#[test]
fn message_registry_destinations_are_twenty_distinct_users_or_channels() {
    assert!(send_with(|with| with["destinations"] = destinations(20, "user")).is_ok());
    assert!(send_with(|with| with["destinations"] = destinations(21, "user")).is_err());
    assert!(send_with(|with| with["destinations"] = destinations(0, "channel")).is_err());
    // Groups are not a destination kind (user decision: users and channels only).
    assert!(send_with(|with| with["destinations"] = destinations(1, "group")).is_err());
    let duplicate = json!({"literal":[{"kind":"user","id":"00000000-0000-0000-0000-000000000001"},
                                       {"kind":"user","id":"00000000-0000-0000-0000-000000000001"}]});
    assert!(send_with(|with| with["destinations"] = duplicate).is_err());
    let not_uuid = json!({"literal":[{"kind":"channel","id":"support"}]});
    assert!(send_with(|with| with["destinations"] = not_uuid).is_err());
}

#[test]
fn message_registry_built_in_names_cannot_be_registered_tools() {
    let mut example = example("tool.call").unwrap();
    example.facts.tools[0].name = TypeName::parse("message.send").unwrap();
    changed(&mut example, |v| {
        v["steps"]["start"]["with"]["tool"] = json!({"literal":"message.send"})
    });
    assert!(compile_example(&example).is_err());
}

/// A `message.send` whose first message is awaited by a `message.reply` wait, edited by `edit`.
fn reply_wait(edit: impl FnOnce(&mut Value)) -> Result<CompiledWorkflow, Diagnostic> {
    use crate::application::workflow::triggers::MessageTriggerInput;
    let mut example = example("message.send").unwrap();
    changed(&mut example, |v| {
        v["steps"]["start"]["routes"]["success"] = json!("answer");
        v["steps"]["answer"] = json!({"type":"wait.event","with":{
            "event":{"literal":"message.reply"},
            "correlation":{"ref":"/steps/start/output/messages/0/message_id"},
            "deadline":{"literal":"2030-01-01T00:00:00Z"},
            "payload_schema":{"literal":MessageTriggerInput::schema()}},
            "routes":{"success":"$end"}});
        edit(&mut v["steps"]["answer"]["with"]);
    });
    compile_example(&example)
}

/// p08 D1.5.1 (F004.3, F018): a reply wait receives exactly the standard message payload, and
/// correlates on a message step's outbound message, the only correlation a reply can carry.
#[test]
fn message_registry_reply_wait_takes_the_standard_payload_and_a_message_correlation() {
    reply_wait(|_| {}).unwrap();
    let schema = reply_wait(|with| with["payload_schema"] = json!({"literal":{"type":"object"}}));
    assert_eq!(
        schema.err().map(|error| error.code),
        Some("registry.message_reply_schema")
    );
    for correlation in [
        json!({"literal":"request-1"}),
        json!({"ref":"/steps/start/output/status"}),
        json!({"ref":"/steps/start/output/messages/first/message_id"}),
        json!({"ref":"/input"}),
    ] {
        let error = reply_wait(|with| with["correlation"] = correlation.clone());
        assert_eq!(
            error.err().map(|error| error.code),
            Some("registry.message_reply_correlation"),
            "{correlation}"
        );
    }
    // Any other event keeps its own payload schema and correlation.
    reply_wait(|with| {
        with["event"] = json!({"literal":"review.completed"});
        with["correlation"] = json!({"literal":"request-1"});
        with["payload_schema"] = json!({"literal":{"type":"object"}});
    })
    .unwrap();
}

/// p08 D2.1: a `tool.call` step cannot select a built-in provisioning tool, even when the facts
/// supply a contract of that name; an `mcp.call` of that name is the server's own tool.
#[test]
fn provisioning_names_are_not_step_tools() {
    use crate::application::workflow::provisioning::{CREATE_AGENT_CHANNEL, LIST_COMPANY_AGENTS};
    let renamed = |kind: &str, name: &str| {
        let mut example = example(kind).unwrap();
        changed(&mut example, |v| {
            let tool = &mut v["steps"]["start"]["with"]["tool"];
            match tool.get_mut("literal") {
                Some(literal) => *literal = json!(name),
                None => *tool = json!(name),
            }
        });
        for tool in &mut example.facts.tools {
            if tool.name.as_str() == "lookup" {
                tool.name = crate::domain::workflow::TypeName::parse(name).unwrap();
            }
        }
        compile_example(&example)
    };
    for name in [LIST_COMPANY_AGENTS, CREATE_AGENT_CHANNEL] {
        assert_eq!(
            renamed("tool.call", name).err().unwrap().code,
            "registry.tool"
        );
        assert!(renamed("mcp.call", name).is_ok(), "{name}");
    }
}
