use super::*;
use crate::application::workflow::triggers::MessageTriggerInput;
use crate::domain::entities::message::CanonicalMessageId;
use chrono::Duration;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-22T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn run_deadline() -> DateTime<Utc> {
    now() + Duration::hours(1)
}
fn desk() -> Uuid {
    Uuid::from_u128(0xD5)
}
fn call(arguments: Value) -> Result<ChannelCallRequest, ChannelCallError> {
    ChannelCallRequest::parse(&arguments, now(), run_deadline())
}
fn valid() -> Value {
    json!({"channel_id":desk().to_string(),"content":{"subject":"Order review","body":"Review order 42."}})
}
fn code(result: Result<ChannelCallRequest, ChannelCallError>) -> &'static str {
    result.unwrap_err().code
}
fn reply(body: &str) -> CallReply {
    CallReply {
        content: CanonicalContent::parse("Re: Order review", body).unwrap(),
        fields: None,
    }
}
fn writable() -> DeskFacts {
    DeskFacts {
        writable: true,
        in_ancestry: false,
        unit_archived: false,
        enabled: true,
        responder: true,
    }
}

// --- channel_call_contract_ ------------------------------------------------------------------

#[test]
fn channel_call_contract_accepts_exactly_the_documented_fields() {
    let request = call(valid()).unwrap();
    assert_eq!(request.channel, desk());
    assert_eq!(request.content.subject(), "Order review");
    assert_eq!(request.content.body_text(), "Review order 42.");
    assert_eq!(request.fields, None);
    assert_eq!(request.deadline, None);

    let mut full = valid();
    full["fields"] = json!({"order_id":"42"});
    full["deadline"] = json!("2026-11-22T12:30:00Z");
    let request = call(full).unwrap();
    assert_eq!(request.fields.unwrap()["order_id"], "42");
    assert_eq!(request.deadline, Some(now() + Duration::minutes(30)));

    // A subject is optional, as for message.send.
    let request = call(json!({"channel_id":desk().to_string(),"content":{"body":"Hi"}})).unwrap();
    assert_eq!(request.content.subject(), "");
}

#[test]
fn channel_call_contract_refuses_identity_spoofing_and_unknown_fields() {
    for spoof in [
        "source_message",
        "sender",
        "workflow_id",
        "binding_id",
        "run_as",
        "params",
        "thread_id",
        "agent",
    ] {
        let mut arguments = valid();
        arguments[spoof] = json!("00000000-0000-0000-0000-000000000001");
        assert_eq!(code(call(arguments)), INVALID_INPUT, "{spoof}");
    }
    let mut extra_content = valid();
    extra_content["content"]["from"] = json!("ceo@example.com");
    assert_eq!(code(call(extra_content)), INVALID_INPUT);
    assert_eq!(code(call(json!("text"))), INVALID_INPUT);
    assert_eq!(
        code(call(json!({"channel_id":"desk-7","content":{"body":"x"}}))),
        INVALID_INPUT
    );
    assert_eq!(
        code(call(json!({"channel_id":desk().to_string()}))),
        INVALID_INPUT
    );
    for content in [
        json!({"body":""}),
        json!({"body":"  "}),
        json!({"body":"\u{200B}\u{2060}\u{FEFF}"}),
        json!({"subject":"s"}),
        json!({"body":7}),
    ] {
        assert_eq!(
            code(call(
                json!({"channel_id":desk().to_string(),"content":content})
            )),
            INVALID_INPUT
        );
    }
    let mut fields = valid();
    fields["fields"] = json!(["not", "an", "object"]);
    assert_eq!(code(call(fields)), INVALID_INPUT);
}

#[test]
fn channel_call_contract_bounds_content_in_bytes_at_char_boundaries() {
    let at_bound = "é".repeat(MESSAGE_INPUT_BODY_BYTES / 2);
    let mut arguments = valid();
    arguments["content"]["body"] = json!(at_bound);
    assert_eq!(
        call(arguments).unwrap().content.body_text().len(),
        MESSAGE_INPUT_BODY_BYTES
    );
    let mut over = valid();
    over["content"]["body"] = json!(format!("{at_bound}a"));
    assert_eq!(code(call(over)), INVALID_INPUT);

    let mut subject = valid();
    subject["content"]["subject"] = json!("ü".repeat(MAX_SUBJECT_BYTES / 2 + 1));
    assert_eq!(code(call(subject)), INVALID_INPUT);

    let mut fields = valid();
    fields["fields"] = json!({"notes":"x".repeat(MAX_FIELDS_BYTES)});
    assert_eq!(code(call(fields)), INVALID_INPUT);
}

