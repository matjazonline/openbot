use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::workflow::authorization::WorkflowActor;
use crate::application::workflow::binding::{ResourceDirectory, ResourceReadiness, ResourceStatus};
use crate::application::workflow::{publication, registry};
use crate::domain::workflow::{
    ActionInvocationId, CompanyId, ExecutionId, RunId, RuntimeResourceId, TypeName, VersionId,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

struct Store(ActionRequest);
fn request() -> ActionRequest {
    super::tests::request(ActionScope {
        company: CompanyId::new(Uuid::from_u128(10)),
        run: RunId::new(Uuid::from_u128(20)),
        execution: ExecutionId::new(Uuid::from_u128(30)),
    })
}
fn authority(request: ActionRequest) -> ActionRunAuthority {
    let mut source: serde_json::Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let mut resources = std::collections::BTreeMap::new();
    if let (
        Some(slot),
        ActionTarget::Connection {
            resource,
            resource_kind,
        },
    ) = (&request.contract.contract.connection, &request.target)
    {
        source["resources"] = json!([{"slot":slot,"kind":resource_kind}]);
        resources.insert(slot.clone(), RuntimeResourceId::new(*resource));
    }
    let bundle = publication::freeze(
        crate::adapters::workflow_source::decode(&source.to_string()).unwrap(),
        request.scope.company,
        VersionId::new(Uuid::from_u128(40)),
        publication::DependencySnapshots {
            tools: vec![request.contract.clone()],
            ..Default::default()
        },
        vec![],
    )
    .unwrap();
    ActionRunAuthority {
        actor: WorkflowActor::authenticated(request.context.actor).unwrap(),
        action: freeze::freeze(request).unwrap(),
        bundle: Arc::new(bundle),
        resources,
    }
}
fn subject(request: &ActionRequest) -> ApprovalSubject {
    ApprovalSubject {
        invocation: ActionInvocationId::new(Uuid::from_u128(50)),
        argument_digest: freeze::freeze(request.clone())
            .unwrap()
            .argument_digest()
            .clone(),
    }
}
#[async_trait]
impl ActionAuthorities for Store {
    async fn action_authority(
        &self,
        _: ActionScope,
        _: &ApprovalSubject,
    ) -> AppResult<ActionRunAuthority> {
        Ok(authority(self.0.clone()))
    }
}
struct Directory {
    request: ActionRequest,
    mode: u8,
}
#[async_trait]
impl ResourceDirectory for Directory {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        assert_eq!(company, self.request.scope.company);
        assert_eq!(actor.user_id(), self.request.context.actor);
        if self.mode == 1 {
            return Err(AppError::Database("resource outage".into()));
        }
        if self.mode == 2 {
            return Ok(None);
        }
        if self.mode == 11 {
            return std::future::pending().await;
        }
        let mut status = ResourceStatus {
            id,
            company_id: company,
            kind: TypeName::parse("fixture").unwrap(),
            supported_contracts: [self.request.contract.contract.name.clone()].into(),
            authorized: true,
            readiness: ResourceReadiness::Ready,
        };
        match self.mode {
            3 => status.authorized = false,
            4 => status.readiness = ResourceReadiness::Revoked,
            5 => status.company_id = CompanyId::new(Uuid::new_v4()),
            6 => status.id = RuntimeResourceId::new(Uuid::new_v4()),
            7 => status.supported_contracts.clear(),
            8 => status.kind = TypeName::parse("other").unwrap(),
            9 => status.readiness = ResourceReadiness::Unavailable,
            10 => {
                status.supported_contracts = (0..257)
                    .map(|n| TypeName::parse(format!("t{n}")).unwrap())
                    .collect()
            }
            _ => {}
        }
        Ok(Some(status))
    }
}
struct Policies {
    request: ActionRequest,
    mode: u8,
}
#[async_trait]
impl ActionPolicyDirectory for Policies {
    async fn action_policy(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        target: &ActionTarget,
        tool: &TypeName,
    ) -> AppResult<Option<CurrentActionPolicy>> {
        assert_eq!(company, self.request.scope.company);
        assert_eq!(actor.user_id(), self.request.context.actor);
        assert_eq!(target, &self.request.target);
        assert_eq!(tool, &self.request.contract.contract.name);
        if self.mode == 1 {
            return Err(AppError::Database("policy outage".into()));
        }
        if self.mode == 2 {
            return Ok(None);
        }
        if self.mode == 9 {
            return std::future::pending().await;
        }
        let mut policy = CurrentActionPolicy {
            tool: self.request.contract.clone(),
            approval_required: self.mode == 3,
        };
        match self.mode {
            4 => policy.tool.policy.policy_revision += 1,
            5 => policy.tool.policy.capability = TypeName::parse("broader").unwrap(),
            6 => policy.tool.contract.output_schema = json!(false),
            7 => policy.tool.contract.input_schema = json!({"description":"x".repeat(65_536)}),
            8 => policy.tool.company_id = CompanyId::new(Uuid::new_v4()),
            _ => {}
        }
        Ok(Some(policy))
    }
}

