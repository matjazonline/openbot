use super::*;
use crate::application::workflow::{
    activation::{ActivationRequest, WorkflowJobId},
    agent_history::AgentHistorySessionRef,
};

pub(super) struct ManualHistory {
    pub(super) f: Box<HistoryThread>,
    pub(super) binding: WorkflowBindingId,
    pub(super) from_run: Option<RunId>,
    pub(super) actor: WorkflowActor,
}
impl ManualHistory {
    pub(super) async fn new() -> Self {
        let mut f = Box::new(Box::pin(HistoryThread::new()).await);
        level::team(&f).await;
        let any = json!({"type":["object","array","string","number","boolean","null"]});
        let binding = history_contract_binding(
            &mut f.s,
            "history",
            HistoryContract {
                input: any.clone(),
                output: any,
                mcp_error_policy: None,
            },
        )
        .await;
        let actor = f.s.owner();
        Self {
            f,
            binding,
            from_run: None,
            actor,
        }
    }

    pub(super) async fn start(&mut self, input: Value) -> ClaimedWorkflow {
        let company = self.f.s.fixture.target.company;
        let request = AdmitWorkflowRequest {
            company_id: company,
            actor: self.actor,
            binding_id: self.binding,
            association: RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(self.f.s.channel),
                thread_id: RelatedThreadId::new(self.f.s.thread),
            },
            trigger: TriggerRef::new(
                company,
                TriggerId::new(Uuid::new_v4()),
                TriggerSource::Manual,
            )
            .unwrap(),
            correlation_id: CorrelationId::new(),
            idempotency_key: IdempotencyKey::parse(Uuid::new_v4().to_string()).unwrap(),
            input,
            rerun_of: None,
            history_session: self
                .from_run
                .map(|from_run| AgentHistorySessionRef { from_run }),
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
            expected_binding_revision: None,
            admission_claim: None,
        };
        let AdmissionResult::Created(run) = admission(self.f.s.p()).admit(request).await.unwrap()
        else {
            panic!("new private task")
        };
        self.from_run.get_or_insert(run);
        claim_run(&self.f, run).await
    }

    pub(super) async fn complete_ordinary(
        &mut self,
        input: Value,
        output: Value,
    ) -> ClaimedWorkflow {
        use crate::application::workflow::completion::WorkflowCompletion;
        let claim = self.start(input).await;
        let result =
            Box::pin(runtime(&self.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
        assert_eq!(result.completed_result().unwrap(), output);
        self.f
            .s
            .p()
            .complete_io(crate::application::workflow::lease::FencedWorkflowResult {
                fence: claim.fence,
                output,
                policy: policy(),
            })
            .await
            .unwrap()
            .expect("ordinary completed job, not cancellation");
        claim
    }

    pub(super) async fn complete(&mut self, input: Value, output: Value) -> ClaimedWorkflow {
        let claim = self.start(input).await;
        let result =
            Box::pin(runtime(&self.f).execute(&WorkflowStepKind("agent.run".into()), &claim)).await;
        assert_eq!(result.completed_result().unwrap(), output);
        cancel(
            &self.f,
            &claim,
            &format!("retire-source-{}", claim.fence.scope.run.as_uuid()),
        )
        .await;
        claim
    }
}

async fn claim_run(f: &HistoryThread, run: RunId) -> ClaimedWorkflow {
    #[derive(sqlx::FromRow)]
    struct Job {
        execution: Uuid,
        job: Uuid,
    }
    let job:Job=sqlx::query_as("SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.completed_at IS NULL")
        .bind(f.s.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    f.s.p()
        .claim_io(
            ActivationRequest {
                company: f.s.fixture.target.company,
                run,
                execution: ExecutionId::new(job.execution),
                job: WorkflowJobId(job.job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            policy(),
        )
        .await
        .unwrap()
        .unwrap()
}
