use crate::app_error::{AppError, AppResult};
use crate::application::agent::{ChannelFinalOrigin, ToolCallOrigin};
use crate::application::workflow::actions::*;
use crate::application::workflow::message::{MessageKind, builtin_message_snapshot};
use crate::domain::workflow::*;
use async_trait::async_trait;
use serde_json::json;
use uuid::Uuid;

fn request() -> ActionRequest {
    let scope = ActionScope {
        company: CompanyId::new(Uuid::new_v4()),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
    };
    let mut request = crate::application::workflow::actions::tests::request(scope);
    request.operation_key = TypeName::parse("message.reply").unwrap();
    request.contract = builtin_message_snapshot(scope.company, MessageKind::Reply).unwrap();
    request.target = ActionTarget::Local {
        resource: scope.company.as_uuid(),
        resource_kind: TypeName::parse("messaging").unwrap(),
    };
    request.arguments =
        json!({"source_message":Uuid::new_v4(),"content":{"subject":"Status","body":"Done"}});
    request.context.capability_ceiling = vec![TypeName::parse("message.reply").unwrap()];
    request.context.origin = ToolCallOrigin::ChannelFinal(ChannelFinalOrigin {
        proposal: ReplyProposalId::new(Uuid::new_v4()),
        integrity: ReplyIntegrity::from_sha256([3; 32]),
    });
    request
}

#[test]
fn channel_reply_completion_origin_roundtrip_and_integrity_are_strict() {
    let request = request();
    let wire = serde_json::to_value(request.context.origin).unwrap();
    assert_eq!(wire["kind"], "channel_final");
    assert_eq!(wire["integrity"], "03".repeat(32));
    assert_eq!(
        serde_json::from_value::<ToolCallOrigin>(wire.clone()).unwrap(),
        request.context.origin
    );
    for digest in ["a".repeat(63), "A".repeat(64), "z".repeat(64)] {
        let mut changed = wire.clone();
        changed["integrity"] = json!(digest);
        assert!(serde_json::from_value::<ToolCallOrigin>(changed).is_err());
    }
    let mut nil = wire;
    nil["proposal"] = json!(Uuid::nil());
    assert!(
        serde_json::from_value::<ToolCallOrigin>(nil)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn channel_reply_completion_origin_binds_immutable_proposal_to_effect_identity() {
    let original = request();
    let first = FrozenAction::channel_final(original.clone()).unwrap();
    let restored = FrozenAction::restore(first.saved_operation().clone()).unwrap();
    assert_eq!(restored.idempotency_key(), first.idempotency_key());
    let mut changed = original;
    let ToolCallOrigin::ChannelFinal(mut origin) = changed.context.origin else {
        panic!()
    };
    origin.integrity = ReplyIntegrity::from_sha256([4; 32]);
    changed.context.origin = ToolCallOrigin::ChannelFinal(origin);
    assert_ne!(
        FrozenAction::channel_final(changed)
            .unwrap()
            .idempotency_key(),
        first.idempotency_key()
    );
}

#[test]
fn channel_reply_completion_origin_is_restricted_to_the_trusted_company_reply_contract() {
    for edit in 0..4 {
        let mut changed = request();
        match edit {
            0 => changed.context.origin = ToolCallOrigin::Workflow,
            1 => {
                changed.target = ActionTarget::Local {
                    resource: Uuid::new_v4(),
                    resource_kind: TypeName::parse("messaging").unwrap(),
                }
            }
            2 => changed
                .context
                .capability_ceiling
                .push(TypeName::parse("http.request").unwrap()),
            _ => changed.operation_key = TypeName::parse("message.send").unwrap(),
        }
        assert!(FrozenAction::channel_final(changed).is_err());
    }
}

struct NoOrdinaryIntent;
#[async_trait]
impl ActionIntents for NoOrdinaryIntent {
    async fn prepare(
        &self,
        _: &FrozenAction,
        _: Option<&ModelToolCallId>,
    ) -> AppResult<ActionIntent> {
        panic!("final origin reached an ordinary action store")
    }
}
#[tokio::test]
async fn channel_reply_completion_origin_cannot_enter_step_or_model_tool_preparation() {
    let service = ActionService::new(NoOrdinaryIntent);
    assert!(matches!(
        service.prepare_step(request()).await,
        Err(AppError::BadRequest(_))
    ));
    assert!(matches!(
        service
            .prepare_tool(request(), ModelToolCallId::parse("call").unwrap())
            .await,
        Err(AppError::BadRequest(_))
    ));
}
