//! p08 end to end against PostgreSQL and the production composition: an accepted email becomes
//! an intent, the engine admits a run for it, a scripted agent answers, and the answer is a
//! `message.reply` that names its source and its run; a member's emailed answer to a workflow's
//! question resumes the wait that asked it.
use super::*;
use crate::adapters::protocols::email::EmailRenderer;
use crate::adapters::protocols::email::parser::RawInboundPayload;
use crate::application::transport::TransportRenderers;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::application::use_cases::company::{CompanyModelConnectionWrite, CompanyPersistence};
use crate::application::use_cases::thread::{InboundIngestPorts, ThreadStores, ThreadUseCases};
use crate::application::workflow::triggers::MessageTriggerInput;
use crate::domain::workflow::MessageTrigger;
use crate::infra::config::AppConfig;
use crate::infra::workflow::WorkflowEngine;
use crate::services::test_support::{
    LlmTurn, SCRIPTED_MODEL, SCRIPTED_PROVIDER, ScriptedLlm, register_scripted_agent_base_url,
    scripted_llm,
};
use crate::transport::DeliveryComposer;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[path = "schedule_runtime_tests.rs"]
mod schedule_runtime_tests;

#[path = "provisioning_runtime_tests.rs"]
mod provisioning_runtime_tests;

#[path = "reviewed_delivery_tests.rs"]
mod reviewed_delivery_tests;

const CUSTOMER: &str = "customer@partner.test";

/// The production engine and ingest over one company: a `support` channel the customer may
/// write to, and an agent answering through a scripted model.
struct Deployment {
    fixture: Fixture,
    channel: Uuid,
    threads: ThreadUseCases,
    engine: WorkflowEngine,
    llm: ScriptedLlm,
}

impl Deployment {
    async fn new(turns: Vec<LlmTurn>) -> Self {
        // The engine's warnings land in this test's captured output, shown when it fails.
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_env_filter("mail_agents=warn")
            .try_init();
        let fixture = Fixture::new().await;
        let p = fixture.persistence.clone();
        let company = fixture.target.company.as_uuid();
        let agent = AgentPersistence::create(
            &p,
            company,
            AgentWrite {
                name: "Assistant".into(),
                slug: "assistant".into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                system_prompt: Some("Answer the customer briefly.".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let channel = ChannelPersistence::create(
            &p,
            company,
            ChannelWrite {
                name: "Support".into(),
                slug: "support".into(),
                participant_emails: Some(vec![CUSTOMER.into()]),
                // Until B4 retires channel-owned agents, an enabled channel still names one.
                enabled: true,
                ..ChannelWrite::default()
            },
        )
        .await
        .unwrap()
        .id;
        CompanyPersistence::replace_model_connections_for_user(
            &p,
            fixture.target.actor.user_id(),
            company,
            vec![CompanyModelConnectionWrite {
                provider: SCRIPTED_PROVIDER.into(),
                api_key: Some("scripted-key".into()),
                models: vec![SCRIPTED_MODEL.into()],
                is_default: true,
            }],
        )
        .await
        .unwrap();
        let llm = scripted_llm(turns).await;
        register_scripted_agent_base_url(agent.id, &llm.base_url);
        let config = Arc::new(AppConfig {
            app_domain_name: APP_DOMAIN.into(),
            ..AppConfig::for_test()
        });
        let renderers = Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new(APP_DOMAIN)))
                .unwrap(),
        );
        let engine = WorkflowEngine::new(
            p.clone(),
            "https://agents.test",
            APP_DOMAIN,
            DeliveryComposer::new(renderers.clone(), Arc::new(p.clone())),
            Arc::new(crate::adapters::mcp::HttpMcpClient::new(
                crate::adapters::mcp::policy::EndpointPolicy::default(),
            )),
            Arc::new(crate::adapters::monitoring::InMemoryMonitor::new()),
            // One slot: this test database's pool holds five connections, and a slot needs two
            // while the intent worker, the schedule materializer and this test's polling need
            // one each (WF-P08-F029).
            1,
        )
        .unwrap();
        let shared = Arc::new(p.clone());
        let threads = ThreadUseCases::new(
            ThreadStores {
                threads: shared.clone(),
                channels: shared.clone(),
                companies: shared.clone(),
                participants: shared.clone(),
            },
            InboundIngestPorts {
                committer: shared.clone(),
                correlation: shared.clone(),
                bindings: shared.clone(),
                standalone_deliveries: shared,
            },
            renderers,
            config,
        )
        .with_intent_wakeups(engine.intent_wakeups.clone());
        Self {
            fixture,
            channel,
            threads,
            engine,
            llm,
        }
    }

