use super::*;
use serde_json::json;

fn reference(path: &str) -> ReplyText {
    ReplyText::Reference {
        r#ref: path.to_owned().try_into().unwrap(),
    }
}
fn contract() -> ChannelReplyContract {
    ChannelReplyContract {
        format_version: 1,
        subject: ReplyText::Literal {
            literal: "Status".into(),
        },
        body: reference("/input/answer"),
    }
}
fn schema() -> Value {
    json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]})
}

#[test]
fn channel_reply_contract_projects_only_declared_content_and_preserves_result() {
    let result = json!({"answer":"Done","no_reply":true,"destination":"attacker@example.test"});
    let saved = result.clone();
    let content = contract().project(&schema(), &result).unwrap();
    assert_eq!(content.body_text(), "Done");
    assert_eq!(result, saved);
}

#[test]
fn channel_reply_contract_uses_existing_reference_pointer_rules() {
    let contract = ChannelReplyContract {
        body: reference("/input/a~1b/~0text"),
        ..contract()
    };
    let schema = json!({"type":"object","required":["a/b"],"properties":{"a/b":{
        "type":"object","required":["~text"],"properties":{"~text":{"type":"string"}}}}});
    assert_eq!(
        contract
            .project(&schema, &json!({"a/b":{"~text":"hello"}}))
            .unwrap()
            .body_text(),
        "hello"
    );
    for invalid in [
        "/run/run_id",
        "/params/body",
        "/steps/a/output",
        "/input/bad~2escape",
    ] {
        assert!(
            ReplyReference::try_from(invalid.to_owned()).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn channel_reply_contract_refuses_optional_ambiguous_and_non_string_schema_paths() {
    for schema in [
        json!(true),
        json!({"type":"string"}),
        json!({"type":"object","properties":{"answer":{"type":"string"}}}),
        json!({"type":"object","properties":{"answer":{"type":"number"}},"required":["answer"]}),
        json!({"type":"object","properties":{"answer":{"type":["string","null"]}},"required":["answer"]}),
        json!({"anyOf":[schema(),schema()]}),
        json!({"type":"object","properties":{"answer":{"type":"string","$ref":"#/$defs/body"}},
            "$defs":{"body":{"type":"string"}},"required":["answer"]}),
    ] {
        assert!(contract().validate(&schema).is_err(), "{schema}");
    }
}

#[test]
fn channel_reply_contract_rejects_missing_wrong_blank_and_oversized_results() {
    for result in [
        json!({}),
        json!({"answer":7}),
        json!({"answer":" \n\t"}),
        json!({"answer":"x".repeat(1024*1024+1)}),
    ] {
        assert!(contract().project(&schema(), &result).is_err());
    }
    let subject = ChannelReplyContract {
        subject: ReplyText::Literal {
            literal: "x".repeat(999),
        },
        ..contract()
    };
    assert!(subject.validate(&schema()).is_err());
}

#[test]
fn channel_reply_contract_literal_output_still_checks_business_schema() {
    let contract = ChannelReplyContract {
        body: ReplyText::Literal {
            literal: "Done".into(),
        },
        ..contract()
    };
    assert!(contract.project(&schema(), &json!({})).is_err());
    assert!(
        contract
            .project(&schema(), &json!({"answer":"business result"}))
            .is_ok()
    );
    assert!(contract.project(&json!(true), &json!(null)).is_ok());
}

#[test]
fn channel_reply_contract_roundtrip_refuses_unknown_fields_and_arbitrary_operators() {
    let value = serde_json::to_value(contract()).unwrap();
    assert_eq!(
        serde_json::from_value::<ChannelReplyContract>(value.clone()).unwrap(),
        contract()
    );
    for value in [
        json!({"format_version":1,"subject":{"literal":"x"},"body":{"literal":"ok"},"to":"elsewhere"}),
        json!({"format_version":1,"subject":{"literal":"x"},"body":{"literal":"ok","to":"elsewhere"}}),
        json!({"format_version":1,"subject":{"literal":"x"},"body":{"concat":[{"literal":"ok"}]}}),
    ] {
        assert!(serde_json::from_value::<ChannelReplyContract>(value).is_err());
    }
}

#[test]
fn channel_reply_contract_requires_supported_explicit_format_version() {
    let mut unsupported = contract();
    unsupported.format_version = 2;
    assert!(unsupported.validate(&schema()).is_err());
    let mut missing = serde_json::to_value(contract()).unwrap();
    missing.as_object_mut().unwrap().remove("format_version");
    assert!(serde_json::from_value::<ChannelReplyContract>(missing).is_err());
}

#[test]
fn channel_reply_contract_bounds_stored_literals_before_evaluation() {
    let too_large = ChannelReplyContract {
        body: ReplyText::Literal {
            literal: "x".repeat(MAX_CONTRACT_BYTES),
        },
        ..contract()
    };
    assert!(too_large.validate(&json!(true)).is_err());
    let blank = ChannelReplyContract {
        body: ReplyText::Literal {
            literal: "   ".into(),
        },
        ..contract()
    };
    assert!(blank.validate(&json!(true)).is_err());
}
