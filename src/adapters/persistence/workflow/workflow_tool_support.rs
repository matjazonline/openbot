//! A company with a workflow tool registration, an agent granted it, and a published, bound
//! parent whose `agent.run` can call it (07-D2). The model is scripted; nothing leaves the test.
use super::*;
use crate::adapters::{
    harness::rig::{WorkflowAgentRuntime, providers::ProviderRegistry},
    mcp::{HttpMcpClient, policy::EndpointPolicy},
    persistence::workflow::{
        agent_publication::AgentPublicationDirectory, child_publication::ChildPublicationDirectory,
        tests::approval_test_support::notices,
    },
    workflow_agent::AgentActionTools,
    workflow_http::{HttpDestinationPolicy, HttpRuntime},
    workflow_mcp::McpRuntime,
};
use crate::application::workflow::{
    AdmissionResult,
    activation::{ActivationRequest, WorkflowJobId},
};
use crate::application::{
    use_cases::{
        agent::{AgentPersistence, AgentWrite},
        company::{CompanyModelConnectionWrite, CompanyPersistence},
    },
    workflow::{
        agent::AgentToolObservation,
        batch::{BatchBudget, WorkflowBatch},
        decisions::{DecisionCommandKey, DecisionId, SubmitDecision, WorkflowDecisions},
        polling::WorkflowStepKind,
        publication::WorkflowToolPin,
        tools::{
            WorkflowToolCall, WorkflowToolDispatch, WorkflowToolRegistration, WorkflowToolWrite,
            WorkflowTools,
        },
        worker::WorkflowHandler,
    },
};
use crate::domain::entities::correlation::CorrelationId;
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};

pub(super) const TOOL: &str = "refund.review";
/// The provider-safe declaration the scripted model calls.
pub(super) const DECLARED: &str = "refund__review";
pub(super) const CHILD: u128 = 0x0901;

fn question_input() -> Value {
    json!({"type":"object","properties":{"question":{"type":"string","maxLength":200000}},"required":["question"],"additionalProperties":false})
}

pub(super) fn answer_output() -> Value {
    json!({"type":"object","properties":{"choice":{"type":"string","enum":["approve","reject"]},"answer":{"type":"string","maxLength":200000}},"required":["choice","answer"],"additionalProperties":false})
}

fn reviewer_params() -> Value {
    json!({"type":"object","properties":{"reviewer":{"type":"string","maxLength":64}},"required":["reviewer"],"additionalProperties":false})
}

/// A child whose reviewer, named by its own parameters, decides the question.
pub(super) fn decision_child() -> Value {
    let deadline = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": question_input(),
        "parameter_schema": reviewer_params(),
        "output_schema": answer_output(),
        "resources": [],
        "entry": "review",
        "steps": {
            "review": {"type":"decision.human","with":{
                "proposal":{"object":{"question":{"ref":"/input/question"}}},
                "reviewers":{"array":[{"object":{"kind":{"literal":"user"},"id":{"ref":"/params/reviewer"}}}]},
                "deadline":{"literal":deadline},
                "data_schema":{"literal":{"type":"string","maxLength":256}}},
                "routes":{"choices":{"approve":"outcome","reject":"outcome"}}},
            "outcome": {"type":"data.map","with":{
                "value":{"object":{"choice":{"ref":"/steps/review/output/choice"},"answer":{"ref":"/steps/review/output/data"}}},
                "output_schema":{"literal":answer_output()}},
                "routes":{"success":"$end"}}
        },
        "limits": {"max_steps": 10, "max_context_bytes": 65536}
    })
}

/// A child that answers at once by echoing the question.
pub(super) fn echo_child() -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": question_input(),
        "parameter_schema": reviewer_params(),
        "output_schema": answer_output(),
        "resources": [],
        "entry": "answer",
        "steps": {"answer": {"type":"data.map","with":{
            "value":{"object":{"choice":{"literal":"approve"},"answer":{"ref":"/input/question"}}},
            "output_schema":{"literal":answer_output()}},
            "routes":{"success":"$end"}}},
        "limits": {"max_steps": 10, "max_context_bytes": 1048576}
    })
}

pub(super) fn budget() -> BatchBudget {
    BatchBudget::new(4, std::time::Duration::from_millis(500)).unwrap()
}

