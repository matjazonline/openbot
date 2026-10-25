use super::*;
use crate::application::workflow::publication::ToolKind;
use crate::domain::workflow::ResourceName;
use uuid::Uuid;

fn span() -> SourceSpan {
    SourceSpan {
        start: 0,
        end: 0,
        line: 1,
        column: 1,
    }
}

fn valid(schema: &Value, value: &Value) -> bool {
    Schema::compile(schema.clone(), "/schema", span())
        .unwrap()
        .validate(value, "/value", span())
        .is_ok()
}

fn company() -> CompanyId {
    CompanyId::new(Uuid::from_u128(7))
}

/// D2.1: the built-ins are the trusted snapshots under their names, a changed one is not, and
/// only a name-only snapshot classifies as provisioning: an MCP tool of that name stays a slot
/// tool.
#[test]
fn only_the_exact_name_only_built_in_is_trusted() {
    for kind in [
        ProvisioningKind::ListAgents,
        ProvisioningKind::CreateAgentChannel,
    ] {
        let trusted = builtin_provisioning_snapshot(company(), kind).unwrap();
        assert_eq!(trusted.kind(), ToolKind::Provisioning);
        assert!(is_trusted_provisioning_snapshot(&trusted).unwrap());

        let mut widened = trusted.clone();
        widened.contract.input_schema = json!(true);
        assert!(!is_trusted_provisioning_snapshot(&widened).unwrap());

        let mut approved = trusted.clone();
        approved.policy.effect = ActionEffect::Read;
        approved.policy.capability = TypeName::parse("other").unwrap();
        assert!(!is_trusted_provisioning_snapshot(&approved).unwrap());

        let mut mcp = trusted.clone();
        mcp.contract.connection = Some(ResourceName::parse("crm").unwrap());
        assert!(matches!(mcp.kind(), ToolKind::Slot(_)));
        assert!(!is_trusted_provisioning_snapshot(&mcp).unwrap());
    }
    assert_eq!(
        builtin_provisioning_snapshot(company(), ProvisioningKind::CreateAgentChannel)
            .unwrap()
            .policy
            .effect,
        ActionEffect::Write
    );
}

#[test]
fn every_built_in_name_is_reserved() {
    for name in [
        "message.send",
        "message.reply",
        LIST_COMPANY_AGENTS,
        CREATE_AGENT_CHANNEL,
    ] {
        assert!(is_reserved_tool_name(name), "{name}");
    }
    assert!(!is_reserved_tool_name("crm.lookup"));
    assert!(!is_builtin_provisioning_tool("message.send"));
}

/// A committed refusal and a success are both valid receipts: the output schema is one of the
/// two shapes, and nothing else.
#[test]
fn refusals_and_results_are_both_valid_receipts() {
    let (_, create) = provisioning_tool_schemas(ProvisioningKind::CreateAgentChannel);
    for refusal in [
        ProvisioningRefusal::Forbidden,
        ProvisioningRefusal::Conflict("taken".into()),
        ProvisioningRefusal::Invalid("x".repeat(5000)),
    ] {
        let error = refusal.tool_error();
        assert!(valid(&create, &error), "{error}");
        assert_eq!(error["error"]["code"], refusal.code());
    }
    let id = Uuid::nil().to_string();
    let created = json!({
        "agent": {"id": id, "slug": "helper"},
        "channel": {"id": id, "slug": "helper", "address": "helper@acme"},
        "workflow_id": id, "version_id": id, "binding_id": id, "reply_mode": "reviewed"
    });
    assert!(valid(&create, &created));
    assert!(!valid(
        &create,
        &json!({"agent": {"id": id, "slug": "helper"}})
    ));
    assert!(!valid(
        &create,
        &json!({"error": {"code": "workflow.other", "message": "x"}})
    ));

    let (_, list) = provisioning_tool_schemas(ProvisioningKind::ListAgents);
    let listed = json!({
        "agents": [{"slug": "a", "name": "A", "description": null, "provider": null, "model": null}],
        "models": [{"provider": "anthropic", "model": "claude"}],
        "truncated": false
    });
    assert!(valid(&list, &listed));
    let mut long = listed.clone();
    long["agents"][0]["name"] = json!("n".repeat(MAX_LISTED_NAME_CHARS + 1));
    assert!(!valid(&list, &long));
    assert!(valid(&list, &ProvisioningRefusal::Forbidden.tool_error()));
}

#[test]
fn arguments_are_held_to_the_input_contract() {
    let (input, _) = provisioning_tool_schemas(ProvisioningKind::CreateAgentChannel);
    let spec = json!({
        "name": "Helper", "slug": "helper", "system_prompt": "Help.",
        "provider": "anthropic", "model": "claude", "reply_mode": "autonomous",
        "tools": ["crm.lookup"], "skills": ["support"]
    });
    assert!(valid(&input, &spec));
    let parsed = CreateAgentChannel::parse(&spec).unwrap();
    assert_eq!(parsed.reply_mode, ReplyMode::Autonomous);
    assert_eq!(parsed.channel_slug, None);

    let mut unknown = spec.clone();
    unknown["owner"] = json!("someone");
    assert!(!valid(&input, &unknown));
    assert!(CreateAgentChannel::parse(&unknown).is_err());
    let mut mode = spec.clone();
    mode["reply_mode"] = json!("public");
    assert!(!valid(&input, &mode));
    let mut missing = spec;
    missing.as_object_mut().unwrap().remove("model");
    assert!(!valid(&input, &missing));

    let (input, _) = provisioning_tool_schemas(ProvisioningKind::ListAgents);
    assert!(valid(&input, &json!({})));
    assert!(!valid(&input, &json!({"company": "other"})));
}

/// p08 D2.1: no company HTTP operation may take a built-in's name.
#[test]
fn http_operations_cannot_take_a_built_in_name() {
    use crate::application::workflow::http::HttpOperation;
    for tool in [
        builtin_provisioning_snapshot(company(), ProvisioningKind::CreateAgentChannel).unwrap(),
        builtin_provisioning_snapshot(company(), ProvisioningKind::ListAgents).unwrap(),
        crate::application::workflow::message::builtin_message_snapshot(
            company(),
            crate::application::workflow::message::MessageKind::Send,
        )
        .unwrap(),
    ] {
        let mut renamed = tool.clone();
        renamed.contract.name = TypeName::parse("crm.lookup").unwrap();
        assert!(
            HttpOperation {
                tool: renamed,
                approval_required: false
            }
            .validate(company())
            .is_ok()
        );
        let refused = HttpOperation {
            tool,
            approval_required: false,
        }
        .validate(company())
        .unwrap_err();
        assert!(refused.to_string().contains("reserved"), "{refused}");
    }
}
