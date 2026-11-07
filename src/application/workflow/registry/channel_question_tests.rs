use super::tests::*;
use super::*;
use crate::application::workflow::triggers::{CHANNEL_QUESTION_REPLY_EVENT, MessageTriggerInput};

#[test]
fn channel_reply_hitl_prompt_declares_a_human_result_separate_from_its_receipt() {
    let example = example("message.ask").unwrap();
    let compiled = compile_example(&example).unwrap();
    assert_eq!(
        compiled.step_output_schema(&step()),
        Some(&MessageTriggerInput::schema())
    );
    let (_, receipt) = message_tool_schemas("message.ask").unwrap();
    assert_ne!(receipt, MessageTriggerInput::schema());
    compiled
        .validate_step_output(&step(), &MessageTriggerInput::representative())
        .unwrap();
    assert!(
        compiled
            .validate_step_output(&step(), &json!({"status":"accepted","messages":[]}))
            .is_err()
    );
}

#[test]
fn channel_reply_hitl_prompt_schema_is_literal_and_exact() {
    for schema in [json!(true), json!({"type":"string"})] {
        let mut example = example("message.ask").unwrap();
        changed(&mut example, |source| {
            source["steps"]["start"]["with"]["payload_schema"] = json!({"literal":schema})
        });
        assert_eq!(
            compile_example(&example).err().unwrap().code,
            "registry.question_schema"
        );
    }
    let mut example = example("message.ask").unwrap();
    changed(&mut example, |source| {
        source["steps"]["start"]["with"]["payload_schema"] = json!({"ref":"/input"})
    });
    assert!(compile_example(&example).is_err());
}

#[test]
fn channel_reply_hitl_prompt_private_reply_event_cannot_be_declared_generically() {
    let mut example = example("wait.event").unwrap();
    changed(&mut example, |source| {
        source["steps"]["start"]["with"]["event"] = json!({"literal":CHANNEL_QUESTION_REPLY_EVENT})
    });
    assert_eq!(
        compile_example(&example).err().unwrap().code,
        "registry.reserved_event"
    );
}
