//! Exercise the shipped read-only source against actual Doop through normal production owners.
use super::*;
use crate::adapters::persistence::workflow::mcp::McpPublicationSelection;
use crate::application::agent::ActionEffect;
use crate::application::workflow::{
    AdmissionResult, AdmitWorkflowRequest, IdempotencyKey, RelatedAssociation, WorkflowActor,
    lifecycle::*, mcp::*, run_views::*,
};
use crate::domain::entities::correlation::CorrelationId;
use crate::domain::workflow::*;
use crate::infra::workflow::WorkflowEngine;
use std::{collections::BTreeMap, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) async fn prepare(
    service: &McpUseCases,
    engine: &WorkflowEngine,
    actor: Uuid,
    current: &crate::entities::mcp::CompanyMcpConnection,
) -> crate::entities::mcp::CompanyMcpConnection {
    let selected = ["get_profile", "get_canvas"];
    let connection = service
        .update(
            actor,
            current,
            McpConnectionWrite {
                slug: current.slug.clone(),
                endpoint: current.endpoint.clone(),
                enabled: current.enabled,
                auth: current.auth.clone(),
                discovered_tools: current.discovered_tools.clone(),
                tool_grants: selected
                    .map(|name| name.to_owned().try_into().unwrap())
                    .into(),
            },
        )
        .await
        .unwrap();
    let company = CompanyId::new(connection.company_id);
    for name in selected {
        let tool = connection
            .discovered_tools
            .iter()
            .find(|tool| tool.name.as_str() == name)
            .unwrap();
        engine
            .services()
            .mcp_operations()
            .register_mcp_operation(
                company,
                WorkflowActor::authenticated(actor).unwrap(),
                McpOperationWrite {
                    connection: connection.id,
                    catalog_revision: connection.revision,
                    expected_version: None,
                    operation: McpOperation::review(company, tool, ActionEffect::Read, false, 1)
                        .unwrap(),
                },
            )
            .await
            .unwrap();
    }
    connection
}

async fn publish(engine: &WorkflowEngine, target: DefinitionTarget, connection: Uuid) -> VersionId {
    let selected = ["get_profile", "get_canvas"].map(|tool| McpPublicationSelection {
        connection,
        slot: ResourceName::parse("service").unwrap(),
        tool: TypeName::parse(tool).unwrap(),
    });
    let definitions = engine.services().definitions(vec![], selected.into());
    let mut source: Value = serde_json::from_str(include_str!(
        "../../../integrations/doop/verify-company.workflow.json"
    ))
    .unwrap();
    source["workflow_id"] = json!(target.workflow.as_uuid());
    let saved = definitions
        .save(SaveDraft {
            target,
            expected: None,
            content: DraftContent {
                title: "Verify disposable Doop connection".into(),
                description: "Read-only synthetic identity proof".into(),
                source: source.to_string(),
            },
        })
        .await
        .unwrap();
    let version = VersionId::new(Uuid::new_v4());
    definitions
        .publish(PublishDraft {
            target,
            expected: saved.revision(),
            version,
            key: IdempotencyKey::parse(format!("verify-{}", target.workflow.as_uuid())).unwrap(),
        })
        .await
        .unwrap();
    version
}

async fn bind(
    engine: &WorkflowEngine,
    target: DefinitionTarget,
    connection: Uuid,
    company: &Company,
    version: VersionId,
) -> BindingTarget {
    let binding = BindingTarget {
        company: target.company,
        actor: target.actor,
        binding: WorkflowBindingId::new(Uuid::new_v4()),
    };
    let bindings = engine.services().bindings();
    let configured = bindings.configure(ConfigureBinding {
        target: binding, expected: None, association: RelatedAssociation::Company, version,
        params: json!({"expected_bot_id":company.bot,"expected_workspace_id":company.workspace,"verification_canvas_id":company.verification_canvas}),
        resources: BTreeMap::from([(ResourceName::parse("service").unwrap(), RuntimeResourceId::new(connection))]),
        run_as: None, message_trigger: None,
    }).await.unwrap();
    bindings
        .activate(SetBindingActivity {
            target: binding,
            expected: configured.revision,
        })
        .await
        .unwrap();
    binding
}

async fn admit(
    engine: &WorkflowEngine,
    binding: BindingTarget,
) -> crate::app_error::AppResult<RunId> {
    let admission = engine
        .services()
        .service()
        .admit(AdmitWorkflowRequest {
            history_session: None,
            company_id: binding.company,
            actor: binding.actor,
            association: RelatedAssociation::Company,
            trigger: TriggerRef::new(
                binding.company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            binding_id: binding.binding,
            idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
            input: json!({}),
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        })
        .await?;
    let AdmissionResult::Created(run) = admission else {
        panic!("fresh verification run")
    };
    Ok(run)
}

async fn drive(p: &PostgresPersistence, engine: &WorkflowEngine, scope: RunReadScope) -> RunView {
    let shutdown = CancellationToken::new();
    let _cancel_on_drop = shutdown.clone().drop_guard();
    let workers = engine.spawn(&shutdown);
    let result = tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let view = p.run_view(scope).await?.unwrap();
            if matches!(view.detail.summary.state.as_str(), "succeeded" | "failed") {
                return crate::app_error::AppResult::Ok(view);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    shutdown.cancel();
    for (_, worker) in workers {
        tokio::time::timeout(Duration::from_secs(10), worker)
            .await
            .unwrap()
            .unwrap();
    }
    result
        .expect("actual Doop verification must terminate")
        .unwrap()
}

pub(super) struct Verification {
    target: DefinitionTarget,
    version: VersionId,
    binding: BindingTarget,
}

pub(super) async fn freeze_verification(
    engine: &WorkflowEngine,
    actor: Uuid,
    connection: &crate::entities::mcp::CompanyMcpConnection,
    company: &Company,
) -> Verification {
    let target = DefinitionTarget {
        company: CompanyId::new(connection.company_id),
        actor: WorkflowActor::authenticated(actor).unwrap(),
        workflow: WorkflowId::new(Uuid::new_v4()),
    };
    let version = Box::pin(publish(engine, target, connection.id)).await;
    let binding = bind(engine, target, connection.id, company, version).await;
    Verification {
        target,
        version,
        binding,
    }
}

impl Verification {
    pub(super) async fn execute(
        &self,
        p: &PostgresPersistence,
        engine: &WorkflowEngine,
    ) -> crate::app_error::AppResult<(RunReadScope, RunView)> {
        let run = admit(engine, self.binding).await?;
        let scope = RunReadScope {
            company: self.target.company,
            run,
            viewer: self.target.actor,
        };
        let view = Box::pin(drive(p, engine, scope)).await;
        Ok((scope, view))
    }

    pub(super) async fn verified(
        &self,
        p: &PostgresPersistence,
        engine: &WorkflowEngine,
        company: &Company,
    ) -> Value {
        let (scope, view) = self.execute(p, engine).await.unwrap();
        assert_eq!(view.detail.summary.state, "succeeded");
        let step = view
            .detail
            .executions
            .iter()
            .find(|step| step.step == "verified")
            .unwrap();
        let verified = p.step_view(scope, step.execution).await.unwrap().unwrap();
        let expected = json!({"verified":true,"bot_id":company.bot,"workspace_id":company.workspace,"canvas_id":company.verification_canvas});
        assert_eq!(verified.committed_output, Some(expected.clone()));
        json!({"workflow_id":self.target.workflow.as_uuid(),"version_id":self.version.as_uuid(),"binding_id":self.binding.binding.as_uuid(),"run_id":scope.run.as_uuid(),"result":expected})
    }
}