    fn p(&self) -> &PostgresPersistence {
        &self.fixture.persistence
    }

    fn address(&self, local: &str) -> String {
        format!("{local}@workflow-tests.{APP_DOMAIN}")
    }

    /// Publish `source` through the production composition and bind it to the channel.
    async fn bind(&self, mut source: Value, trigger: Option<MessageTrigger>) -> BindingTarget {
        let workflow = WorkflowId::new(Uuid::new_v4());
        source["workflow_id"] = json!(workflow.to_string());
        let definitions = self.engine.definitions(Vec::new(), Vec::new());
        let definition = DefinitionTarget {
            workflow,
            ..self.fixture.target
        };
        let mut draft = self.fixture.save(None);
        draft.target = definition;
        draft.content.source = source.to_string();
        definitions.save(draft).await.unwrap();
        let mut publication = self.fixture.publish();
        publication.target = definition;
        publication.key =
            IdempotencyKey::parse(format!("publication-{}", workflow.as_uuid().simple())).unwrap();
        let version = publication.version;
        definitions.publish(publication).await.unwrap();
        let target = BindingTarget {
            company: self.fixture.target.company,
            actor: self.fixture.target.actor,
            binding: WorkflowBindingId::new(Uuid::new_v4()),
        };
        let state = self
            .engine
            .bindings()
            .configure(ConfigureBinding {
                target,
                expected: None,
                association: RelatedAssociation::Channel(RelatedChannelId::new(self.channel)),
                version,
                params: json!({}),
                resources: BTreeMap::new(),
                run_as: None,
                message_trigger: trigger,
            })
            .await
            .unwrap();
        self.engine
            .bindings()
            .activate(SetBindingActivity {
                target,
                expected: state.revision,
            })
            .await
            .unwrap();
        target
    }

    async fn email(
        &self,
        payload: RawInboundPayload,
    ) -> crate::use_cases::thread::InboundIngestResult {
        let ingested = self.threads.ingest_test_email(payload).await.unwrap();
        assert!(ingested.accepted, "{:?}", ingested.reason());
        ingested
    }

