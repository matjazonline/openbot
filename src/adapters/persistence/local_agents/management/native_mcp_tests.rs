use super::*;
#[path = "native_mcp_http_tests.rs"]
mod http_tests;
#[path = "native_mcp_integration_tests.rs"]
mod integration_tests;
#[path = "native_mcp_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "native_mcp_limits_tests.rs"]
mod limits_tests;
#[path = "native_mcp_live_tests.rs"]
mod live_tests;
#[path = "native_mcp_native_tests.rs"]
mod native_tests;
#[path = "native_mcp_private_tests.rs"]
mod private_tests;
#[path = "native_mcp_retention_tests.rs"]
mod retention_tests;
use crate::adapters::persistence::local_agents::protocol::test_support::{
    agent as agent_observation, turn,
};
use crate::application::agent::*;
use crate::application::local_agents::mcp::*;
use crate::application::local_agents::protocol::{ConnectionIdentity, JournalHello};
use crate::entities::agent_capability::*;
use crate::use_cases::{
    agent::{AgentPersistence, AgentWrite},
    mcp::*,
};

struct McpSetup {
    s: Setup,
    operation: OperationId,
    fence: ConnectionFence,
}

impl McpSetup {
    async fn new() -> Self {
        Box::pin(Self::new_at("http://127.0.0.1:9")).await
    }
    async fn new_at(endpoint: &str) -> Self {
        let s = prepared_setup(endpoint).await;
        Box::pin(Self::from_setup(s)).await
    }
    async fn from_setup(s: Setup) -> Self {
        let request = s.request();
        s.f.persistence.start_session(&request).await.unwrap();
        let fence =
            s.f.persistence
                .acquire_connection(
                    s.paired.candidate.credential.verifier(),
                    s.paired.receipt.machine.installation_id,
                    Uuid::new_v4(),
                )
                .await
                .unwrap()
                .unwrap();
        s.f.persistence
            .connect_journal(
                fence,
                &JournalHello {
                    version: 2,
                    installation_id: fence.machine.installation_id,
                    journal_id: Uuid::new_v4(),
                    journal_sequence: 0,
                },
            )
            .await
            .unwrap();
        assert!(s.f.persistence.finish_reconciliation(fence).await.unwrap());
        s.f.persistence.pending_commands(fence).await.unwrap();
        for observation in [
            agent_observation(1, request.request_id, LocalEvidence::Receipt {}),
            agent_observation(2, request.request_id, LocalEvidence::SubmissionIntent {}),
            turn(
                3,
                request.request_id,
                "turn_native_mcp",
                TurnState::Running,
                None,
            ),
        ] {
            s.f.persistence
                .accept_observation(fence, &observation)
                .await
                .unwrap();
        }
        Self {
            s,
            operation: request.request_id,
            fence,
        }
    }
    fn request(&self, arguments: Value) -> NativeMcpRequest {
        NativeMcpRequest {
            verifier: self.s.paired.candidate.credential.verifier(),
            operation: self.operation,
            identity: ConnectionIdentity::from_fence(self.fence).unwrap(),
            body: NativeMcpBody {
                invocation: NativeMcpInvocation {
                    thread_id: "thr_frozen".to_owned().try_into().unwrap(),
                    turn_id: "turn_native_mcp".to_owned().try_into().unwrap(),
                    client_request_id: "creq_native".to_owned().try_into().unwrap(),
                    model_call_id: "toolu_native".to_owned().try_into().unwrap(),
                },
                action: NativeMcpAction::Call {
                    alias: AgentToolAlias::parse("crm.lookup").unwrap(),
                    arguments,
                },
            },
        }
    }
}

async fn prepared_setup(endpoint: &str) -> Setup {
    let s = prepare_agent(Setup::new().await, endpoint).await;
    install_claude(&s).await;
    s.f.cases.save_profile(s.f.owner, &s.profile).await.unwrap();
    s
}

async fn prepare_agent(s: Setup, endpoint: &str) -> Setup {
    let source = crate::adapters::persistence::local_agents::test_support::native_mcp_binding(
        s.f.persistence.as_ref(),
        s.f.company,
        s.f.owner,
        endpoint,
    )
    .await;
    prepare_agent_with_source(s, source).await
}

