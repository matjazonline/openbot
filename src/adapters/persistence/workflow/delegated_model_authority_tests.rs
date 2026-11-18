//! Real owned/observer/manual/scheduled activations at the model authority boundaries.
use super::*;
use crate::adapters::persistence::workflow::{action_authority, agent_preparation};
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::application::{
    ports::channel::ChannelPersistence,
    workflow::{actions::ActionScope, agent::AgentPreparation, triggers::MessageTriggerInput},
};
use crate::entities::company_member::CompanyAccessRole;
use crate::use_cases::user::UserPersistence;

#[derive(Clone, Copy)]
enum Provenance {
    OwnedReply,
    Observer,
    Manual,
    Schedule,
}

struct ModelAuthority {
    h: HistoryThread,
    claim: ClaimedWorkflow,
    principal: Uuid,
}

impl ModelAuthority {
    async fn new(provenance: Provenance) -> Self {
        let mut h = HistoryThread::new().await;
        h.s.fixture.persistence =
            h.s.fixture
                .persistence
                .clone()
                .with_app_domain("agents.test");
        if matches!(provenance, Provenance::Schedule) {
            h.s.deactivate(h.binding).await;
            h.binding = history_contract_binding(
                &mut h.s,
                "history",
                HistoryContract {
                    input: json!(true),
                    output: json!({"type":"string"}),
                    mcp_error_policy: None,
                },
            )
            .await;
        }
        let principal: Uuid =
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
                .bind(h.s.fixture.target.company.as_uuid())
                .bind(h.agent)
                .fetch_one(h.s.pool())
                .await
                .unwrap();
        private_agent_desk(&h, principal).await;
        let claim = match provenance {
            Provenance::OwnedReply => {
                super::super::channel_reply_tests::select(&h.s, h.binding).await;
                h.next().await
            }
            Provenance::Observer => h.next().await,
            Provenance::Manual => manual_claim(&h).await,
            Provenance::Schedule => schedule_claim(&h).await,
        };
        Self {
            h,
            claim,
            principal,
        }
    }

    fn p(&self) -> &PostgresPersistence {
        self.h.s.p()
    }
    fn scope(&self) -> ActionScope {
        let scope = self.claim.fence.scope;
        ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        }
    }

    async fn remove_owner_view(&self) {
        let suffix = Uuid::new_v4().simple().to_string();
        let owner = self
            .p()
            .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
            .await
            .unwrap();
        crate::adapters::persistence::test_support::transfer_company_ownership(
            self.p().pool(),
            self.scope().company.as_uuid(),
            owner.id,
            CompanyAccessRole::Admin,
        )
        .await;
        let context = self
            .p()
            .access_context_for_user(self.scope().company.as_uuid(), self.h.s.owner().user_id())
            .await
            .unwrap()
            .unwrap();
        let channel = ChannelPersistence::get_by_id(self.p(), self.h.s.channel)
            .await
            .unwrap()
            .unwrap();
        assert!(!channel.viewer_access(context));
        let role = crate::application::use_cases::administration::AdministrationPersistence::management_role(
            self.p(), self.h.s.owner().user_id(), self.scope().company,
            crate::application::use_cases::administration::ManagementTarget::Unit(channel.department_id),
        ).await.unwrap();
        assert_eq!(
            role,
            Some(crate::application::use_cases::administration::ManagementRole::Admin)
        );
    }

    async fn prepared(&self) -> Value {
        self.p()
            .prepare_agent(
                &self.claim,
                LeasePolicy::new(Duration::from_secs(60)).unwrap(),
            )
            .await
            .unwrap()
            .expect_ready()
            .prepared
            .spec(self.claim.fence)
            .unwrap()
            .prepared
    }

    async fn current_transaction(
        &self,
        prepared: &Value,
    ) -> AppResult<sqlx::Transaction<'_, sqlx::Postgres>> {
        let mut tx = self.p().pool().begin().await?;
        action_authority::prelock_on(&mut tx, self.scope()).await?;
        agent_preparation::current_on(&mut tx, self.scope(), prepared).await?;
        Ok(tx)
    }

    async fn revocation(&self, capability: &str) -> sqlx::Transaction<'_, sqlx::Postgres> {
        let mut tx = self.p().pool().begin().await.unwrap();
        let changed = sqlx::query("DELETE FROM channel_principal_grants WHERE company_id=$1 AND channel_id=$2 AND principal_id=$3 AND capability=$4")
            .bind(self.scope().company.as_uuid()).bind(self.h.s.channel).bind(self.principal)
            .bind(capability).execute(&mut *tx).await.unwrap().rows_affected();
        assert!(changed > 0);
        tx
    }
}