    /// Run the production workers until `done` holds, then shut them down and join them.
    async fn run_until(&self, what: &str, done: impl AsyncFn() -> bool) {
        let shutdown = CancellationToken::new();
        let handles = self.engine.spawn(&shutdown);
        let settled = tokio::time::timeout(Duration::from_secs(30), async {
            while !done().await {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        // Taken before shutdown, on a connection of its own, so a stalled worker is seen holding
        // whatever it holds (WF-P08-F029).
        let stalled = match settled {
            Ok(()) => None,
            Err(_) => Some(self.diagnostics().await),
        };
        shutdown.cancel();
        for (name, handle) in handles {
            tokio::time::timeout(Duration::from_secs(10), handle)
                .await
                .unwrap_or_else(|_| panic!("{name} did not stop"))
                .unwrap();
        }
        if let Some(state) = stalled {
            panic!("the engine never reached: {what}\n{state}");
        }
    }

    /// The queue and run state a timeout is diagnosed from (WF-P08-F029).
    async fn diagnostics(&self) -> String {
        use sqlx::Connection;
        let pool = self.p().pool();
        let pool_state = format!("pool size {} idle {}", pool.size(), pool.num_idle());
        let mut connection = match sqlx::PgConnection::connect_with(&pool.connect_options()).await {
            Ok(connection) => connection,
            Err(error) => return format!("{pool_state}; no diagnostic connection: {error}"),
        };
        let state: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object( \
               'intents', (SELECT jsonb_agg(jsonb_build_object('id', id, 'state', state, \
                 'attempts', attempts, 'available_at', available_at, 'lease', lease_expires_at, \
                 'outcome', outcome, 'reply_settled', reply_settled, 'now', clock_timestamp(), \
                 'updated_at', updated_at, 'worker', worker_id)) \
                 FROM workflow_message_intents WHERE company_id = $1), \
               'runs', (SELECT jsonb_agg(jsonb_build_object('id', id, 'state', state, \
                 'waiting', waiting_reason)) FROM workflow_runs WHERE company_id = $1), \
               'jobs', (SELECT jsonb_agg(jsonb_build_object('id', id, 'status', status, \
                 'lock_expires_at', lock_expires_at, 'run_at', run_at)) \
                 FROM background_tasks WHERE company_id = $1 AND queue_kind = 'workflow'), \
               'waits', (SELECT jsonb_agg(jsonb_build_object('run', run_id, 'reason', reason, \
                 'state', state, 'deadline', deadline, 'settled_at', settled_at, \
                 'consumed', consumed_event_id)) FROM workflow_waits WHERE company_id = $1), \
               'events', (SELECT jsonb_agg(jsonb_build_object('id', id, 'created_at', created_at, \
                 'consumed_by', consumed_execution_id)) FROM workflow_wait_events WHERE company_id = $1), \
               'sessions', (SELECT jsonb_agg(jsonb_build_object('pid', pid, 'state', state, \
                 'wait', wait_event_type || ':' || wait_event, 'xact_start', xact_start, \
                 'blocked_by', pg_blocking_pids(pid), 'query', left(query, 160))) \
                 FROM pg_stat_activity WHERE datname = current_database() AND pid <> pg_backend_pid()))",
        )
        .bind(self.fixture.target.company.as_uuid())
        .fetch_one(&mut connection)
        .await
        .unwrap_or_else(|error| json!({"diagnostics_unavailable": error.to_string()}));
        format!(
            "{pool_state}\n{}",
            serde_json::to_string_pretty(&state).unwrap_or_default()
        )
    }

    async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(self.fixture.target.company.as_uuid())
            .fetch_one(self.p().pool())
            .await
            .unwrap()
    }
}

/// An agent answers the message, and the workflow replies to exactly that message.
fn answering_source() -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": MessageTriggerInput::schema(),
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "answer",
        "steps": {
            "answer": {"type": "agent.run", "with": {
                "agent": {"literal": "assistant"},
                "context": {"object": {"message": {"ref": "/input/message"}}},
                "output_schema": {"literal": {"type": "string", "maxLength": 128}}},
                "routes": {"success": "reply"}},
            "reply": {"type": "message.reply", "with": {
                "content": {"object": {"body": {"ref": "/steps/answer/output"}}},
                "source_message": {"ref": "/input/source_message"}},
                "routes": {"success": "$end"}}
        },
        "limits": {"max_steps": 100, "max_context_bytes": 65536}
    })
}

fn customer_email(
    deployment: &Deployment,
    key: &str,
    in_reply_to: Option<&str>,
) -> RawInboundPayload {
    let mut headers = format!("Message-ID: <{key}@partner.test>\n");
    if let Some(parent) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: <{parent}@partner.test>\n"));
    }
    RawInboundPayload {
        headers: Some(headers),
        to: deployment.address("support"),
        from: CUSTOMER.into(),
        subject: Some("Invoice".into()),
        text: Some(format!("Question {key}")),
        ..Default::default()
    }
}

/// Each reply the workflows published: `(run, answered message, thread)`.
async fn replies(deployment: &Deployment) -> Vec<(Uuid, Uuid, Uuid)> {
    sqlx::query_as(
        "SELECT run_id, destination_id, thread_id FROM workflow_message_destinations \
         WHERE company_id = $1 AND kind = 'reply' ORDER BY created_at",
    )
    .bind(deployment.fixture.target.company.as_uuid())
    .fetch_all(deployment.p().pool())
    .await
    .unwrap()
}