#[test]
fn channel_call_contract_deadline_must_fall_before_the_runs() {
    for (deadline, accepted) in [
        ("2026-11-22T12:59:59Z", true),
        ("2026-11-22T13:00:00Z", false), // equal to the run's
        ("2026-11-22T14:00:00Z", false), // later
        ("2026-11-22T12:00:00Z", false), // now
        ("2026-11-22T11:00:00Z", false), // past
        ("tomorrow", false),
    ] {
        let mut arguments = valid();
        arguments["deadline"] = json!(deadline);
        match call(arguments) {
            Ok(request) => {
                assert!(accepted, "{deadline}");
                assert_eq!(
                    request.child_deadline(run_deadline()),
                    request.deadline.unwrap()
                );
            }
            Err(error) => {
                assert!(!accepted, "{deadline}");
                assert_eq!(error.code, DEADLINE_INVALID);
            }
        }
    }
    let mut numeric = valid();
    numeric["deadline"] = json!(1_700_000_000);
    assert_eq!(code(call(numeric)), DEADLINE_INVALID);
    assert_eq!(
        call(valid()).unwrap().child_deadline(run_deadline()),
        run_deadline()
    );
}

#[test]
fn channel_call_contract_every_result_matches_the_trusted_output_schema() {
    let (input, output) = channel_call_schemas();
    let input = jsonschema::validator_for(&input).unwrap();
    assert!(input.is_valid(&valid()));
    let output = jsonschema::validator_for(&output).unwrap();
    let child = RunId::new(Uuid::from_u128(0xC1));
    let nested = RunId::new(Uuid::from_u128(0xC2));
    let mut with_fields = reply("Approved.");
    with_fields.fields = Some(Map::from_iter([("approved".into(), json!(true))]));
    let outcomes = [
        ChannelCallOutcome::Done {
            child,
            reply: with_fields.clone(),
        },
        ChannelCallOutcome::HandedOff {
            child,
            to: nested,
            reply: reply("Finance answered."),
        },
        ChannelCallOutcome::Denied {
            code: DeniedCode::Procedure,
            reason: "Out of scope".into(),
            launched: Some((child, reply("Out of scope"))),
        },
        ChannelCallOutcome::refused(DeniedCode::UnitArchived),
        ChannelCallOutcome::refused(DeniedCode::UnitBudgetExhausted),
        ChannelCallOutcome::NoReply { child },
    ];
    for outcome in &outcomes {
        let value = outcome.to_value().unwrap();
        assert!(output.is_valid(&value), "{value}");
    }
    let done = outcomes[0].to_value().unwrap();
    assert_eq!(done["closure"], "done");
    assert_eq!(done["content"]["body"], "Approved.");
    assert_eq!(done["fields"]["approved"], true);
    let refused = outcomes[3].to_value().unwrap();
    assert_eq!(
        refused,
        json!({"closure":"denied","code":"unit.archived","reason":"The desk's unit is archived"})
    );
    assert_eq!(
        outcomes[4].to_value().unwrap()["code"],
        "budget.unit_exhausted"
    );
    let handed = outcomes[1].to_value().unwrap();
    assert_eq!(handed["run_id"], nested.as_uuid().to_string());

    for code in [
        INVALID_INPUT,
        DEADLINE_INVALID,
        TARGET_UNAVAILABLE,
        CYCLE,
        INTAKE_NOT_DECLARED,
        AUDIENCE_REFUSED,
        REVOKED,
        REVIEW_REJECTED,
        REVIEW_EXPIRED,
    ] {
        let error = ChannelCallError::new(code, "Explained").to_value();
        assert!(output.is_valid(&error), "{error}");
    }
}