#[tokio::test]
async fn workflow_action_authorization_current_revocation_errors_policy_and_approval() {
    let request = request();
    let service = ActionService::new(Store(request.clone()));
    let subject = subject(&request);
    let good = Directory {
        request: request.clone(),
        mode: 0,
    };
    for mode in 0..=10 {
        let resources = Directory {
            request: request.clone(),
            mode,
        };
        let policies = Policies {
            request: request.clone(),
            mode: 0,
        };
        let result = service
            .authorize(request.scope, &subject, &resources, &policies)
            .await;
        if mode == 0 {
            assert_eq!(
                result.unwrap(),
                ActionAccessDecision::CurrentAccessConfirmed
            );
        } else if mode == 1 {
            assert!(matches!(result, Err(AppError::Database(_))));
        } else {
            assert!(result.is_err());
        }
    }
    for mode in 0..=8 {
        let policies = Policies {
            request: request.clone(),
            mode,
        };
        let result = service
            .authorize(request.scope, &subject, &good, &policies)
            .await;
        match mode {
            0 => assert_eq!(
                result.unwrap(),
                ActionAccessDecision::CurrentAccessConfirmed
            ),
            3 => assert_eq!(result.unwrap(), ActionAccessDecision::ApprovalRequired),
            1 => assert!(matches!(result, Err(AppError::Database(_)))),
            _ => assert!(result.is_err()),
        }
    }
    let mut protected = request;
    protected.context.approval_required = true;
    let service = ActionService::new(Store(protected.clone()));
    assert_eq!(
        service
            .authorize(
                protected.scope,
                &subject,
                &good,
                &Policies {
                    request: protected,
                    mode: 0
                }
            )
            .await
            .unwrap(),
        ActionAccessDecision::ApprovalRequired
    );
}

#[test]
fn workflow_action_authorization_frozen_provenance_ceiling_and_binding() {
    let request = request();
    let subject = subject(&request);
    for mode in 0..9 {
        let mut authority = authority(request.clone());
        match mode {
            0 => {}
            1 => authority.actor = WorkflowActor::authenticated(Uuid::new_v4()).unwrap(),
            2 => authority.action.request.context.capability_ceiling.clear(),
            3 => authority
                .action
                .request
                .context
                .capability_ceiling
                .push(TypeName::parse("broader").unwrap()),
            4 => authority.action.request.contract.policy.policy_revision += 1,
            5 => {
                authority.action.request.contract.contract.connection =
                    Some(crate::domain::workflow::ResourceName::parse("service").unwrap())
            }
            6 => {
                authority.action.request.target = ActionTarget::Connection {
                    resource: Uuid::new_v4(),
                    resource_kind: TypeName::parse("mcp").unwrap(),
                }
            }
            7 => authority.action.request.scope.run = RunId::new(Uuid::new_v4()),
            _ => authority.action.request.scope.company = CompanyId::new(Uuid::new_v4()),
        }
        let result = authorization::validate_run_authority(request.scope, &subject, &authority);
        assert_eq!(result.is_ok(), mode == 0);
    }
}

#[test]
fn workflow_action_authorization_exact_frozen_connection_slot() {
    let mut request = request();
    let slot = crate::domain::workflow::ResourceName::parse("service").unwrap();
    let id = Uuid::new_v4();
    request.contract.contract.connection = Some(slot.clone());
    request.target = ActionTarget::Connection {
        resource: id,
        resource_kind: TypeName::parse("mcp").unwrap(),
    };
    let subject = subject(&request);
    let mut authority = authority(request.clone());
    assert!(authorization::validate_run_authority(request.scope, &subject, &authority).is_ok());
    authority
        .resources
        .insert(slot.clone(), RuntimeResourceId::new(Uuid::new_v4()));
    assert!(authorization::validate_run_authority(request.scope, &subject, &authority).is_err());
    authority.resources.remove(&slot);
    assert!(authorization::validate_run_authority(request.scope, &subject, &authority).is_err());
}

#[tokio::test]
async fn workflow_action_authorization_lookup_timeouts_fail_closed() {
    let request = request();
    let service = ActionService::new(Store(request.clone()));
    let subject = subject(&request);
    let resources = Directory {
        request: request.clone(),
        mode: 11,
    };
    let good_resources = Directory {
        request: request.clone(),
        mode: 0,
    };
    let policy = Policies {
        request: request.clone(),
        mode: 9,
    };
    let good_policy = Policies {
        request: request.clone(),
        mode: 0,
    };
    let (resource, policy) = tokio::join!(
        service.authorize(request.scope, &subject, &resources, &good_policy),
        service.authorize(request.scope, &subject, &good_resources, &policy),
    );
    assert!(matches!(resource, Err(AppError::Timeout(_))));
    assert!(matches!(policy, Err(AppError::Timeout(_))));
}