/// Verification plan, end to end: inbound email → intent → run → scripted agent →
/// `message.reply` naming its source and run; and two messages in one thread keep separate
/// runs and separate replies, each answering its own message.
#[tokio::test]
async fn message_ingress_end_to_end_each_message_gets_its_own_run_and_reply() {
    let d = Deployment::new(vec![
        LlmTurn::text("\"We are on it.\""),
        LlmTurn::text("\"We are on it.\""),
    ])
    .await;
    let binding = d
        .bind(answering_source(), Some(MessageTrigger::Direct))
        .await;

    let first = d.email(customer_email(&d, "first", None)).await;
    let second = d.email(customer_email(&d, "second", Some("first"))).await;
    let thread = first.thread.as_ref().unwrap().id;
    assert_eq!(
        second.thread.as_ref().unwrap().id,
        thread,
        "one conversation"
    );
    assert_eq!((first.intent_ids.len(), second.intent_ids.len()), (1, 1));

    d.run_until("two replies", async || replies(&d).await.len() == 2)
        .await;

    let sources: Vec<Uuid> = [&first, &second]
        .iter()
        .map(|ingest| {
            ingest
                .inbound_message
                .as_ref()
                .unwrap()
                .canonical_id
                .as_uuid()
        })
        .collect();
    let runs: Vec<(Uuid, Uuid, Value)> = sqlx::query_as(
        "SELECT id, trigger_id, input FROM workflow_runs WHERE binding_id = $1 ORDER BY created_at",
    )
    .bind(binding.binding.as_uuid())
    .fetch_all(d.p().pool())
    .await
    .unwrap();
    assert_eq!(runs.len(), 2, "one run per message");
    let replies = replies(&d).await;
    for (index, ingest) in [&first, &second].iter().enumerate() {
        let (run, trigger, input) = &runs
            .iter()
            .find(|run| run.1 == ingest.intent_ids[0])
            .cloned()
            .expect("each message's intent admitted its own run");
        assert_eq!(*trigger, ingest.intent_ids[0]);
        assert_eq!(input["source_message"], sources[index].to_string());
        let answered: Vec<_> = replies.iter().filter(|reply| reply.0 == *run).collect();
        assert_eq!(answered.len(), 1, "one reply per run");
        assert_eq!((answered[0].1, answered[0].2), (sources[index], thread));
    }
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_message_intents WHERE company_id = $1 AND outcome = 'admitted'")
            .await,
        2
    );
    assert_eq!(d.llm.finish().await.unwrap(), 2);
}

/// A plain member provisioned as production does: a principal holding their account email as a
/// verified identity, so mail from that address is theirs. Returns the user and that address.
async fn verified_member(fixture: &Fixture) -> (Uuid, String) {
    let pool = fixture.persistence.pool();
    let company = fixture.target.company.as_uuid();
    let user = Uuid::new_v4();
    let email = format!("{}@example.test", user.simple());
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash) VALUES ($1, $2, $3, 'test')",
    )
    .bind(user)
    .bind(user.simple().to_string())
    .bind(&email)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO company_members (id, company_id, user_id, role) VALUES ($1, $2, $3, 'member')",
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    crate::adapters::persistence::participant::create_person_principal_on(
        &mut connection,
        company,
        user,
        "Member",
        &email,
    )
    .await
    .unwrap();
    (user, email)
}