async fn private_agent_desk(h: &HistoryThread, principal: Uuid) {
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        h.s.pool(),
        h.s.channel,
    )
    .await;
    let unit: Uuid = sqlx::query_scalar("SELECT department_id FROM channels WHERE id=$1")
        .bind(h.s.channel)
        .fetch_one(h.s.pool())
        .await
        .unwrap();
    let departments = crate::adapters::persistence::department::PostgresDepartments::new(
        h.s.p().clone(),
        crate::adapters::persistence::test_support::provisioning_templates(),
    );
    crate::application::use_cases::department::DepartmentUseCases::new(
        Arc::new(h.s.p().clone()),
        Arc::new(departments),
    )
    .add_member(
        h.s.owner().user_id(),
        h.s.fixture.target.company.as_uuid(),
        crate::entities::department::DepartmentId::new(unit),
        crate::entities::transport::PrincipalId::new(principal),
    )
    .await
    .unwrap();
}

async fn claim_run(h: &HistoryThread, run: RunId) -> ClaimedWorkflow {
    #[derive(sqlx::FromRow)]
    struct PendingModel {
        execution: Uuid,
        job: Uuid,
    }
    let pending: PendingModel = sqlx::query_as(
        "SELECT execution.id AS execution,job.id AS job FROM workflow_executions AS execution JOIN background_tasks AS job \
         ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(h.s.fixture.target.company.as_uuid()).bind(run.as_uuid()).fetch_one(h.s.pool()).await.unwrap();
    h.s.p()
        .claim_io(
            crate::application::workflow::activation::ActivationRequest {
                company: h.s.fixture.target.company,
                run,
                execution: ExecutionId::new(pending.execution),
                job: crate::application::workflow::activation::WorkflowJobId(pending.job),
            },
            WorkflowWorkerId(Uuid::new_v4()),
            LeasePolicy::new(Duration::from_secs(60)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap()
}

async fn manual_claim(h: &HistoryThread) -> ClaimedWorkflow {
    use crate::application::workflow::manual::{self, StartRun};
    let source = h.s.message().await;
    let started = manual::start(
        &admission(h.s.p()),
        StartRun {
            company_id: h.s.fixture.target.company,
            actor: h.s.owner(),
            binding_id: h.binding,
            association: RelatedAssociation::Channel(RelatedChannelId::new(h.s.channel)),
            input: MessageTriggerInput::new(source, "Manual", "Work", "member@example.test")
                .to_value(),
            history_session: None,
            idempotency_key: "delegated-model-manual".into(),
            rerun_of: None,
        },
    )
    .await
    .unwrap();
    claim_run(h, started.run).await
}

async fn schedule_claim(h: &HistoryThread) -> ClaimedWorkflow {
    use crate::entities::schedule::{ScheduleTimezone, ScheduleType, ScheduleWrite};
    let cases =
        crate::adapters::persistence::workflow::schedule_test_support::schedule_use_cases(h.s.p());
    let schedule = cases
        .create_schedule(
            h.s.owner().user_id(),
            h.s.fixture.target.company.as_uuid(),
            h.s.channel,
            ScheduleWrite {
                name: "Model provenance".into(),
                schedule_type: ScheduleType::Interval,
                interval_seconds: Some(3600),
                scheduled_at: None,
                timezone: ScheduleTimezone::utc(),
                binding_id: h.binding.as_uuid(),
                enabled: true,
            },
        )
        .await
        .unwrap();
    cases
        .trigger_schedule_now(
            h.s.owner().user_id(),
            h.s.fixture.target.company.as_uuid(),
            schedule.id,
            "model-provenance",
        )
        .await
        .unwrap();
    let run: Uuid =
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1 AND binding_id=$2")
            .bind(h.s.fixture.target.company.as_uuid())
            .bind(h.binding.as_uuid())
            .fetch_one(h.s.pool())
            .await
            .unwrap();
    claim_run(h, RunId::new(run)).await
}

async fn blocked_by(pool: &sqlx::PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(pool).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("the real authority caller/revoker waits on this exact backend");
}

#[path = "delegated_model_revocation_tests.rs"]
mod revocation;
