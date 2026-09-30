use super::*;
use crate::application::workflow::publication::{
    ActionEffect, ActionRecovery, ApprovedActionPolicy, ToolSnapshot,
};
use crate::application::workflow::registry::ToolContract;
use crate::domain::workflow::TypeName;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) fn request(scope: ActionScope) -> ActionRequest {
    ActionRequest {
        scope,
        operation_key: TypeName::parse("call").unwrap(),
        target: ActionTarget::Local {
            resource: scope.execution.as_uuid(),
            resource_kind: TypeName::parse("fixture").unwrap(),
        },
        contract: ToolSnapshot {
            company_id: scope.company,
            contract: ToolContract {
                name: TypeName::parse("fixture.write").unwrap(),
                connection: None,
                input_schema: json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"],"additionalProperties":false}),
                output_schema: json!({"type":"object"}),
            },
            policy: ApprovedActionPolicy {
                capability: TypeName::parse("fixture.write").unwrap(),
                policy_revision: 1,
                effect: ActionEffect::Write,
                recovery: ActionRecovery::Reconcile,
            },
        },
        arguments: json!({"value":1}),
        context: ActionPolicyContext {
            actor: Uuid::new_v4(),
            capability_ceiling: vec![TypeName::parse("fixture.write").unwrap()],
            approval_required: false,
        },
    }
}

fn scope() -> ActionScope {
    use crate::domain::workflow::{CompanyId, ExecutionId, RunId};
    ActionScope {
        company: CompanyId::new(Uuid::new_v4()),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
    }
}

#[test]
fn workflow_action_freeze_canonical_identity_binds_operation_target_and_contract() {
    let original = request(scope());
    let first = freeze::freeze(original.clone()).unwrap();
    let restored = FrozenAction::restore(first.saved_operation().clone()).unwrap();
    assert_eq!(first.idempotency_key(), restored.idempotency_key());
    for change in 0..5 {
        let mut changed = original.clone();
        match change {
            0 => changed.arguments = json!({"value":2}),
            1 => {
                changed.target = ActionTarget::Local {
                    resource: Uuid::new_v4(),
                    resource_kind: TypeName::parse("fixture").unwrap(),
                }
            }
            2 => changed.contract.policy.policy_revision = 2,
            3 => changed.contract.contract.name = TypeName::parse("fixture.other").unwrap(),
            _ => changed.operation_key = TypeName::parse("other").unwrap(),
        }
        assert_ne!(
            first.idempotency_key(),
            freeze::freeze(changed).unwrap().idempotency_key()
        );
    }
    let mut context = original;
    context.context.approval_required = true;
    let changed = freeze::freeze(context).unwrap();
    assert_eq!(first.idempotency_key(), changed.idempotency_key());
    assert_ne!(first.saved_operation(), changed.saved_operation());
    assert_eq!(changed.decision(), ActionPolicyDecision::ApprovalRequired);
}

#[test]
fn workflow_action_freeze_rejects_schema_scope_depth_bytes_and_untrusted_restore() {
    for change in 0..7 {
        let mut value = request(scope());
        match change {
            0 => value.arguments = json!({"value":"secret-invalid"}),
            1 => {
                value.contract.company_id = crate::domain::workflow::CompanyId::new(Uuid::new_v4())
            }
            2 => value.contract.policy.policy_revision = 0,
            3 => {
                value.contract.contract.input_schema =
                    json!({"$ref":"https://invalid.example/schema"})
            }
            4 => value
                .context
                .capability_ceiling
                .push(value.context.capability_ceiling[0].clone()),
            5 => {
                value.contract.contract.input_schema = json!(true);
                value.arguments = json!("x".repeat(65_536));
            }
            _ => {
                value.contract.contract.input_schema = json!(true);
                let mut deep = Value::Null;
                for _ in 0..34 {
                    deep = json!([deep]);
                }
                value.arguments = deep;
            }
        }
        assert!(freeze::freeze(value).is_err());
    }
    assert!(FrozenAction::restore(json!({"version":99})).is_err());
    assert!(ModelToolCallId::parse("\nunsafe").is_err());
    assert!(ModelToolCallId::parse("x".repeat(129)).is_err());
}

#[test]
fn workflow_action_freeze_object_order_is_canonical_and_array_order_is_significant() {
    let mut original = request(scope());
    original.contract.contract.input_schema = json!(true);
    original.arguments = serde_json::from_str(r#"{"z":[1,2],"a":{"b":true,"a":null}}"#).unwrap();
    let first = freeze::freeze(original.clone()).unwrap();
    original.arguments = serde_json::from_str(r#"{"a":{"a":null,"b":true},"z":[1,2]}"#).unwrap();
    assert_eq!(
        first.argument_digest(),
        freeze::freeze(original.clone()).unwrap().argument_digest()
    );
    original.arguments["z"] = json!([2, 1]);
    assert_ne!(
        first.argument_digest(),
        freeze::freeze(original).unwrap().argument_digest()
    );
}
