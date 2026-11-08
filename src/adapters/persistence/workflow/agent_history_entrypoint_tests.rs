use super::*;
use crate::application::workflow::lifecycle::BindingLifecycle;

async fn dynamic_binding(d: &Deployment) -> BindingTarget {
    let other = AgentPersistence::create(
        d.p(),
        d.fixture.target.company.as_uuid(),
        AgentWrite {
            name: "Other agent".into(),
            slug: "other".into(),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            system_prompt: Some("Other actual agent instructions".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    d.fixture.publish_agent(other.id).await;
    crate::services::test_support::register_scripted_agent_base_url(other.id, &d.llm.base_url);
    let mut source = model_source();
    source["parameter_schema"] = json!({"type":"object","properties":{"agent":{"type":"string","enum":["assistant","other"]}},"required":["agent"],"additionalProperties":false});
    source["steps"]["start"]["with"]["agent"] = json!({"ref":"/params/agent"});
    d.bind_source(SourceBinding {
        source,
        params: json!({"agent":"assistant"}),
        trigger: Some(MessageTrigger::Any),
    })
    .await
}

async fn choose(d: &Deployment, binding: BindingTarget, key: crate::application::agent::AgentKey) {
    let state = d
        .p()
        .binding(binding.company, binding.binding)
        .await
        .unwrap()
        .unwrap();
    d.engine
        .services()
        .bindings()
        .configure(ConfigureBinding {
            target: binding,
            expected: Some(state.revision),
            association: state.association,
            version: state
                .configuration
                .bundle()
                .compiled()
                .graph()
                .definition()
                .version_id,
            params: json!({"agent":key.as_str()}),
            resources: state.configuration.resources().clone(),
            run_as: None,
            message_trigger: Some(MessageTrigger::Any),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_history_callers_dynamic_selection_uses_actual_agent_and_returns_to_its_own_thread_stream()
 {
    let mut d = Box::pin(Deployment::new(vec![
        LlmTurn::text("\"alpha\""),
        LlmTurn::text("\"beta\""),
        LlmTurn::text("\"gamma\""),
    ]))
    .await;
    let binding = dynamic_binding(&d).await;
    d.email(customer_email(&d, "dynamic-first", None)).await;
    Box::pin(d.run_until("first dynamic agent completed", async || {
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
            .await
            == 1
    }))
    .await;
    choose(
        &d,
        binding,
        crate::application::agent::AgentKey::parse("other").unwrap(),
    )
    .await;
    d.email(customer_email(&d, "dynamic-second", Some("dynamic-first")))
        .await;
    Box::pin(d.run_until("other dynamic agent completed", async || {
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
            .await
            == 2
    }))
    .await;
    choose(
        &d,
        binding,
        crate::application::agent::AgentKey::parse("assistant").unwrap(),
    )
    .await;
    d.email(customer_email(&d, "dynamic-third", Some("dynamic-first")))
        .await;
    Box::pin(
        d.run_until("original dynamic agent completed again", async || {
            d.count("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND state='succeeded'")
                .await
                == 3
        }),
    )
    .await;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('agent',conversation.agent_id,'conversation',exchange.conversation_id,'scope',conversation.scope_kind,'output',exchange.output,'seed',exchange.frozen_preparation->'history'->'seed') FROM agent_history_exchanges AS exchange JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE exchange.company_id=$1")
        .bind(d.fixture.target.company.as_uuid()).fetch_all(d.p().pool()).await.unwrap();
    assert_eq!(rows.len(), 3);
    let first = rows.iter().find(|r| r["output"] == "alpha").unwrap();
    let other = rows.iter().find(|r| r["output"] == "beta").unwrap();
    let third = rows.iter().find(|r| r["output"] == "gamma").unwrap();
    assert_eq!(first["agent"], third["agent"]);
    assert_ne!(first["agent"], other["agent"]);
    assert_eq!(first["conversation"], third["conversation"]);
    assert_ne!(first["conversation"], other["conversation"]);
    assert!(rows.iter().all(|r| r["scope"] == "thread"));
    assert_eq!(other["seed"], json!([]));
    assert_eq!(third["seed"].as_array().unwrap().len(), 1);
    assert_eq!(third["seed"][0]["output"], "alpha");
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1]["messages"].as_array().unwrap().len(), 2);
    assert_eq!(requests[2]["messages"].as_array().unwrap().len(), 4);
    assert!(requests[2].to_string().contains("alpha"));
    assert!(!requests[2].to_string().contains("beta"));
    assert_eq!(d.llm.finish().await.unwrap(), 3);
}

async fn configure_actor(d: &Deployment, binding: BindingTarget, admin: Uuid) {
    ChannelPersistence::update(
        d.p(),
        crate::application::ports::channel::ChannelUpdate {
            company_id: d.fixture.target.company.as_uuid(),
            channel_id: d.channel,
            write: ChannelWrite {
                name: "Support".into(),
                slug: "support".into(),
                enabled: true,
                participant_emails: None,
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
    let state = d
        .p()
        .binding(binding.company, binding.binding)
        .await
        .unwrap()
        .unwrap();
    d.engine
        .services()
        .bindings()
        .configure(ConfigureBinding {
            target: binding,
            expected: Some(state.revision),
            association: state.association,
            version: state
                .configuration
                .bundle()
                .compiled()
                .graph()
                .definition()
                .version_id,
            params: state.configuration.params().clone(),
            resources: state.configuration.resources().clone(),
            run_as: Some(WorkflowActor::authenticated(admin).unwrap()),
            message_trigger: None,
        })
        .await
        .unwrap();
}

async fn materialize_next(d: &Deployment) -> RunId {
    let materializer = ScheduleMaterializer::new(d.p().clone(), d.engine.services().service());
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = materializer.claim(worker).await.unwrap();
    assert_eq!(claimed.len(), 1);
    materializer.materialize(worker, &claimed[0]).await.unwrap();
    let run = scheduled_run(d, claimed[0].id).await;
    assert!(materializer.claim(worker).await.unwrap().is_empty());
    run
}

#[tokio::test]
async fn agent_history_callers_schedule_uses_current_configured_actor_and_manual_starter_keeps_own_level()
 {
    let mut d = Box::pin(Deployment::new(vec![
        LlmTurn::text("\"accepted\""),
        LlmTurn::text("\"accepted\""),
        LlmTurn::text("\"accepted\""),
    ]))
    .await;
    let admin = crate::adapters::persistence::workflow::tests::message_tests::support::member(
        &d.fixture, "admin",
    )
    .await;
    let company = d.fixture.target.company.as_uuid();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(admin)
        .execute(d.p().pool())
        .await
        .unwrap();
    let binding = d.bind(model_source(), None).await;
    configure_actor(&d, binding, admin).await;
    let schedule = schedule(&d, binding).await;
    let high = materialize_next(&d).await;
    sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(admin)
        .execute(d.p().pool())
        .await
        .unwrap();
    due(&d, schedule).await;
    let low = materialize_next(&d).await;
    let service = d.engine.services().service();
    let manual = manual::start(
        &service,
        manual_request(&d, binding, "history-manual-owner"),
    )
    .await
    .unwrap();
    let replay = manual::start(
        &service,
        manual_request(&d, binding, "history-manual-owner"),
    )
    .await
    .unwrap();
    assert!(replay.replayed);
    assert_eq!(manual.run, replay.run);
    let runs = vec![high.as_uuid(), low.as_uuid(), manual.run.as_uuid()];
    Box::pin(d.run_until("all native producer calls accepted",async||{
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND id=ANY($2) AND state='succeeded'")
            .bind(company).bind(&runs).fetch_one(d.p().pool()).await.unwrap();
        count==3
    })).await;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('run',run.id,'actor',run.actor_id,'ceiling',run.knowledge_ceiling,'session',mapping.session_id,'scope',conversation.scope_kind,'seed',exchange.frozen_preparation->'history'->'seed') FROM workflow_runs AS run JOIN workflow_agent_sessions AS mapping ON mapping.company_id=run.company_id AND mapping.run_id=run.id JOIN agent_history_exchanges AS exchange ON exchange.company_id=run.company_id AND exchange.run_id=run.id JOIN agent_history_conversations AS conversation ON conversation.company_id=exchange.company_id AND conversation.id=exchange.conversation_id WHERE run.company_id=$1 AND run.id=ANY($2)")
        .bind(company).bind(&runs).fetch_all(d.p().pool()).await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_producer_rows(
        &d,
        &rows,
        ScheduledRuns {
            high,
            low,
            manual: manual.run,
        },
        admin,
    );
    let requests = d.llm.observed();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|r| r["messages"].as_array().unwrap().len() == 2)
    );
    assert_eq!(d.llm.finish().await.unwrap(), 3);
}

struct ScheduledRuns {
    high: RunId,
    low: RunId,
    manual: RunId,
}

fn assert_producer_rows(d: &Deployment, rows: &[Value], runs: ScheduledRuns, admin: Uuid) {
    let high = rows.iter().find(|r| r["run"] == json!(runs.high)).unwrap();
    let low = rows.iter().find(|r| r["run"] == json!(runs.low)).unwrap();
    let manual = rows
        .iter()
        .find(|r| r["run"] == json!(runs.manual))
        .unwrap();
    assert_eq!(high["actor"], json!(admin));
    assert_eq!(high["ceiling"], 5);
    assert_eq!(low["actor"], json!(admin));
    assert_eq!(low["ceiling"], 0);
    assert_eq!(manual["actor"], json!(d.fixture.target.actor.user_id()));
    assert_eq!(manual["ceiling"], 9);
    let sessions: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|r| r["session"].as_str().unwrap())
        .collect();
    assert_eq!(sessions.len(), 3);
    assert!(
        rows.iter()
            .all(|r| r["scope"] == "private_session" && r["seed"] == json!([]))
    );
}
