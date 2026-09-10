use super::*;
use crate::{
    entities::response_contract::{ContractUpdate, MAX_CONTRACT_BYTES},
    services::response_contract::{InvalidResponse, StructuredResponse},
};
use serde_json::json;

fn contract(schema: Value) -> ResponseContract {
    serde_json::from_value(json!({"version":1,"format":"json_schema","schema":schema})).unwrap()
}

#[test]
fn documented_rig_configuration_and_response_contracts_pass_application_parsers() {
    use crate::entities::harness::{HarnessConfig, HarnessKind};

    let guide = include_str!("../../../docs/rig.md");
    let mut examples = 0;
    for block in guide.split("```json\n").skip(1) {
        let source = block.split_once("\n```").expect("closed JSON example").0;
        let example: Value = serde_json::from_str(source).expect("valid documented JSON");
        if let Some(kind) = example.get("harness_kind") {
            let kind: HarnessKind = serde_json::from_value(kind.clone()).unwrap();
            assert_eq!(kind, HarnessKind::Rig);
            let config = HarnessConfig::parse(kind, example.get("config_json")).unwrap();
            assert_eq!(config.rig().unwrap().effective_max_turns(16), 8);
        } else {
            let contract = ResponseContract::parse(&example["response_contract"].to_string())
                .expect("documented response contract envelope");
            contract.require_harness(HarnessKind::Rig).unwrap();
            assert!(contract.require_harness(HarnessKind::AiAgents).is_err());
            JsonResponseValidator.validate_contract(&contract).unwrap();
            let valid = r#"{"status":"done","summary":"The request is complete."}"#;
            let output =
                StructuredResponse::validate(&contract, valid, None, &JsonResponseValidator)
                    .expect("response validator available")
                    .expect("documented answer passes final validation");
            assert_eq!(output.body(), valid);
            assert!(
                !JsonResponseValidator
                    .validate_value(&contract, &json!({"status":"invalid","summary":"x"}))
                    .unwrap()
            );
        }
        examples += 1;
    }
    assert_eq!(
        examples, 3,
        "keep the published configuration/schema examples covered"
    );
}

#[test]
fn contract_envelope_patch_and_fingerprint_are_stable() {
    #[derive(serde::Deserialize)]
    struct Patch {
        #[serde(default)]
        response_contract: ContractUpdate,
    }
    assert!(
        serde_json::from_str::<Patch>("{}")
            .unwrap()
            .response_contract
            .0
            .is_none()
    );
    assert_eq!(
        serde_json::from_str::<Patch>(r#"{"response_contract":null}"#)
            .unwrap()
            .response_contract
            .0,
        Some(None)
    );
    let valid = json!({"version":1,"format":"json_schema","schema":{"type":"object","properties":{"b":{"type":"string"},"a":{"type":"integer"}}}});
    let parsed: ResponseContract = serde_json::from_value(valid.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), valid);
    assert_eq!(
        ResponseContract::parse(&valid.to_string())
            .unwrap()
            .fingerprint(),
        parsed.fingerprint()
    );
    for bad in [
        json!({"version":2,"format":"json_schema","schema":true}),
        json!({"version":1,"format":"json","schema":true}),
        json!({"version":1,"format":"json_schema","schema":true,"repair":2}),
    ] {
        assert!(serde_json::from_value::<ResponseContract>(bad).is_err());
    }
    assert!(ResponseContract::parse(&" ".repeat(MAX_CONTRACT_BYTES + 1)).is_err());
    assert!(
        parsed
            .require_harness(crate::entities::harness::HarnessKind::AiAgents)
            .is_err()
    );
}
#[test]
fn schemas_are_validated_without_external_or_recursive_resolution() {
    for schema in [
        json!({"type":42}),
        json!({"$schema":"http://json-schema.org/draft-07/schema#"}),
        json!({"$ref":"https://example.test/schema"}),
        json!({"$ref":"file:///etc/passwd"}),
        json!({"$ref":"#"}),
        json!({"$defs":{"a":{"$ref":"#/$defs/a"}},"$ref":"#/$defs/a"}),
        json!({"pattern":"["}),
    ] {
        assert!(
            JsonResponseValidator
                .validate_contract(&contract(schema))
                .is_err()
        );
    }
    let local = contract(json!({"$defs":{"status":{"type":"string"}},"$ref":"#/$defs/status"}));
    JsonResponseValidator.validate_contract(&local).unwrap();
    assert!(
        JsonResponseValidator
            .validate_value(&local, &json!("done"))
            .unwrap()
    );
    let deep = (0..34).fold(json!(true), |child, _| json!({"allOf":[child]}));
    assert!(
        JsonResponseValidator
            .validate_contract(&contract(deep))
            .is_err()
    );
    let nodes = contract(json!({"enum":(0..4097).collect::<Vec<_>>()}));
    assert!(JsonResponseValidator.validate_contract(&nodes).is_err());
    assert!(
        serde_json::from_value::<ResponseContract>(
            json!({"version":1,"format":"json_schema","schema":{"description":"x".repeat(65_536)}})
        )
        .is_err()
    );
}
#[test]
fn final_payload_requires_exact_json_and_validates_after_sanitization() {
    let schema = contract(
        json!({"type":"object","properties":{"status":{"const":"done"}},"required":["status"],"additionalProperties":false}),
    );
    for candidate in [
        "```json\n{\"status\":\"done\"}\n```",
        "{\"status\":\"done\"} trailing",
        "{} {}",
        "prefix {}",
        "{",
        "{\"status\":1}",
        "{}",
        "{\"status\":\"done\",\"extra\":1}",
    ] {
        assert!(
            StructuredResponse::validate(&schema, candidate, None, &JsonResponseValidator)
                .unwrap()
                .is_err()
        );
    }
    let valid = StructuredResponse::validate(
        &schema,
        " { \"status\": \"done\" } ",
        None,
        &JsonResponseValidator,
    )
    .unwrap()
    .unwrap();
    assert_eq!(valid.body(), r#"{"status":"done"}"#);
    valid.verify(valid.body(), &JsonResponseValidator).unwrap();
    assert!(valid.verify("{}", &JsonResponseValidator).is_err());
    let secret = contract(json!({"const":"secret-key"}));
    assert_eq!(
        StructuredResponse::validate(
            &secret,
            r#""secret-key""#,
            Some("secret-key"),
            &JsonResponseValidator
        )
        .unwrap(),
        Err(InvalidResponse::SchemaMismatch)
    );
    assert_eq!(
        StructuredResponse::validate(&schema, &"x".repeat(65_537), None, &JsonResponseValidator)
            .unwrap(),
        Err(InvalidResponse::OutputLimit)
    );
}