async fn prepare_agent_with_source(mut s: Setup, source: AgentToolBinding) -> Setup {
    let company = CompanyId::new(s.f.company);
    let agent = AgentPersistence::create(
        s.f.persistence.as_ref(),
        s.f.company,
        AgentWrite {
            name: "Native MCP".into(),
            slug: "native-mcp".into(),
            native_permissions: LocalPermissions::Full,
            tool_bindings: Some(vec![source]),
            system_prompt: Some("Use the company lookup.".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    s.f.persistence
        .publish_agent(PublishAgent {
            scope: AgentPublicationScope {
                company,
                actor: WorkflowActor::authenticated(s.f.owner).unwrap(),
                agent: AgentId::new(agent.id),
            },
            expected_draft_revision: agent.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse("native-mcp").unwrap(),
        })
        .await
        .unwrap();
    s.profile.agent_id = Some(AgentId::new(agent.id));
    s.profile.selection.provider = LocalProvider::ClaudeCode;
    s.profile.selection.model = "fixture-claude".to_owned().try_into().unwrap();
    s
}

// A busy owner is a bounded retry of the same immutable invocation, never a new key.
async fn call_with_retry(
    p: &PostgresPersistence,
    request: &NativeMcpRequest,
) -> AppResult<NativeMcpResponse> {
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            let result=Box::pin(p.native_mcp(request)).await;
            if matches!(&result,Err(AppError::ServiceUnavailable(message)) if message.contains("source is busy")) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                continue;
            }
            return result;
        }
    }).await.map_err(|_|AppError::ServiceUnavailable("fixture source retry deadline".into()))?
}

async fn install_claude(s: &Setup) {
    let catalog = json!({"projects":["proj_test"],"providers":[{"provider":"claude-code","permission_modes":["full","auto","accept_edits"],
        "models":[{"model":"fixture-claude","reasoning_levels":["none","low","high"]}]}]});
    sqlx::query("INSERT INTO local_machine_capabilities(company_id,machine_id,installation_id,sequence,catalog) VALUES($1,$2,$3,2,$4)")
        .bind(s.f.company).bind(s.paired.receipt.machine.id.as_uuid()).bind(s.paired.receipt.machine.installation_id.as_uuid())
        .bind(catalog).execute(&s.f.database.pool).await.unwrap();
    sqlx::query("UPDATE local_machines SET capabilities_sequence=2 WHERE company_id=$1 AND id=$2")
        .bind(s.f.company)
        .bind(s.paired.receipt.machine.id.as_uuid())
        .execute(&s.f.database.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn native_mcp_competing_invocations_publish_one_real_linked_run_and_job() {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let request = f.request(json!({"query":"exact"}));
    let (left, right) = tokio::join!(call_with_retry(p, &request), call_with_retry(p, &request));
    let (left, right) = (left.unwrap(), right.unwrap());
    assert_eq!(left.call_id, right.call_id);
    assert_eq!(left.run_id, right.run_id);
    assert_eq!(left.deadline, right.deadline);
    let counts:(i64,i64,i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM native_mcp_calls),(SELECT count(*) FROM workflow_runs),
        (SELECT count(*) FROM workflow_executions),(SELECT count(*) FROM background_tasks WHERE workflow_execution_id IS NOT NULL),(SELECT count(*) FROM workflow_admissions)")
        .fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1, 1, 1));
    let linked:(Uuid,Value,bool)=sqlx::query_as("SELECT native_mcp_call_id,input,content_exact FROM workflow_runs WHERE company_id=$1 AND id=$2")
        .bind(f.s.f.company).bind(left.run_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(linked.0, left.call_id.as_uuid());
    assert_eq!(linked.1["arguments"], json!({"query":"exact"}));
    assert!(!linked.2);
    assert!(
        p.native_mcp(&f.request(json!({"query":"changed"})))
            .await
            .is_err()
    );
    for sql in [
        "UPDATE workflow_runs SET native_mcp_call_id=NULL",
        "UPDATE native_mcp_calls SET operation_id=gen_random_uuid()",
        "DELETE FROM native_mcp_calls",
    ] {
        assert!(
            sqlx::query(sql)
                .execute(&f.s.f.database.pool)
                .await
                .is_err(),
            "{sql}"
        );
    }
}

#[tokio::test]
async fn native_mcp_bad_input_or_unacknowledged_turn_never_publishes_work() {
    let f = McpSetup::new().await;
    for args in [
        json!([]),
        json!({}),
        json!({"query":1}),
        json!({"query":"x","reviewers":[]}),
    ] {
        assert!(
            f.s.f
                .persistence
                .native_mcp(&f.request(args))
                .await
                .is_err()
        );
    }
    let mut foreign = f.request(json!({"query":"x"}));
    foreign.body.invocation.turn_id = "turn_newer".to_owned().try_into().unwrap();
    assert!(f.s.f.persistence.native_mcp(&foreign).await.is_err());
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM native_mcp_brokers),(SELECT count(*) FROM workflow_runs),(SELECT count(*) FROM workflow_executions)")
        .fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
}