#[test]
fn channel_call_contract_denied_reason_is_cut_and_oversized_results_are_refused() {
    let long = ChannelCallOutcome::Denied {
        code: DeniedCode::Procedure,
        reason: "ä".repeat(MAX_REASON_BYTES),
        launched: None,
    }
    .to_value()
    .unwrap();
    assert!(long["reason"].as_str().unwrap().len() <= MAX_REASON_BYTES);
    let huge = ChannelCallOutcome::Done {
        child: RunId::new(Uuid::from_u128(1)),
        reply: CallReply {
            content: CanonicalContent::parse("s", "b").unwrap(),
            fields: Some(Map::from_iter([(
                "blob".into(),
                json!("x".repeat(MAX_TOOL_RESULT_BYTES)),
            )])),
        },
    };
    assert!(huge.to_value().is_err());
}

#[test]
fn channel_call_contract_built_in_snapshot_is_exact_and_its_name_reserved() {
    let company = CompanyId::new(Uuid::from_u128(7));
    let trusted = builtin_channel_call_snapshot(company).unwrap();
    assert_eq!(trusted.kind(), ToolKind::ChannelCall);
    assert!(is_trusted_channel_call_snapshot(&trusted).unwrap());
    let mut widened = trusted.clone();
    widened.contract.input_schema["additionalProperties"] = json!(true);
    assert!(!is_trusted_channel_call_snapshot(&widened).unwrap());
    let mut other_effect = trusted.clone();
    other_effect.policy.effect = ActionEffect::Read;
    assert!(!is_trusted_channel_call_snapshot(&other_effect).unwrap());
    assert!(crate::application::workflow::provisioning::is_reserved_tool_name(CHANNEL_CALL));

    use crate::entities::agent_capability::AgentBuiltinTool;
    assert_eq!(
        AgentBuiltinTool::parse(CHANNEL_CALL),
        Some(AgentBuiltinTool::ChannelCall)
    );
    assert_eq!(AgentBuiltinTool::ChannelCall.name(), CHANNEL_CALL);
    assert_eq!(
        crate::application::workflow::agent::AgentToolTarget::builtin(
            AgentBuiltinTool::ChannelCall
        )
        .unwrap(),
        crate::application::workflow::agent::AgentToolTarget::ChannelCall {
            desks: Default::default()
        }
    );
}

// --- channel_call_authority_ -----------------------------------------------------------------

#[test]
fn channel_call_authority_target_checks_run_in_order_behind_the_write_rule() {
    use TargetDecision::*;
    assert_eq!(decide_target(None), Error(TARGET_UNAVAILABLE));
    // An unwritable desk hides every later fact, whatever they are (I2).
    for in_ancestry in [false, true] {
        for unit_archived in [false, true] {
            for enabled in [false, true] {
                for responder in [false, true] {
                    let desk = DeskFacts {
                        writable: false,
                        in_ancestry,
                        unit_archived,
                        enabled,
                        responder,
                    };
                    assert_eq!(decide_target(Some(desk)), Error(TARGET_UNAVAILABLE));
                }
            }
        }
    }
    let all_bad = DeskFacts {
        writable: true,
        in_ancestry: true,
        unit_archived: true,
        enabled: false,
        responder: false,
    };
    assert_eq!(decide_target(Some(all_bad)), Error(CYCLE));
    let archived = DeskFacts {
        in_ancestry: false,
        ..all_bad
    };
    assert_eq!(
        decide_target(Some(archived)),
        Denied(DeniedCode::UnitArchived)
    );
    let disabled = DeskFacts {
        unit_archived: false,
        ..archived
    };
    assert_eq!(
        decide_target(Some(disabled)),
        Denied(DeniedCode::DeskDisabled)
    );
    let no_responder = DeskFacts {
        enabled: true,
        ..disabled
    };
    assert_eq!(
        decide_target(Some(no_responder)),
        Denied(DeniedCode::NoResponder)
    );
    // Positive control: a writable, live desk with a responder proceeds.
    assert_eq!(decide_target(Some(writable())), Proceed);
}

