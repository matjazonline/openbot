//! Retained producer and real cross-agent relay runtime journeys.
use super::*;

/// A person's ordinary message to the department channel, through the canonical commit.
pub(super) async fn ordinary_department_message(f: &AgentCase) -> Uuid {
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let channel = f.people.role.channel;
    let endpoint: Uuid = sqlx::query_scalar(
        "SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 \
         AND transport='email' AND status='active'",
    )
    .bind(company)
    .bind(channel)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let settings: i64 =
        sqlx::query_scalar("SELECT settings_revision FROM channels WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(channel)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let binding = ChannelBindingId::new(endpoint);
    let mut request = ordinary_request(f, binding, settings);
    request.associations = BoundedVec::parse(
        "department",
        vec![ThreadAssociation {
            address_target: crate::entities::address::AddressTarget::Desk(channel),
            binding_id: binding,
            target: ThreadTarget::Create {
                subject: "Ordinary work".into(),
            },
            role: RecipientRole::To,
            step: PipelineStep::only(),
            principals: BoundedVec::empty(),
        }],
    )
    .unwrap();
    request.intents = BoundedVec::parse(
        "intent",
        vec![InboundAdmissionIntent {
            target: crate::entities::address::AddressTarget::Desk(channel),
            role: RecipientRole::To,
            mentioned: true,
            reply_to: None,
            settings_revision: settings,
        }],
    )
    .unwrap();
    p.commit_inbound(request)
        .await
        .unwrap()
        .message_id
        .as_uuid()
}

pub(super) async fn hold(f: &AgentCase, message: Uuid) {
    sqlx::query(
        "UPDATE workflow_message_intents SET available_at=clock_timestamp()+interval '1 hour' \
         WHERE company_id=$1 AND message_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(message)
    .execute(f.people.role.p().pool())
    .await
    .unwrap();
}

pub(super) async fn release(f: &AgentCase, message: Uuid) {
    sqlx::query(
        "UPDATE workflow_message_intents SET available_at=clock_timestamp() \
         WHERE company_id=$1 AND message_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(message)
    .execute(f.people.role.p().pool())
    .await
    .unwrap();
}

/// Resume the delivered rerun to its terminal state, sending whatever it still owes.
pub(super) async fn finish(f: &AgentCase, rerun: RunId, threads: Arc<ThreadUseCases>) {
    let llm = scripted_llm(vec![LlmTurn::text(r#"{"body":"Followed up."}"#)]).await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let p = f.people.role.p();
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let stop = CancellationToken::new();
    let handles = engine.spawn(&stop);
    let finished = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            // A final reply owed by email goes out through the same relay.
            Box::pin(deliver(f, threads.clone())).await;
            let (state, active): (String, i64) = sqlx::query_as(
                "SELECT run.state, (SELECT count(*) FROM workflow_executions AS execution \
                    JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
                    WHERE execution.company_id=run.company_id AND execution.run_id=run.id AND job.status IN ('pending','processing')) \
                 FROM workflow_runs AS run WHERE run.company_id=$1 AND run.id=$2",
            )
            .bind(f.people.role.company().as_uuid())
            .bind(rerun.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
            let owed: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2 AND state<>'published'",
            )
            .bind(f.people.role.company().as_uuid())
            .bind(rerun.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
            if state == "succeeded" && active == 0 && owed == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    stop.cancel();
    for (name, handle) in handles {
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .unwrap_or_else(|_| panic!("{name} did not stop"))
            .unwrap();
    }
    let model = llm.finish().await;
    assert!(
        finished.is_ok(),
        "the delivered rerun never finished; model: {model:?}"
    );
}

/// Retire the producer's tree through the real owners: its agent history payload expires first,
/// then the aged source tree retires.
pub(super) async fn retire_producer(f: &AgentCase, run: RunId) {
    use crate::adapters::persistence::workflow::agent_history_retention_fixture as history_clock;
    use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
    let p = f.people.role.p();
    let sources: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT execution_id, job_id FROM agent_history_exchanges \
         WHERE company_id=$1 AND run_id=$2 AND payload_retained ORDER BY finished_at",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert!(
        !sources.is_empty(),
        "the rerun's agent owns retained history"
    );
    let scopes: Vec<ActivationRequest> = sources
        .into_iter()
        .map(|(execution, job)| ActivationRequest {
            company: f.people.role.company(),
            run,
            execution: crate::domain::workflow::ExecutionId::new(execution),
            job: WorkflowJobId(job),
        })
        .collect();
    for scope in &scopes {
        history_clock::expire(p, *scope).await;
    }
    history_clock::retire_source(p, scopes[0]).await;
}

/// A second company agent with its own default desk, answering the department channel.
pub(super) async fn department_agent(f: &AgentCase) -> Uuid {
    use crate::adapters::persistence::{
        company_agent::CompanyAgentService, test_support::provisioning_templates,
    };
    use crate::application::use_cases::agent::{AgentPlacement, AgentWrite, CompanyAgents};
    use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};
    let p = f.people.role.p();
    let service = CompanyAgentService::new(p.clone(), provisioning_templates());
    // The real publisher descends into agent and workflow compilation; box it.
    let created = Box::pin(service.create(
        f.people.role.admission.binding.target.actor,
        f.people.role.company(),
        AgentWrite {
            name: "Department handler".into(),
            slug: "department-handler".into(),
            tool_bindings: Some(vec![
                crate::entities::agent_capability::AgentToolBinding::builtin(
                    AgentBuiltinTool::MessageReply,
                ),
            ]),
            provider: Some(SCRIPTED_PROVIDER.into()),
            model: Some(SCRIPTED_MODEL.into()),
            ..Default::default()
        },
        Some(AgentPlacement {
            department: f.people.role.department,
            role: None,
        }),
    ))
    .await
    .unwrap();
    created.agent.id
}

/// What one leg's reply became: the canonical reply message, the email that carried it (if any),
/// and the message that email relayed into the other desk (if it was accepted).
struct Leg {
    reply: Uuid,
    delivery: Option<DeliveryId>,
    relayed: Option<Uuid>,
}

/// One leg of the exchange: `agent` replies to the message that just arrived, the real engine runs
/// it to its planned destination, and any email goes through the real worker, sender and relay.
async fn leg(
    f: &AgentCase,
    threads: &Arc<ThreadUseCases>,
    agent: Uuid,
    source: Uuid,
    models: &mut Vec<crate::services::test_support::ScriptedLlm>,
) -> Leg {
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "message__reply",
            json!({"source_message":source,"content":{"body":"Back to you."}}),
        ),
        LlmTurn::text(r#"{"body":"Handed back."}"#),
    ])
    .await;
    register_scripted_agent_base_url(agent, &llm.base_url);
    models.push(llm);
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let stop = CancellationToken::new();
    let handles = engine.spawn(&stop);
    let queued = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (reply, delivery, busy): (Option<Uuid>, Option<Uuid>, i64) = sqlx::query_as(
                "SELECT destination.message_id, destination.delivery_id, \
                  (SELECT count(*) FROM background_tasks WHERE company_id=$1 AND status IN ('pending','processing')) \
                 FROM (SELECT 1) AS anchor LEFT JOIN workflow_message_destinations AS destination \
                   ON destination.company_id=$1 AND destination.replied_message_id=$2",
            )
            .bind(company)
            .bind(source)
            .fetch_one(p.pool())
            .await
            .unwrap();
            if let (Some(reply), 0) = (reply, busy) {
                break (reply, delivery);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    stop.cancel();
    for (name, handle) in handles {
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .unwrap_or_else(|_| panic!("{name} did not stop"))
            .unwrap();
    }
    let Ok((reply, delivery)) = queued else {
        let state: Value = sqlx::query_scalar("SELECT jsonb_build_object( \
            'intents',(SELECT jsonb_agg(jsonb_build_object('message',message_id,'channel',channel_id,'origin',origin,'outcome',outcome,'hop',hop_count)) FROM workflow_message_intents WHERE company_id=$1), \
            'outcomes',(SELECT jsonb_agg(to_jsonb(selected) - 'company_id') FROM workflow_message_intent_bindings AS selected WHERE selected.company_id=$1), \
            'runs',(SELECT jsonb_agg(jsonb_build_object('id',id,'state',state,'binding',binding_id,'channel',channel_id)) FROM workflow_runs WHERE company_id=$1), \
            'attempts',(SELECT jsonb_agg(jsonb_build_object('code',attempt.workflow_failure_code,'status',attempt.status,'stop',attempt.stop_reason)) \
                FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id WHERE job.company_id=$1), \
            'destinations',(SELECT jsonb_agg(jsonb_build_object('kind',kind,'replied',replied_message_id,'delivery',delivery_id)) FROM workflow_message_destinations WHERE company_id=$1))")
            .bind(company).fetch_one(p.pool()).await.unwrap();
        panic!("the reply to {source} was never planned: {state}");
    };
    let Some(delivery) = delivery.map(DeliveryId::new) else {
        return Leg {
            reply,
            delivery: None,
            relayed: None,
        };
    };
    let smtp = Box::pin(deliver(f, threads.clone())).await;
    assert!(smtp.sent().is_empty(), "no hop of the cycle leaves by SMTP");
    let next: Option<Uuid> = sqlx::query_scalar(
        "SELECT message_id FROM message_internal_relays WHERE company_id=$1 AND delivery_id=$2",
    )
    .bind(company)
    .bind(delivery.as_uuid())
    .fetch_optional(p.pool())
    .await
    .unwrap();
    if let Some(next) = next {
        hold(f, next).await;
    }
    Leg {
        reply,
        delivery: Some(delivery),
        relayed: next,
    }
}

#[tokio::test]
async fn workflow_escalation_origin_two_agent_cross_desk_exchange_ends_in_app_with_exact_origin_and_no_case()
 {
    let (f, _source) = Box::pin(pending_agent_from_run_with_tools(&[
        AgentBuiltinTool::MessageReply,
    ]))
    .await;
    let second = Box::pin(department_agent(&f)).await;
    Box::pin(answering_binding(&f, second)).await;
    let (root, origin) = Box::pin(settle_root(&f)).await;
    let department_address = address(&f, f.people.role.channel).await;
    let (spoofed, thread) = spoofed_from_department(&f, &department_address).await;
    let before = counts(&f).await;
    Box::pin(deliver_rerun(&f, root, thread)).await;
    let threads = relay_threads(f.people.role.p());
    let mut models = Vec::new();
    // The desk agent's rerun answers the spoofed mail by email; the relay carries it to the
    // department, whose own agent answers it. An answer to a relayed message stays in-app, so the
    // exchange ends there rather than looping on the hop budget.
    let first = Box::pin(leg(&f, &threads, f.created.agent.id, spoofed, &mut models)).await;
    assert!(
        first.delivery.is_some(),
        "the rerun's answer to outside mail is an email"
    );
    let relayed = first
        .relayed
        .expect("the desk's email was relayed to the department");
    release(&f, relayed).await;
    let second_leg = Box::pin(leg(&f, &threads, second, relayed, &mut models)).await;
    assert!(
        second_leg.delivery.is_none() && second_leg.relayed.is_none(),
        "the department agent's answer to a relayed hop is in-app: nothing is mailed or relayed"
    );
    // Let the department run finish and confirm nothing new arrives anywhere.
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let stop = CancellationToken::new();
    let handles = engine.spawn(&stop);
    tokio::time::sleep(Duration::from_secs(2)).await;
    stop.cancel();
    for (name, handle) in handles {
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .unwrap_or_else(|_| panic!("{name} did not stop"))
            .unwrap();
    }
    Box::pin(deliver(&f, threads.clone())).await;
    let shape: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object( \
           'relays',(SELECT count(*) FROM message_internal_relays WHERE company_id=$1), \
           'marked',(SELECT jsonb_agg(origin.origin_id=$4 ORDER BY origin.message_id) FROM workflow_message_notification_origins AS origin \
               WHERE origin.company_id=$1 AND origin.message_id=ANY($2)), \
           'runs',(SELECT jsonb_agg(DISTINCT run.origin_id=$4) FROM workflow_run_notification_origins AS run \
               JOIN workflow_runs AS owner ON owner.company_id=run.company_id AND owner.id=run.run_id \
               WHERE run.company_id=$1 AND (owner.principal_id=$3 OR owner.channel_id=$5)), \
           'internal_intents',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND origin='internal'), \
           'deliveries',(SELECT count(*) FROM message_deliveries WHERE company_id=$1), \
           'pending',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND outcome IS NULL AND available_at<=clock_timestamp()))",
    )
    .bind(company)
    .bind(vec![first.reply, relayed, second_leg.reply])
    .bind(f.created.principal_id.as_uuid())
    .bind(origin)
    .bind(f.people.role.channel)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        shape,
        json!({"relays":1,"marked":[true,true,true],"runs":[true],"internal_intents":1,"deliveries":1,"pending":0}),
        "one relayed hop; the desk reply, the hop and the department answer all carry the exact origin"
    );
    let after = counts(&f).await;
    assert_eq!(
        after["cases"], before["cases"],
        "the exchange never started a case"
    );
    assert_eq!(
        after["roots"], before["roots"],
        "nor a second handling root"
    );
    assert_eq!(
        after["targets"], before["targets"],
        "nor another private notice"
    );
    drop(models);
}