/// A workflow emails a member a question and waits for their answer.
fn asking_source(member: Uuid) -> Value {
    json!({
        "format_version": 1,
        "workflow_id": Uuid::nil().to_string(),
        "input_schema": true,
        "parameter_schema": true,
        "output_schema": true,
        "resources": [],
        "entry": "ask",
        "steps": {
            "ask": {"type": "message.send", "with": {
                "content": {"literal": {"subject": "Approve the refund?", "body": "Please answer."}},
                "destinations": {"literal": [{"kind": "user", "id": member.to_string()}]}},
                "routes": {"success": "answer"}},
            "answer": {"type": "wait.event", "with": {
                "event": {"literal": "message.reply"},
                "correlation": {"ref": "/steps/ask/output/messages/0/message_id"},
                "deadline": {"literal": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()},
                "payload_schema": {"literal": MessageTriggerInput::schema()}},
                "routes": {"success": "$end"}}
        },
        "limits": {"max_steps": 100, "max_context_bytes": 65536}
    })
}

/// D1.5.4/.7 (F016.3): a member, not on the channel's allowlist, answers a workflow's emailed
/// question by email; ingress accepts it as a party of the thread the question opened, and the
/// answer resumes the run's wait rather than starting another run.
#[tokio::test]
async fn message_ingress_member_email_reply_resumes_the_question_on_an_allowlist_channel() {
    let d = Deployment::new(Vec::new()).await;
    let (user, email) = verified_member(&d.fixture).await;
    let target = d.bind(asking_source(user), None).await;
    let other = d.bind(answering_source(), Some(MessageTrigger::Any)).await;
    let scope = Box::pin(admitted(&d.fixture, target, d.channel, "ask-member")).await;
    let run = scope.run.as_uuid();

    // The question goes out; the delivery engine is not running, so settle it by hand.
    d.run_until("the question's delivery", async || {
        d.count("SELECT count(*) FROM workflow_message_destinations WHERE company_id = $1 AND delivery_id IS NOT NULL")
            .await
            == 1
    })
    .await;
    deliver(
        d.p().pool(),
        delivery(d.p().pool(), scope.run, 0).await,
        "delivered",
    )
    .await;
    let parked = async || {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM workflow_waits WHERE run_id = $1 AND reason = 'event' AND state = 'waiting'",
        )
        .bind(run)
        .fetch_one(d.p().pool())
        .await
        .unwrap()
            == 1
    };
    d.run_until("the wait parked", parked).await;

    let (question, rfc): (Uuid, String) = sqlx::query_as(
        "SELECT target.message_id, metadata.rfc_message_id FROM workflow_message_destinations AS target \
         JOIN email_message_metadata AS metadata ON metadata.company_id = target.company_id \
           AND metadata.message_id = target.message_id WHERE target.run_id = $1",
    )
    .bind(run)
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    let answer = d
        .email(RawInboundPayload {
            headers: Some(format!(
                "Message-ID: <answer@example.test>\nIn-Reply-To: {rfc}\nReferences: {rfc}\n"
            )),
            to: d.address("support"),
            from: email,
            subject: Some("Re: Approve the refund?".into()),
            text: Some("Approved.".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(answer.intent_ids.len(), 1);

    let intent = answer.intent_ids[0];
    d.run_until("the answer matched", async || {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT outcome FROM workflow_message_intents WHERE id = $1",
        )
        .bind(intent)
        .fetch_one(d.p().pool())
        .await
        .unwrap()
        .is_some()
    })
    .await;
    let (outcome, matched, reply_to): (Option<String>, Option<Uuid>, Option<Uuid>) =
        sqlx::query_as(
            "SELECT outcome, matched_run_id, reply_to_message_id FROM workflow_message_intents WHERE id = $1",
        )
        .bind(intent)
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    assert_eq!(reply_to, Some(question));
    assert_eq!((outcome.as_deref(), matched), (Some("matched"), Some(run)));
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE binding_id = $1")
        .bind(other.binding.as_uuid())
        .fetch_one(d.p().pool())
        .await
        .unwrap();
    assert_eq!(runs, 0, "the answer started nothing else");
    d.run_until("the run completed", async || {
        sqlx::query_scalar::<_, String>("SELECT state FROM workflow_runs WHERE id = $1")
            .bind(run)
            .fetch_one(d.p().pool())
            .await
            .unwrap()
            == "succeeded"
    })
    .await;
}
