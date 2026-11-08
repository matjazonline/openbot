use super::level::{high_next, public, raise};
use super::*;
use crate::application::workflow::agent::{AgentLaunch, PreparedAgent, PreparedStep};
use crate::entities::access_level::AccessLevel;

pub(super) async fn execute(f: &HistoryThread, claim: &ClaimedWorkflow) -> Value {
    Box::pin(runtime(f).execute(&WorkflowStepKind("agent.run".into()), claim))
        .await
        .completed_result()
        .unwrap()
}

pub(super) struct TargetSeed {
    pub(super) claim: ClaimedWorkflow,
    pub(super) prepared: PreparedAgent,
    pub(super) cancelled: ClaimedWorkflow,
}
impl TargetSeed {
    pub(super) async fn new(
        f: &HistoryThread,
        owner: ClaimedWorkflow,
        launch: AgentLaunch,
    ) -> Self {
        let waiting = f.next().await;
        assert!(matches!(
            f.s.p().prepare_agent(&waiting, policy()).await.unwrap(),
            AgentPreparationResult::Parked(_)
        ));
        cancel(f, &owner, "freeze-guard-release-owner").await;
        assert!(matches!(
            f.s.p().resume_wait(waiting.fence.scope).await.unwrap(),
            WaitProgress::Continued
        ));
        let claim =
            f.s.p()
                .claim_io(
                    waiting.fence.scope,
                    WorkflowWorkerId(Uuid::new_v4()),
                    policy(),
                )
                .await
                .unwrap()
                .unwrap();
        let mut history = launch.prepared.frozen_history().unwrap().clone();
        let base = PreparedAgent::new(
            target_selection(f, &claim).await,
            &claim.activation.inputs,
            PreparedStep::AgentRun { targets: vec![] },
        )
        .unwrap()
        .with_history(AgentExchangeId::for_execution(claim.fence.scope.execution))
        .unwrap();
        history.budget = base.history_budget(&history.seed).unwrap();
        let prepared = base.with_frozen_history(history).unwrap();
        let actual: Uuid = sqlx::query_scalar(
            "SELECT manifest_id FROM agent_history_exchanges WHERE company_id=$1 AND id=$2",
        )
        .bind(claim.fence.scope.company.as_uuid())
        .bind(claim.fence.scope.execution.as_uuid())
        .fetch_one(f.s.pool())
        .await
        .unwrap();
        assert_eq!(
            prepared.selection().manifest_id().as_uuid(),
            actual,
            "template must use this actual run's manifest"
        );
        Self {
            claim,
            prepared,
            cancelled: owner,
        }
    }
}

async fn target_selection(
    f: &HistoryThread,
    claim: &ClaimedWorkflow,
) -> crate::application::workflow::publication::ResolvedAgent {
    let mut connection = f.s.pool().acquire().await.unwrap();
    let bytes:Vec<u8>=sqlx::query_scalar("SELECT stored.bundle FROM workflow_runs AS run JOIN workflow_bundles AS stored ON stored.company_id=run.company_id AND stored.digest=run.bundle_digest WHERE run.company_id=$1 AND run.id=$2 AND octet_length(stored.bundle)<=$3")
        .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.run.as_uuid())
        .bind(crate::application::workflow::publication::MAX_BUNDLE_BYTES as i32).fetch_one(&mut *connection).await.unwrap();
    let bundle = crate::application::workflow::publication::restore_bundle(
        &bytes,
        &crate::adapters::workflow_source::WorkflowSourceDecoder,
    )
    .unwrap();
    crate::adapters::persistence::workflow::agent_manifest::execution_selection_on(
        &mut connection,
        crate::application::workflow::actions::ActionScope {
            company: claim.fence.scope.company,
            run: claim.fence.scope.run,
            execution: claim.fence.scope.execution,
        },
        f.s.owner(),
        &bundle,
    )
    .await
    .unwrap()
}

pub(super) struct NativeCase {
    pub(super) f: Box<HistoryThread>,
    pub(super) public: Vec<ClaimedWorkflow>,
    pub(super) hidden: ClaimedWorkflow,
    pub(super) target: TargetSeed,
}
impl NativeCase {
    pub(super) async fn new() -> Self {
        Box::pin(Self::with_hidden(1)).await
    }
    pub(super) async fn with_hidden(hidden_count: usize) -> Self {
        assert!((1..=18).contains(&hidden_count));
        let f = Box::new(Box::pin(HistoryThread::new()).await);
        let mut turns = vec![LlmTurn::text("\"public-a\""), LlmTurn::text("\"public-b\"")];
        turns.extend((0..hidden_count).map(|_| LlmTurn::text("\"hidden\"")));
        let llm = scripted_llm(turns).await;
        register_scripted_agent_base_url(f.agent, &llm.base_url);
        let mut sources = Vec::new();
        for expected in ["public-a", "public-b"] {
            let claim = f.next().await;
            assert_eq!(execute(&f, &claim).await, json!(expected));
            cancel(&f, &claim, &format!("finish-public-{expected}")).await;
            sources.push(claim);
        }
        let mut hidden = None;
        for index in 0..hidden_count {
            let claim = high_next(&f).await;
            raise(&f, &claim, AccessLevel::L5).await;
            assert_eq!(execute(&f, &claim).await, json!("hidden"));
            cancel(&f, &claim, &format!("finish-hidden-{index}")).await;
            hidden = Some(claim);
        }
        let hidden = hidden.unwrap();
        public(&f, &hidden).await;
        let owner = f.next().await;
        let launch =
            f.s.p()
                .prepare_agent(&owner, policy())
                .await
                .unwrap()
                .expect_ready();
        assert_eq!(launch.prepared.frozen_history().unwrap().selected.len(), 2);
        let target = Box::pin(TargetSeed::new(&f, owner, *launch)).await;
        assert_eq!(llm.finish().await.unwrap(), hidden_count + 2);
        Self {
            f,
            public: sources,
            hidden,
            target,
        }
    }

    pub(super) async fn write(
        &self,
        document: &Value,
    ) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
        sqlx::query("UPDATE agent_history_exchanges SET frozen_preparation=$3 WHERE company_id=$1 AND execution_id=$2")
            .bind(self.target.claim.fence.scope.company.as_uuid()).bind(self.target.claim.fence.scope.execution.as_uuid()).bind(document)
            .execute(self.f.s.pool()).await
    }

    pub(super) async fn metadata(&self, claim: &ClaimedWorkflow) -> Value {
        sqlx::query_scalar("SELECT jsonb_build_object('id',id,'position',position,'access_level',access_level) FROM agent_history_exchanges WHERE company_id=$1 AND id=$2")
            .bind(claim.fence.scope.company.as_uuid()).bind(claim.fence.scope.execution.as_uuid())
            .fetch_one(self.f.s.pool()).await.unwrap()
    }
}