/// The parent run's and its call wait's state, and its children.
#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
pub(super) struct CallParked {
    pub(super) run: String,
    pub(super) reason: Option<String>,
    pub(super) job: String,
    pub(super) stop: Option<String>,
    pub(super) children: i64,
}
pub(super) async fn call_parked(p: &PostgresPersistence, scope: ActivationRequest) -> CallParked {
    sqlx::query_as(
        "SELECT run.state AS run, run.waiting_reason AS reason, job.status AS job, \
           (SELECT stop_reason FROM task_attempts WHERE task_id=job.id ORDER BY attempt_number DESC LIMIT 1) AS stop, \
           (SELECT count(*) FROM workflow_runs WHERE parent_run_id=run.id) AS children \
         FROM workflow_runs AS run JOIN background_tasks AS job ON job.id=$2 WHERE run.id=$1",
    )
    .bind(scope.run.as_uuid())
    .bind(scope.job.0)
    .fetch_one(p.pool())
    .await
    .unwrap()
}

pub(super) fn kind() -> WorkflowStepKind {
    WorkflowStepKind("agent.run".into())
}

/// The saved result of the agent's first tool invocation, as its checkpoint holds it.
pub(super) async fn saved_result(p: &PostgresPersistence, scope: ActivationRequest) -> Value {
    sqlx::query_scalar(
        "SELECT checkpoint->'invocations'->0->'result' FROM task_harness_runs WHERE workflow_execution_id=$1",
    )
    .bind(scope.execution.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap()
}

/// How many children `run` has.
pub(super) async fn children(p: &PostgresPersistence, run: RunId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE parent_run_id=$1")
        .bind(run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}

pub(super) async fn run_state(p: &PostgresPersistence, run: RunId) -> String {
    sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
        .bind(run.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap()
}

pub(super) struct ToolSetup {
    pub(super) fixture: Fixture,
    pub(super) reviewer: Uuid,
    pub(super) agent: Uuid,
    pub(super) child: VersionId,
    pub(super) parent: VersionId,
    pub(super) target: BindingTarget,
}

impl ToolSetup {
    /// Publish `child`, register it as the `refund.review` tool, grant the tool to a scripted
    /// `assistant`, then publish and bind the `agent.run` parent.
    pub(super) async fn new(child: Value) -> Self {
        let fixture = Fixture::new().await;
        let reviewer = member(&fixture, "admin").await;
        let child = fixture
            .publish_workflow(WorkflowId::new(Uuid::from_u128(CHILD)), child)
            .await;
        let p = fixture.persistence.clone();
        let company = fixture.target.company;
        p.register_workflow_tool(
            company,
            fixture.target.actor,
            WorkflowToolWrite {
                name: TypeName::parse(TOOL).unwrap(),
                workflow: WorkflowId::new(Uuid::from_u128(CHILD)),
                version: child,
                params: json!({"reviewer": reviewer.to_string()}),
                expected: None,
            },
        )
        .await
        .unwrap();
        let agent = AgentPersistence::create(
            &p,
            company.as_uuid(),
            AgentWrite {
                name: "Assistant".into(),
                slug: "assistant".into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                system_prompt: Some("Ask the refund review, then answer.".into()),
                workflow_tools: Some(vec![TypeName::parse(TOOL).unwrap()]),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        CompanyPersistence::replace_model_connections_for_user(
            &p,
            fixture.target.actor.user_id(),
            company.as_uuid(),
            vec![CompanyModelConnectionWrite {
                provider: SCRIPTED_PROVIDER.into(),
                api_key: Some("scripted-key".into()),
                models: vec![SCRIPTED_MODEL.into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let parent = publish_parent(&fixture, "publication").await.unwrap();
        let target = Box::pin(bound(&fixture, parent)).await;
        Self {
            fixture,
            reviewer,
            agent,
            child,
            parent,
            target,
        }
    }

    pub(super) fn persistence(&self) -> &PostgresPersistence {
        &self.fixture.persistence
    }

    /// Admit a run of the bound parent and claim its `agent.run` step.
    pub(super) async fn claim(&self, key: &str) -> ClaimedWorkflow {
        let scope = admitted_as(&self.fixture, self.target, RelatedAssociation::Company, key).await;
        self.persistence()
            .claim_io(scope, WorkflowWorkerId(Uuid::new_v4()), lease())
            .await
            .unwrap()
            .unwrap()
    }

    /// Admit a run of `target` through the application service, as `trigger` would.
    pub(super) async fn admit(
        &self,
        target: BindingTarget,
        trigger: TriggerId,
        key: &str,
    ) -> AppResult<AdmissionResult> {
        let p = self.persistence();
        let company = self.fixture.target.company;
        WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight)
            .admit(AdmitWorkflowRequest {
                company_id: company,
                actor: self.fixture.target.actor,
                association: RelatedAssociation::Company,
                trigger: TriggerRef::new(company, trigger, TriggerSource::Manual).unwrap(),
                correlation_id: CorrelationId::new(),
                binding_id: target.binding,
                idempotency_key: IdempotencyKey::parse(key).unwrap(),
                input: json!({}),
                rerun_of: None,
            })
            .await
    }

    /// The pin `version` of the parent froze for the tool.
    pub(super) async fn frozen_pin(&self, version: VersionId) -> WorkflowToolPin {
        self.persistence()
            .selectable_version(self.fixture.target.company, version)
            .await
            .unwrap()
            .unwrap()
            .snapshots()
            .tools
            .iter()
            .find(|tool| tool.contract.name.as_str() == TOOL)
            .and_then(|tool| tool.workflow.clone())
            .unwrap()
    }

    /// A new saved call of the tool with `pin`, as the agent dispatches it.
    pub(super) fn call(&self, pin: WorkflowToolPin, arguments: Value) -> WorkflowToolCall {
        WorkflowToolCall {
            name: TypeName::parse(TOOL).unwrap(),
            pin,
            call: ToolCallId::new(Uuid::new_v4()),
            arguments,
        }
    }

    /// Dispatch `call` under `claim`'s fence.
    pub(super) async fn dispatch(
        &self,
        claim: &ClaimedWorkflow,
        call: &WorkflowToolCall,
    ) -> AppResult<AgentToolObservation> {
        self.persistence()
            .call_workflow_tool(claim.fence, call.clone())
            .await
    }

    /// Cancel `run` through the run control, as an operator would.
    pub(super) async fn cancel(&self, run: ActivationRequest, key: &str) {
        let p = self.persistence();
        let revision = p
            .head(run.company, run.run)
            .await
            .unwrap()
            .unwrap()
            .revision;
        assert!(matches!(
            p.cancel(CancelCommand {
                company_id: run.company,
                actor: self.fixture.target.actor,
                run_id: run.run,
                command_key: IdempotencyKey::parse(key).unwrap(),
                expected_revision: revision,
            })
            .await
            .unwrap(),
            CancelResult::Applied { .. }
        ));
    }

    /// Run the call's echo child to its end.
    pub(super) async fn finish_child(&self, parent: ActivationRequest) -> ActivationRequest {
        let child = self.child_of(parent).await;
        self.persistence()
            .advance_pure(child, budget())
            .await
            .unwrap();
        child
    }

    /// The `agent.run` handler with the workflow tool runtime composed in (WF-C001 r2).
    pub(super) fn runtime(&self) -> impl WorkflowHandler + use<> {
        let p = self.persistence().clone();
        WorkflowAgentRuntime::new(
            ProviderRegistry::standard().unwrap(),
            p.clone(),
            AgentActionTools::new(
                Arc::new(HttpRuntime::new(
                    p.clone(),
                    HttpDestinationPolicy::default(),
                    lease(),
                )),
                Arc::new(McpRuntime::new(
                    p.clone(),
                    Arc::new(HttpMcpClient::new(EndpointPolicy::default())),
                    lease(),
                )),
                Arc::new(runtime(&p)),
            )
            .with_workflow_tools(Arc::new(p.clone())),
            Some(notices(&p)),
            lease(),
        )
    }

    pub(super) async fn registration(&self) -> WorkflowToolRegistration {
        self.persistence()
            .workflow_tools(self.fixture.target.company, self.fixture.target.actor)
            .await
            .unwrap()
            .into_iter()
            .find(|tool| tool.name.as_str() == TOOL)
            .unwrap()
    }

    /// Point the tool at `workflow`/`version` (a re-pin, or a re-point to another workflow).
    pub(super) async fn repin(&self, workflow: WorkflowId, version: VersionId) {
        let current = self.registration().await;
        self.persistence()
            .register_workflow_tool(
                self.fixture.target.company,
                self.fixture.target.actor,
                WorkflowToolWrite {
                    name: current.name,
                    workflow,
                    version,
                    params: current.params,
                    expected: Some(current.revision),
                },
            )
            .await
            .unwrap();
    }

    pub(super) async fn revoke(&self) {
        let current = self.registration().await;
        self.persistence()
            .revoke_workflow_tool(
                self.fixture.target.company,
                self.fixture.target.actor,
                current.name,
                current.revision,
            )
            .await
            .unwrap();
    }

    /// The live child of `parent`'s call, with its entry execution and job.
    pub(super) async fn child_of(&self, parent: ActivationRequest) -> ActivationRequest {
        let (run, execution, job): (Uuid, Uuid, Uuid) = sqlx::query_as(
            "SELECT run.id, execution.id, job.id FROM workflow_runs AS run \
             JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id \
             JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
             WHERE run.parent_run_id=$1 ORDER BY run.created_at DESC, execution.activation DESC LIMIT 1",
        )
        .bind(parent.run.as_uuid())
        .fetch_one(self.persistence().pool())
        .await
        .unwrap();
        ActivationRequest {
            company: parent.company,
            run: RunId::new(run),
            execution: ExecutionId::new(execution),
            job: WorkflowJobId(job),
        }
    }

    /// The reviewer decides the child's open decision; returns the child's next step.
    pub(super) async fn decide(
        &self,
        child: ActivationRequest,
        choice: &str,
        data: &str,
    ) -> ActivationRequest {
        let p = self.persistence();
        p.park_decision(child, &notices(p)).await.unwrap().unwrap();
        let id: Uuid = sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE run_id=$1")
            .bind(child.run.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        let actor = WorkflowActor::authenticated(self.reviewer).unwrap();
        let view = p
            .read_decision(child.company, actor, DecisionId(id))
            .await
            .unwrap();
        let applied = p
            .submit_decision(SubmitDecision {
                company: child.company,
                actor,
                decision: view.decision,
                command_key: DecisionCommandKey::parse("review").unwrap(),
                expected_revision: view.revision,
                expected_artifact: view.artifact_digest.clone(),
                choice: choice.into(),
                feedback: String::new(),
                data: json!(data),
            })
            .await
            .unwrap();
        applied.successor.expect("the decision continues the child")
    }
}

/// Publish the `agent.run` parent through the production directory nesting (WF-C001 r2).
pub(super) async fn publish_parent(fixture: &Fixture, key: &str) -> LifecycleResult<VersionId> {
    let p = fixture.persistence.clone();
    let source = registry::example("agent.run").unwrap().source;
    publish_parent_with(
        fixture,
        key,
        source,
        message::MessagePublicationDirectory { persistence: p },
    )
    .await
}

/// Publish `source` as the fixture's workflow with `ChildPublicationDirectory` outside
/// `AgentPublicationDirectory` outside `inner`, which captures the steps' own tools.
pub(super) async fn publish_parent_with<D: PublicationDirectory>(
    fixture: &Fixture,
    key: &str,
    source: String,
    inner: D,
) -> LifecycleResult<VersionId> {
    let p = fixture.persistence.clone();
    let directory = ChildPublicationDirectory {
        persistence: p.clone(),
        inner: AgentPublicationDirectory {
            persistence: p,
            inner,
        },
    };
    publish_through(fixture, fixture.target.workflow, key, source, directory).await
}

/// Publish `source` as the next version of `workflow` through exactly `directory`.
pub(super) async fn publish_through<D: PublicationDirectory>(
    fixture: &Fixture,
    workflow: WorkflowId,
    key: &str,
    source: String,
    directory: D,
) -> LifecycleResult<VersionId> {
    let p = fixture.persistence.clone();
    let definitions =
        DefinitionService::new(Preflight, p.clone(), WorkflowSourceDecoder, directory);
    let target = DefinitionTarget {
        workflow,
        ..fixture.target
    };
    let current = p.draft(target.company, workflow).await.unwrap();
    let mut draft = fixture.save(current.as_ref().map(|state| state.draft.revision()));
    draft.target = target;
    draft.content.source = source;
    let saved = definitions.save(draft).await?;
    let mut publication = fixture.publish();
    publication.target = target;
    publication.expected = saved.revision();
    publication.version = VersionId::new(Uuid::new_v4());
    publication.key = IdempotencyKey::parse(key).unwrap();
    definitions.publish(publication.clone()).await?;
    Ok(publication.version)
}

/// Publish the next version of the child `workflow` (`source`'s id is overwritten).
pub(super) async fn publish_child(
    fixture: &Fixture,
    workflow: WorkflowId,
    mut source: Value,
    key: &str,
) -> VersionId {
    source["workflow_id"] = json!(workflow.to_string());
    let directory = ChildPublicationDirectory {
        persistence: fixture.persistence.clone(),
        inner: Preflight,
    };
    publish_through(fixture, workflow, key, source.to_string(), directory)
        .await
        .unwrap()
}