#[tokio::test]
async fn native_mcp_cancel_by_invocation_invalidates_then_run_first_sweep_closes_work() {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let response = p
        .native_mcp(&f.request(json!({"query":"pending"})))
        .await
        .unwrap();
    let live = || {
        sqlx::query_scalar::<_, bool>("SELECT public.native_mcp_run_live($1,$2)")
            .bind(f.s.f.company)
            .bind(response.run_id.as_uuid())
            .fetch_one(&f.s.f.database.pool)
    };
    assert!(live().await.unwrap());
    let mut cancel = f.request(json!({"query":"unused"}));
    cancel.body.action = NativeMcpAction::CancelInvocation;
    assert_eq!(
        p.native_mcp(&cancel).await.unwrap().call_id,
        response.call_id
    );
    assert!(!live().await.unwrap());
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 1);
    let row:(String,Option<String>)=sqlx::query_as("SELECT run.state,operation.outcome FROM workflow_runs AS run
        JOIN native_mcp_calls AS call ON call.company_id=run.company_id AND call.run_id=run.id
        JOIN local_agent_operations AS operation ON operation.company_id=call.company_id AND operation.id=call.operation_id
        WHERE run.company_id=$1 AND run.id=$2")
        .bind(f.s.f.company).bind(response.run_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert_eq!(
        row,
        ("cancelled".into(), None),
        "sweeper must not mutate the native owner inline"
    );
    let replay = p
        .native_mcp(&f.request(json!({"query":"pending"})))
        .await
        .unwrap();
    assert_eq!(replay.call_id, response.call_id);
    assert!(matches!(replay.state, NativeMcpState::Cancelled));
}

#[tokio::test]
async fn native_mcp_poison_retirement_keeps_a_durable_delay_after_rollback() {
    let f = McpSetup::new().await;
    let p = &f.s.f.persistence;
    let response = p
        .native_mcp(&f.request(json!({"query":"poison"})))
        .await
        .unwrap();
    let mut cancel = f.request(json!({"query":"unused"}));
    cancel.body.action = NativeMcpAction::CancelInvocation;
    p.native_mcp(&cancel).await.unwrap();
    sqlx::raw_sql(&format!("CREATE SEQUENCE native_mcp_poison_attempts;
        CREATE FUNCTION native_mcp_poison() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
            IF NEW.id='{}'::uuid AND NEW.state='cancelled' THEN PERFORM nextval('native_mcp_poison_attempts');
                RAISE EXCEPTION 'fixture poison'; END IF; RETURN NEW; END $$;
        CREATE TRIGGER native_mcp_poison BEFORE UPDATE ON workflow_runs FOR EACH ROW EXECUTE FUNCTION native_mcp_poison();",response.run_id.as_uuid()))
        .execute(&f.s.f.database.pool).await.unwrap();
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 0);
    let delayed:bool=sqlx::query_scalar("SELECT maintenance_after>clock_timestamp()+interval '20 seconds' FROM native_mcp_calls WHERE company_id=$1 AND id=$2")
        .bind(f.s.f.company).bind(response.call_id.as_uuid()).fetch_one(&f.s.f.database.pool).await.unwrap();
    assert!(delayed);
    assert_eq!(p.sweep_native_mcp(16).await.unwrap(), 0);
    let attempts: i64 = sqlx::query_scalar("SELECT last_value FROM native_mcp_poison_attempts")
        .fetch_one(&f.s.f.database.pool)
        .await
        .unwrap();
    assert_eq!(
        attempts, 1,
        "a rolled-back poison candidate must not be immediately reclaimed"
    );
}
