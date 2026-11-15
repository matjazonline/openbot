use super::*;
use crate::application::agent::{ActionEffect, ActionRecovery, ApprovedActionPolicy};
use crate::domain::workflow::TypeName;
use crate::entities::agent_capability::{AgentBindingId, AgentToolAlias, AgentToolBinding};
use uuid::Uuid;

fn identity() -> NativeBrokerIdentity {
    NativeBrokerIdentity {
        company: CompanyId::new(Uuid::new_v4()),
        actor: WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
        workflow: WorkflowId::new(Uuid::new_v4()),
        version: VersionId::new(Uuid::new_v4()),
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    }
}

fn tool(approval_required: bool) -> ResolvedAgentTool {
    ResolvedAgentTool {
        binding: AgentToolBinding {
            id: AgentBindingId::new(Uuid::new_v4()),
            alias: AgentToolAlias::parse("lookup").unwrap(),
            source: AgentBindingSource::Mcp {
                connection: Uuid::new_v4(),
                operation: TypeName::parse("lookup").unwrap(),
                reviewed_version: Uuid::new_v4(),
                remote_name: "remote_lookup".to_owned().try_into().unwrap(),
            },
        },
        input_schema: json!({"type":"object","additionalProperties":false,"required":["key"],
            "properties":{"key":{"type":"integer","minimum":0}}}),
        output_schema: json!({"type":"object"}),
        policy: ApprovedActionPolicy {
            capability: TypeName::parse("mcp.call").unwrap(),
            policy_revision: 1,
            effect: ActionEffect::Write,
            recovery: ActionRecovery::Reconcile,
        },
        approval_required,
    }
}

#[test]
fn native_broker_compiles_real_mcp_contract_and_binds_original_reviewer() {
    for protected in [false, true] {
        let id = identity();
        let actor = id.actor.user_id();
        let tool = tool(protected);
        let prepared = PreparedNativeMcp::compile(
            &crate::adapters::workflow_source::WorkflowSourceDecoder,
            id,
            &tool,
            &json!({"key":18446744073709551615_u64}),
            chrono::Utc::now(),
        )
        .unwrap();
        let bundle = prepared.binding().bundle();
        assert_eq!(bundle.compiled().graph().definition().steps.len(), 1);
        let step = &bundle.compiled().graph().definition().steps
            [&crate::domain::workflow::StepId::parse("start").unwrap()];
        assert_eq!(step.step_type.as_str(), "mcp.call");
        let source: Value = serde_json::from_str(bundle.compiled().source()).unwrap();
        assert_eq!(
            source["steps"]["start"]["with"]["approval"].is_object(),
            protected
        );
        if protected {
            assert_eq!(
                source["steps"]["start"]["with"]["approval"]["object"]["reviewers"]["literal"][0]["id"],
                json!(actor)
            );
        }
        assert_eq!(
            prepared.input()["arguments"]["key"],
            json!(18446744073709551615_u64)
        );
        assert_eq!(prepared.argument_digest().len(), 64);
    }
}

#[test]
fn native_broker_refuses_invalid_arguments_and_non_mcp_sources() {
    let decoder = crate::adapters::workflow_source::WorkflowSourceDecoder;
    for args in [
        json!([]),
        json!({}),
        json!({"key":"wrong"}),
        json!({"key":1,"reviewers":[]}),
        json!({"key":1,"padding":"x".repeat(MAX_ARGUMENT_BYTES)}),
    ] {
        assert!(
            PreparedNativeMcp::compile(
                &decoder,
                identity(),
                &tool(false),
                &args,
                chrono::Utc::now()
            )
            .is_err()
        );
    }
    let mut invalid = tool(false);
    invalid.binding.source = AgentBindingSource::Http {
        connection: Uuid::new_v4(),
        operation: TypeName::parse("lookup").unwrap(),
    };
    assert!(
        PreparedNativeMcp::compile(
            &decoder,
            identity(),
            &invalid,
            &json!({"key":1}),
            chrono::Utc::now()
        )
        .is_err()
    );
}
#[test]
fn native_broker_refuses_postgres_nul_values_and_keys_before_publication() {
    use super::canonical_arguments;
    for arguments in [
        json!({"query":"bad\0value"}),
        json!({"bad\0key":true}),
        json!({"nested":[{"value":"bad\0value"}]}),
    ] {
        assert!(canonical_arguments(&arguments).is_err());
    }
    let valid = json!({"query":"literal \\u0000 and 🌱"});
    assert_eq!(canonical_arguments(&valid).unwrap(), valid);
}
