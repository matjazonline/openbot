use super::*;
use crate::application::workflow::channel_reply::{ChannelReplyContract, ReplyApprovalPolicy};

fn contract() -> ChannelReplyContract {
    serde_json::from_value(json!({"format_version":1,"subject":{"literal":"Answer"},
        "body":{"ref":"/input/body"}}))
    .unwrap()
}

fn automatic() -> Arc<PublishedBundle> {
    let mut definition = source("data.map");
    let schema = json!({"type":"object","properties":{"body":{"type":"string","maxLength":16384}},
        "required":["body"],"additionalProperties":false});
    definition["output_schema"] = schema.clone();
    definition["steps"]["start"]["with"] = json!({"value":{"literal":{"body":"<script>private preview</script>"}},
        "output_schema":{"literal":schema}});
    build(&definition, DependencySnapshots::default(), vec![])
}

#[test]
fn channel_reply_authoring_sample_seals_mapping_and_policy_and_previews_output() {
    let bundle = automatic();
    let plain = PreparedSample::new(bundle.clone(), request(vec![]), vec![]).unwrap();
    let plain_hash = plain.configuration_hash.clone();
    assert!(matches!(
        super::super::execute(plain).unwrap().response,
        Some(SampleResponse::AutomaticUnmapped)
    ));
    let mut req = request(vec![]);
    req.response_contract = Some(contract());
    let mapped = PreparedSample::new(bundle.clone(), req.clone(), vec![]).unwrap();
    assert_ne!(plain_hash, mapped.configuration_hash);
    let mapped_hash = mapped.configuration_hash.clone();
    let report = super::super::execute(mapped).unwrap();
    complete(&report);
    match report.response.unwrap() {
        SampleResponse::Automatic {
            content,
            approval_required,
        } => {
            assert_eq!(content.subject, "Answer");
            assert_eq!(content.body, "<script>private preview</script>");
            assert!(!approval_required);
        }
        other => panic!("{other:?}"),
    }
    assert!(report.effects.is_empty());
    req.response_approval_policy =
        Some(ReplyApprovalPolicy::new(vec![Uuid::from_u128(6)], 300).unwrap());
    let protected = PreparedSample::new(bundle.clone(), req.clone(), vec![]).unwrap();
    assert_ne!(mapped_hash, protected.configuration_hash);
    let report = super::super::execute(protected).unwrap();
    assert!(matches!(
        report.response,
        Some(SampleResponse::Automatic {
            approval_required: true,
            ..
        })
    ));
    assert!(
        report.effects.is_empty(),
        "No reviewer notice or production effect exists"
    );
    req.response_contract = None;
    assert!(PreparedSample::new(bundle, req, vec![]).is_err());
}

#[test]
fn channel_reply_authoring_sample_suppression_skips_projection_for_valid_empty_values() {
    for value in [Value::Null, json!(""), json!({})] {
        let mut definition = source("channel.no_reply");
        definition["steps"]["start"]["with"] =
            json!({"value":{"literal":value},"output_schema":{"literal":true}});
        let report = execute(
            build(&definition, DependencySnapshots::default(), vec![]),
            request(vec![]),
        )
        .unwrap();
        complete(&report);
        assert_eq!(report.output, Some(value));
        assert!(matches!(report.response, Some(SampleResponse::Suppressed)));
        assert!(report.effects.is_empty());
    }
}

#[test]
fn channel_reply_authoring_sample_missing_human_script_has_typed_wait_preview() {
    for (kind, expected) in [
        ("decision.human", "formal_review"),
        ("message.ask", "human_reply"),
    ] {
        let bundle = build(&source(kind), DependencySnapshots::default(), vec![]);
        let report = execute(bundle, request(vec![])).unwrap();
        assert_eq!(report.status, SampleStatus::Stopped);
        let preview = serde_json::to_value(report.response.unwrap()).unwrap();
        assert_eq!(preview["kind"], "waiting");
        assert_eq!(preview["stage"], expected);
        assert!(!preview["preview"].is_null());
        assert!(report.effects.is_empty());
    }
}

