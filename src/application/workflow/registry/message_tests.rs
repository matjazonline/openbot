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