#[test]
fn channel_call_authority_child_ceiling_is_the_lowest_of_three() {
    use AccessLevel as L;
    let levels = [L::PUBLIC, L::L5, L::MAX];
    for caller in levels {
        for allowance in levels {
            for run_as in levels {
                let ceiling = child_ceiling(caller, allowance, run_as);
                assert!(ceiling <= caller && ceiling <= allowance && ceiling <= run_as);
                assert!(ceiling == caller || ceiling == allowance || ceiling == run_as);
            }
        }
    }
    // Until request rules exist, no caller and no run_as lift a call above public data (U4).
    assert_eq!(ALLOWANCE_WITHOUT_REQUEST_RULES, L::PUBLIC);
    assert_eq!(
        child_ceiling(L::MAX, ALLOWANCE_WITHOUT_REQUEST_RULES, L::MAX),
        L::PUBLIC
    );
    // A raised allowance (plan 11 03C) lets the caller or run_as decide.
    assert_eq!(child_ceiling(L::L5, L::MAX, L::MAX), L::L5);
    assert_eq!(child_ceiling(L::MAX, L::MAX, L::L5), L::L5);
}

#[test]
fn channel_call_authority_request_above_the_ceiling_is_refused() {
    use AccessLevel as L;
    assert!(release_request(L::PUBLIC, L::PUBLIC).is_ok());
    assert!(release_request(L::L5, L::L5).is_ok());
    assert_eq!(
        release_request(L::L5, L::PUBLIC).unwrap_err().code,
        AUDIENCE_REFUSED
    );
    assert_eq!(
        release_request(L::MAX, L::L5).unwrap_err().code,
        AUDIENCE_REFUSED
    );
}

// --- channel_call_intake_ --------------------------------------------------------------------

fn intake() -> Value {
    json!({
        "type":"object","additionalProperties":false,"required":["order_id"],
        "properties":{
            "order_id":{"type":"string","description":"The order to review","maxLength":32},
            "amount":{"type":"number","description":"Order total","minimum":0},
            "rush":{"type":"boolean","description":"Whether it is urgent"},
            "region":{"enum":["eu","us"],"description":"Sales region"},
            "tags":{"type":"array","items":{"type":"string","maxLength":20},"maxItems":5,
                "description":"Labels"}
        }
    })
}

#[test]
fn channel_call_intake_schema_bound_accepts_the_documented_types() {
    assert_eq!(check_fields_schema(&intake()), Ok(()));
}

#[test]
fn channel_call_intake_schema_bound_names_the_failing_path() {
    let mut cases: Vec<(Value, &str)> = vec![];
    let mut nested = intake();
    nested["properties"]["address"] = json!({"type":"object","description":"Deep",
        "properties":{"street":{"type":"string"}}});
    cases.push((nested, "/properties/address/type"));
    let mut undescribed = intake();
    undescribed["properties"]["rush"] = json!({"type":"boolean"});
    cases.push((undescribed, "/properties/rush/description"));
    let mut numeric_enum = intake();
    numeric_enum["properties"]["region"] = json!({"enum":[1,2],"description":"Region"});
    cases.push((numeric_enum, "/properties/region/enum"));
    let mut object_items = intake();
    object_items["properties"]["tags"]["items"] = json!({"type":"object"});
    cases.push((object_items, "/properties/tags/items"));
    let mut unknown_keyword = intake();
    unknown_keyword["properties"]["order_id"]["$ref"] = json!("#/x");
    cases.push((unknown_keyword, "/properties/order_id/$ref"));
    let mut root_keyword = intake();
    root_keyword["oneOf"] = json!([]);
    cases.push((root_keyword, "/oneOf"));
    let mut undeclared_required = intake();
    undeclared_required["required"] = json!(["missing"]);
    cases.push((undeclared_required, "/required"));
    let mut too_many = intake();
    too_many["properties"] = Value::Object(
        (0..=MAX_FIELDS_PROPERTIES)
            .map(|i| (format!("f{i}"), json!({"type":"string","description":"x"})))
            .collect(),
    );
    cases.push((too_many, "/properties"));
    let mut oversized = intake();
    oversized["properties"]["order_id"]["description"] = json!("d".repeat(MAX_DESCRIPTION_CHARS));
    oversized["description"] = json!("x".repeat(MAX_FIELDS_SCHEMA_BYTES));
    cases.push((oversized, ""));
    cases.push((json!({"type":"string"}), "/type"));
    for (schema, path) in cases {
        let (failed, _) = check_fields_schema(&schema).unwrap_err();
        assert_eq!(failed, path, "{schema}");
    }
}