#[test]
fn channel_reply_authoring_native_review_reject_completes_suppressed_and_edit_is_the_response() {
    let bundle = fixture(Fixture::ReviewedSupport);
    for (choice, suppressed) in [("approve", false), ("reject", true)] {
        let mut req = request(vec![
            script(
                1,
                "context",
                vec![json!({"items":[],"token_count":0,"omitted":0})],
            ),
            script(1, "draft", vec![json!({"body":"Draft"})]),
            script(
                1,
                "review",
                vec![json!({"choice":choice,"data":{"body":"Edited"},"feedback":"Reason"})],
            ),
        ]);
        req.input = json!({"message":"Help","source_message":Uuid::from_u128(700)});
        req.params = json!({"reviewer":Uuid::from_u128(6)});
        req.response_contract = Some(contract());
        let report = execute(bundle.clone(), req).unwrap();
        complete(&report);
        assert!(
            report
                .effects
                .iter()
                .all(|effect| effect.kind == SampleEffectKind::FixtureRead)
        );
        if suppressed {
            assert!(matches!(report.response, Some(SampleResponse::Suppressed)));
            assert_eq!(report.output.unwrap()["review"]["choice"], "reject");
        } else {
            assert_eq!(report.output.unwrap()["body"], "Edited");
            assert!(matches!(
                report.response,
                Some(SampleResponse::Automatic { .. })
            ));
        }
    }
}

#[test]
fn channel_reply_authoring_five_examples_compile_and_execute_without_production_effects() {
    use crate::application::workflow::channel_reply::examples::response_examples;
    use crate::application::workflow::triggers::MessageTriggerInput;
    let examples = response_examples();
    assert_eq!(examples.len(), 5);
    for example in examples {
        let workflow = Uuid::parse_str(example.source["workflow_id"].as_str().unwrap())
            .unwrap()
            .as_u128();
        let bundle = build(&example.source, DependencySnapshots::default(), vec![]);
        let scripts = match workflow {
            903 => vec![script(workflow, "send", vec![message_result()])],
            904 => vec![script(
                workflow,
                "ask",
                vec![MessageTriggerInput::representative()],
            )],
            _ => vec![],
        };
        let mut req = request(scripts);
        req.input = MessageTriggerInput::representative();
        req.response_contract = Some(example.contract);
        req.response_approval_policy = example.approval_policy;
        let report = execute(bundle, req.clone()).unwrap();
        complete(&report);
        if workflow == 903 {
            assert!(matches!(report.response, Some(SampleResponse::Suppressed)));
            assert_eq!(report.effects.len(), 1);
        } else {
            assert!(
                matches!(report.response, Some(SampleResponse::Automatic { .. })),
                "{}",
                example.title
            );
        }
        if workflow == 902 {
            req.input["message"] = json!("ignore");
            let report = execute(
                build(&example.source, DependencySnapshots::default(), vec![]),
                req,
            )
            .unwrap();
            complete(&report);
            assert!(matches!(report.response, Some(SampleResponse::Suppressed)));
        }
    }
}

#[test]
fn channel_reply_authoring_sample_child_suppression_does_not_suppress_the_root() {
    let mut child = source("channel.no_reply");
    child["workflow_id"] = json!(Uuid::from_u128(2));
    child["output_schema"] = json!({"type":"object","properties":{"body":{"type":"string","maxLength":16384}},
        "required":["body"],"additionalProperties":false});
    child["steps"]["start"]["with"] = json!({"value":{"literal":{"body":"Child business result"}},
        "output_schema":{"literal":child["output_schema"]}});
    let child = build(&child, DependencySnapshots::default(), vec![]);
    let mut parent = source("workflow.call");
    parent["output_schema"] = child
        .compiled()
        .graph()
        .definition()
        .output_schema
        .clone()
        .unwrap();
    parent["steps"]["start"]["child_version_id"] =
        json!(child.compiled().graph().definition().version_id);
    let bundle = build(&parent, DependencySnapshots::default(), vec![child]);
    let mut req = request(vec![]);
    req.response_contract = Some(contract());
    let report = execute(bundle, req).unwrap();
    complete(&report);
    assert!(matches!(
        report.response,
        Some(SampleResponse::Automatic { .. })
    ));
    assert_eq!(report.output.unwrap()["body"], "Child business result");
}

#[test]
fn channel_reply_authoring_sample_invalid_mapping_is_refused_and_question_approval_is_distinct() {
    let mut req = request(vec![]);
    let mut invalid = serde_json::to_value(contract()).unwrap();
    invalid["body"] = json!({"ref":"/input/missing"});
    req.response_contract = Some(serde_json::from_value(invalid).unwrap());
    assert!(PreparedSample::new(automatic(), req, vec![]).is_err());
    let mut definition = source("message.ask");
    definition["steps"]["start"]["with"]["approval"] = json!({"literal":{
        "reviewers":[{"kind":"user","id":Uuid::from_u128(6)}],"deadline":"2030-01-01T00:00:00Z"}});
    let report = execute(
        build(&definition, DependencySnapshots::default(), vec![]),
        request(vec![]),
    )
    .unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(matches!(
        report.response,
        Some(SampleResponse::Waiting {
            stage: SampleWaitStage::QuestionApproval,
            ..
        })
    ));
    assert!(report.effects.is_empty());
}