#[test]
fn channel_call_intake_call_fields_are_checked_against_the_captured_intake() {
    let fields = |value: Value| value.as_object().cloned().unwrap();
    assert_eq!(check_intake(None, None).unwrap(), Ok(()));
    assert_eq!(check_intake(Some(&intake()), None).unwrap(), Ok(()));
    assert_eq!(
        check_intake(None, Some(&fields(json!({"order_id":"42"}))))
            .unwrap()
            .unwrap_err()
            .code,
        INTAKE_NOT_DECLARED
    );
    assert_eq!(
        check_intake(
            Some(&intake()),
            Some(&fields(json!({"order_id":"42","rush":true})))
        )
        .unwrap(),
        Ok(())
    );
    let error = check_intake(
        Some(&intake()),
        Some(&fields(json!({"amount":"lots","region":"asia"}))),
    )
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, INTAKE_INVALID);
    let (schema, paths) = error.intake.clone().unwrap();
    assert_eq!(schema, intake());
    for path in ["", "/amount", "/region"] {
        assert!(paths.iter().any(|p| p == path), "{path} in {paths:?}");
    }
    let value = error.to_value();
    assert_eq!(value["error"]["code"], INTAKE_INVALID);
    assert_eq!(value["error"]["schema"], intake());
    let (_, output) = channel_call_schemas();
    assert!(jsonschema::validator_for(&output).unwrap().is_valid(&value));
}

#[test]
fn channel_call_intake_mail_input_has_no_fields_and_a_call_input_reaches_input_fields() {
    // The canonical schema declares no intake; a responder adds `properties.fields` itself.
    let canonical = MessageTriggerInput::schema();
    assert_eq!(declared_fields(Some(&canonical)), None);
    let canonical = jsonschema::validator_for(&canonical).unwrap();
    let mut responder = MessageTriggerInput::schema();
    responder["properties"]["fields"] = intake();
    assert_eq!(declared_fields(Some(&responder)), Some(&intake()));
    let responder = jsonschema::validator_for(&responder).unwrap();

    let source = CanonicalMessageId::new(Uuid::from_u128(9));
    let mail = MessageTriggerInput::new(source, "body", "subject", "a@example.com");
    assert!(mail.to_value().get("fields").is_none());
    assert!(
        MessageTriggerInput::representative()
            .get("fields")
            .is_none()
    );
    assert!(canonical.is_valid(&mail.to_value()));
    assert!(responder.is_valid(&mail.to_value()));
    let call = MessageTriggerInput::new(source, "body", "subject", "agent@desk")
        .with_fields(Some(Map::from_iter([("order_id".into(), json!("42"))])));
    let value = call.to_value();
    assert_eq!(value["fields"], json!({"order_id":"42"}));
    assert!(responder.is_valid(&value));
    // A desk without an intake cannot receive fields; the call is refused before this point.
    assert!(!canonical.is_valid(&value));
}

#[test]
fn channel_call_intake_call_becomes_the_childs_message_input() {
    let mut arguments = valid();
    arguments["fields"] = json!({"order_id":"42"});
    let source = CanonicalMessageId::new(Uuid::from_u128(11));
    let input = call(arguments)
        .unwrap()
        .trigger_input(source, "lead@acme.mailagents.com");
    assert_eq!(
        input.to_value(),
        json!({"source_message":source.as_uuid().to_string(),"message":"Review order 42.",
            "subject":"Order review","sender":"lead@acme.mailagents.com",
            "fields":{"order_id":"42"}})
    );
    let without = call(valid())
        .unwrap()
        .trigger_input(source, "lead@acme.mailagents.com");
    assert!(without.to_value().get("fields").is_none());
}

#[test]
fn channel_call_intake_declared_fields_and_required_rule() {
    let input = json!({"type":"object","properties":{"fields":intake()}});
    assert_eq!(declared_fields(Some(&input)), Some(&intake()));
    assert_eq!(declared_fields(None), None);
    assert!(!fields_required(Some(&input)));
    let required = json!({"type":"object","required":["message","fields"],
        "properties":{"fields":intake()}});
    assert!(fields_required(Some(&required)));
}
