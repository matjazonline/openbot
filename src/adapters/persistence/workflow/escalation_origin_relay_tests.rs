//! D4.3: the actual email sender's internal relay proves the delivery part it is, inherits the
//! exact source origin, and refuses forged, foreign and mismatched claims with no canonical effect.
//!
//! The journey is the real one an origin-bearing email takes today: an authorized `deliver` rerun
//! of the private notice root replies by email to a message in its thread whose author is another
//! of the company's channels; the real delivery worker hands the frozen part to the real
//! [`EmailSender`], whose relay commits it on that channel through the canonical inbound commit.
use super::*;
use crate::adapters::protocols::email::mail::DeploymentMailTransports;
use crate::adapters::protocols::email::test_support::RecordingTransport;
use crate::adapters::protocols::email::{EmailRenderer, EmailSender, OutboundEmailV1};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::application::transport::TransportRenderers;
use crate::application::use_cases::thread::{InboundIngestPorts, ThreadStores, ThreadUseCases};
use crate::domain::workflow::MessageTrigger;
use crate::entities::channel::Channel;
use crate::entities::transport::DeliveryId;
use crate::infra::config::AppConfig;
use crate::services::test_support::{LlmTurn, register_scripted_agent_base_url, scripted_llm};
use crate::transport::{
    InternalMailRelay, InternalRelayMail, InternalRelayProof, PartIndex, RelayDisposition,
    TransportRegistry,
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const DOMAIN: &str = "agents.test";

/// The private-notice case, plus an escalating binding on the department channel that answers
/// every message with the desk agent's own published workflow. A relayed hop into that channel is
/// exactly the recursion D1.9 forbids, and an ordinary message there is exactly the work it keeps.
async fn answering_case() -> (AgentCase, RunId, WorkflowBindingId) {
    let (f, source) = Box::pin(pending_agent_from_run_with_tools(&[
        AgentBuiltinTool::MessageReply,
    ]))
    .await;
    let answering = Box::pin(answering_binding(&f, f.created.agent.id)).await;
    (f, source, answering)
}

/// An escalating binding on the department channel answering every message with `agent`'s own
/// published default-desk workflow.
async fn answering_binding(f: &AgentCase, agent: Uuid) -> WorkflowBindingId {
    let configuration: Configuration = sqlx::query_as("SELECT configuration.version_id,configuration.params,configuration.resources,configuration.run_as_user_id \
        FROM workflow_provisioning_commands AS provision JOIN workflow_bindings AS binding \
          ON binding.company_id=provision.company_id AND binding.id::text=provision.result->>'binding_id' \
        JOIN workflow_binding_revisions AS configuration ON configuration.company_id=binding.company_id AND configuration.binding_id=binding.id \
          AND configuration.revision=binding.configuration_revision WHERE provision.company_id=$1 AND provision.command_key=$2")
        .bind(f.people.role.company().as_uuid()).bind(crate::adapters::persistence::company_agent::default_desk_key(agent))
        .fetch_one(f.people.role.p().pool()).await.unwrap();
    let service = f.people.role.admission.binding.service();
    let mut request = f.people.role.admission.binding.request(None);
    request.target.binding = WorkflowBindingId::new(Uuid::new_v4());
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.people.role.channel));
    request.version = VersionId::new(configuration.version_id);
    request.params = configuration.params;
    request.resources = serde_json::from_value(configuration.resources).unwrap();
    request.run_as = Some(WorkflowActor::authenticated(configuration.run_as_user_id).unwrap());
    request.message_trigger = Some(MessageTrigger::Any);
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    let target = request.target;
    let configured = service.configure(request).await.unwrap();
    service
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    target.binding
}

fn relay_threads(p: &PostgresPersistence) -> Arc<ThreadUseCases> {
    let config = AppConfig {
        app_domain_name: DOMAIN.into(),
        ..AppConfig::for_test()
    };
    let shared = Arc::new(p.clone());
    Arc::new(ThreadUseCases::new(
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
        Arc::new(
            TransportRenderers::new()
                .register(Arc::new(EmailRenderer::new(DOMAIN)))
                .unwrap(),
        ),
        config.thread_settings(),
    ))
}

async fn address(f: &AgentCase, channel: Uuid) -> String {
    use crate::application::ports::channel::ChannelPersistence;
    use crate::application::use_cases::company::CompanyPersistence;
    let p = f.people.role.p();
    let channel = ChannelPersistence::get_by_id(p, channel)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::get_by_id(p, channel.company_id)
        .await
        .unwrap()
        .unwrap();
    Channel::address_for(&channel.slug, &company.slug, DOMAIN).to_string()
}

/// Run the notice's intended root to completion through the real worker and engine.
async fn settle_root(f: &AgentCase) -> (RunId, Uuid) {
    let llm = scripted_llm(vec![LlmTurn::text(r#"{"body":"Noted."}"#)]).await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let after = Box::pin(super::super::super::tool_tests::execute(f)).await;
    assert_eq!(after["handler_state"], "succeeded", "{after}");
    llm.finish().await.unwrap();
    let (root, origin): (Uuid, Uuid) = sqlx::query_as(
        "SELECT ownership.run_id, ownership.notice_id FROM workflow_escalation_handling_roots AS ownership \
         WHERE ownership.company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    (RunId::new(root), origin)
}

/// An external mail on the desk whose author claims the department channel's own address.
///
/// It is ordinary external work however its `From` reads: no header or address mints origin.
async fn spoofed_from_department(f: &AgentCase, author: &str) -> (Uuid, Uuid) {
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let endpoint: Uuid = sqlx::query_scalar(
        "SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 \
         AND transport='email' AND status='active'",
    )
    .bind(company)
    .bind(f.created.desk_id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let settings: i64 =
        sqlx::query_scalar("SELECT settings_revision FROM channels WHERE company_id=$1 AND id=$2")
            .bind(company)
            .bind(f.created.desk_id)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let mut request = ordinary_request(f, ChannelBindingId::new(endpoint), settings);
    request.envelope.author = qualified_email_identity(author).unwrap();
    request.origin = InboundOrigin::External;
    let outcome = p.commit_inbound(request).await.unwrap();
    // The desk's own bindings would answer it; this journey is about the rerun's reply.
    sqlx::query(
        "UPDATE workflow_message_intents SET available_at=clock_timestamp()+interval '1 hour' \
         WHERE company_id=$1 AND message_id=$2",
    )
    .bind(company)
    .bind(outcome.message_id.as_uuid())
    .execute(p.pool())
    .await
    .unwrap();
    let unmarked: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM workflow_message_notification_origins \
         WHERE company_id=$1 AND message_id=$2)",
    )
    .bind(company)
    .bind(outcome.message_id.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(
        unmarked,
        "an external sender address claiming a channel never mints origin"
    );
    (outcome.message_id.as_uuid(), outcome.thread_ids[0])
}

/// An authorized manual rerun of the root, on the spoofed mail's thread, allowed to send email.
async fn deliver_rerun(f: &AgentCase, root: RunId, thread: Uuid) -> RunId {
    let p = f.people.role.p();
    let (binding, input): (Uuid, Value) =
        sqlx::query_as("SELECT binding_id, input FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(f.people.role.company().as_uuid())
            .bind(root.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let mut request = f
        .people
        .role
        .admission
        .request("relay-rerun", f.people.role.admission.manual());
    request.binding_id = WorkflowBindingId::new(binding);
    request.association = RelatedAssociation::Thread {
        channel_id: RelatedChannelId::new(f.created.desk_id),
        thread_id: RelatedThreadId::new(thread),
    };
    request.input = input;
    request.rerun_of = Some(root);
    request.delivery_policy = crate::application::workflow::DeliveryPolicy::Deliver;
    let prepared = f.people.role.admission.prepare(request).await;
    let AdmissionResult::Created(rerun) = p.admit(&prepared).await.unwrap() else {
        panic!("the authorized deliver rerun was not created")
    };
    rerun
}

/// Run the engine until the rerun's reply is queued as an email delivery, and return it.
async fn queued_reply(f: &AgentCase, rerun: RunId, source: Uuid) -> DeliveryId {
    let llm = scripted_llm(vec![
        LlmTurn::tool_call(
            "message__reply",
            json!({"source_message":source,"content":{"body":"Escalation follow-up"}}),
        ),
        LlmTurn::text(r#"{"body":"Followed up."}"#),
    ])
    .await;
    register_scripted_agent_base_url(f.created.agent.id, &llm.base_url);
    let p = f.people.role.p();
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let stop = CancellationToken::new();
    let handles = engine.spawn(&stop);
    let queued = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let delivery: Option<Uuid> = sqlx::query_scalar(
                "SELECT delivery_id FROM workflow_message_destinations \
                 WHERE company_id=$1 AND run_id=$2 AND delivery_id IS NOT NULL",
            )
            .bind(f.people.role.company().as_uuid())
            .bind(rerun.as_uuid())
            .fetch_optional(p.pool())
            .await
            .unwrap();
            if let Some(delivery) = delivery {
                break delivery;
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
    let Ok(delivery) = queued else {
        let state: Value = sqlx::query_scalar("SELECT jsonb_build_object( \
            'run',(SELECT jsonb_build_object('state',state,'policy',delivery_policy,'thread',thread_id) FROM workflow_runs WHERE company_id=$1 AND id=$2), \
            'jobs',(SELECT jsonb_agg(jsonb_build_object('status',job.status,'step',execution.step_id)) FROM workflow_executions AS execution \
                JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
                WHERE execution.company_id=$1 AND execution.run_id=$2), \
            'attempts',(SELECT jsonb_agg(jsonb_build_object('code',attempt.workflow_failure_code,'class',attempt.workflow_failure_class,'stop',attempt.stop_reason,'status',attempt.status)) \
                FROM task_attempts AS attempt JOIN background_tasks AS job ON job.id=attempt.task_id \
                JOIN workflow_executions AS execution ON execution.company_id=job.company_id AND execution.id=job.workflow_execution_id \
                WHERE execution.company_id=$1 AND execution.run_id=$2), \
            'actions',(SELECT jsonb_agg(jsonb_build_object('state',state,'origin',origin)) FROM workflow_message_actions WHERE company_id=$1 AND run_id=$2), \
            'destinations',(SELECT jsonb_agg(jsonb_build_object('kind',kind,'delivery',delivery_id)) FROM workflow_message_destinations WHERE company_id=$1 AND run_id=$2), \
            'events',(SELECT jsonb_agg(event_kind ORDER BY sequence) FROM workflow_run_events WHERE company_id=$1 AND run_id=$2), \
            'model_calls',(SELECT count(*) FROM model_usage_calls WHERE company_id=$1))")
            .bind(f.people.role.company().as_uuid()).bind(rerun.as_uuid()).fetch_one(p.pool()).await.unwrap();
        panic!(
            "the rerun's email reply was never queued: {state}; model: {:?}",
            llm.finish().await
        );
    };
    drop(llm);
    DeliveryId::new(delivery)
}

/// The frozen part as stored, decoded the way the sender decodes it.
async fn frozen(f: &AgentCase, delivery: DeliveryId) -> OutboundEmailV1 {
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM message_delivery_parts WHERE company_id=$1 AND delivery_id=$2 AND part_index=0",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(delivery.as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    serde_json::from_value(payload["body"].clone()).unwrap()
}

/// The hop exactly as the sender would hand it over, claiming `proof`.
fn hop(email: &OutboundEmailV1, proof: InternalRelayProof) -> InternalRelayMail<'_> {
    let relay = email
        .relay
        .as_ref()
        .expect("a channel's mail carries a relay");
    let mut trace = relay.trace_channels.clone();
    if !trace.contains(&relay.source_channel_id) {
        trace.push(relay.source_channel_id);
    }
    InternalRelayMail {
        target: email
            .internal_target
            .as_ref()
            .expect("the reply is addressed to one of our channels"),
        from: &email.from,
        recipient_to: &email.recipients_to[0],
        subject: &email.subject,
        body_text: &email.body_text,
        message_id: &email.message_id,
        in_reply_to: email.in_reply_to.as_ref(),
        references: &email.references,
        source_channel_id: relay.source_channel_id,
        hop_count: relay.hop_count + 1,
        trace,
        correlation_id: email.correlation_id,
        proof,
    }
}

/// What the department channel holds: canonical messages, relay proofs and inherited origins.
async fn department(f: &AgentCase) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object( \
        'messages',(SELECT count(*) FROM thread_messages WHERE company_id=$1 AND channel_id=$2), \
        'intents',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND channel_id=$2), \
        'relays',(SELECT count(*) FROM message_internal_relays AS relay JOIN thread_messages AS membership \
            ON membership.company_id=relay.company_id AND membership.message_id=relay.message_id \
            WHERE relay.company_id=$1 AND membership.channel_id=$2), \
        'origins',(SELECT jsonb_agg(origin.origin_id) FROM workflow_message_notification_origins AS origin \
            JOIN thread_messages AS membership ON membership.company_id=origin.company_id AND membership.message_id=origin.message_id \
            WHERE origin.company_id=$1 AND membership.channel_id=$2))")
        .bind(f.people.role.company().as_uuid()).bind(f.people.role.channel)
        .fetch_one(f.people.role.p().pool()).await.unwrap()
}

/// Claims the real commit must refuse before storing anything, sent as the relay receives them.
async fn forged(
    f: &AgentCase,
    threads: &ThreadUseCases,
    email: &OutboundEmailV1,
    real: InternalRelayProof,
) {
    let company = f.people.role.company().as_uuid();
    let before = department(f).await;
    let mut tampered = email.clone();
    tampered.body_text.push_str("\nAnd one more thing.");
    let cases: [(&str, &OutboundEmailV1, InternalRelayProof); 4] = [
        (
            "a part the delivery does not have",
            email,
            InternalRelayProof::claiming(company, real.delivery_id(), PartIndex::new(1)),
        ),
        (
            "a delivery that does not exist",
            email,
            InternalRelayProof::claiming(company, DeliveryId::random(), PartIndex::new(0)),
        ),
        (
            "another tenant's claim",
            email,
            InternalRelayProof::claiming(Uuid::new_v4(), real.delivery_id(), PartIndex::new(0)),
        ),
        ("content the part did not freeze", &tampered, real),
    ];
    for (case, mail, proof) in cases {
        let disposition = threads.relay_internal(&hop(mail, proof)).await.unwrap();
        assert!(
            matches!(&disposition, RelayDisposition::Refused(reason)
                if reason.contains("does not match the delivery part")),
            "{case}: {disposition:?}"
        );
        assert_eq!(department(f).await, before, "{case} stored nothing");
    }
}

/// Hand every due delivery to the real worker, sending through the real email sender.
async fn deliver(f: &AgentCase, threads: Arc<ThreadUseCases>) -> Arc<RecordingTransport> {
    let p = f.people.role.p();
    let smtp = Arc::new(RecordingTransport::default());
    let transports = TransportRegistry::new()
        .register(
            Arc::new(EmailRenderer::new(DOMAIN)),
            Arc::new(EmailSender::new(
                DeploymentMailTransports::only(smtp.clone()),
                threads,
            )),
        )
        .unwrap();
    let worker = DeliveryWorker::new(Arc::new(p.clone()), Arc::new(transports));
    let (_shutdown, mut receiver) = tokio::sync::broadcast::channel(1);
    worker.drain_once(&mut receiver).await.unwrap();
    smtp
}

async fn delivered(f: &AgentCase, delivery: DeliveryId) -> String {
    sqlx::query_scalar("SELECT status FROM message_deliveries WHERE company_id=$1 AND id=$2")
        .bind(f.people.role.company().as_uuid())
        .bind(delivery.as_uuid())
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn workflow_escalation_origin_actual_email_relay_proves_its_part_and_inherits_exact_origin() {
    let (f, _source, answering) = Box::pin(answering_case()).await;
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let (root, origin) = Box::pin(settle_root(&f)).await;
    let department_address = address(&f, f.people.role.channel).await;
    let (spoofed, thread) = spoofed_from_department(&f, &department_address).await;
    let rerun = Box::pin(deliver_rerun(&f, root, thread)).await;
    let inherited: Uuid = sqlx::query_scalar(
        "SELECT origin_id FROM workflow_run_notification_origins WHERE company_id=$1 AND run_id=$2",
    )
    .bind(company)
    .bind(rerun.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        inherited, origin,
        "the authorized rerun inherits the root's origin"
    );
    let delivery = Box::pin(queued_reply(&f, rerun, spoofed)).await;
    let email = frozen(&f, delivery).await;
    assert_eq!(email.recipients_to[0].to_string(), department_address);
    let real = InternalRelayProof::claiming(company, delivery, PartIndex::new(0));
    let threads = relay_threads(p);
    let before = counts(&f).await;

    Box::pin(forged(&f, &threads, &email, real)).await;

    let smtp = Box::pin(deliver(&f, threads.clone())).await;
    assert!(smtp.sent().is_empty(), "an internal hop never reaches SMTP");
    assert_eq!(delivered(&f, delivery).await, "delivered");
    #[derive(sqlx::FromRow)]
    struct Relayed {
        message_id: Uuid,
        origin_id: Uuid,
        intent_origin: String,
    }
    let relayed: Relayed = sqlx::query_as("SELECT relay.message_id, origin.origin_id, intent.origin AS intent_origin \
        FROM message_internal_relays AS relay \
        JOIN workflow_message_notification_origins AS origin ON origin.company_id=relay.company_id AND origin.message_id=relay.message_id \
        JOIN workflow_message_intents AS intent ON intent.company_id=relay.company_id AND intent.message_id=relay.message_id \
        WHERE relay.company_id=$1 AND relay.delivery_id=$2 AND relay.part_index=0 AND relay.source_channel_id=$3 \
          AND intent.channel_id=$4")
        .bind(company).bind(delivery.as_uuid()).bind(f.created.desk_id).bind(f.people.role.channel)
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(
        relayed.origin_id, origin,
        "the relayed hop inherits its source message's exact origin"
    );
    assert_eq!(relayed.intent_origin, "internal");
    let after_relay = department(&f).await;

    // The same part again is a redelivery and stores nothing; a mismatched claim on the same
    // RFC id is no redelivery at all.
    assert_eq!(
        threads.relay_internal(&hop(&email, real)).await.unwrap(),
        RelayDisposition::Relayed
    );
    assert!(matches!(
        threads
            .relay_internal(&hop(
                &email,
                InternalRelayProof::claiming(company, DeliveryId::random(), PartIndex::new(0))
            ))
            .await
            .unwrap(),
        RelayDisposition::Refused(_)
    ));
    assert_eq!(department(&f).await, after_relay);

    // The producer finishes and its whole tree retires through the real retention owner before
    // anything consumes the hop. The hop's origin is its own sealed fact, not a pointer into it.
    hold(&f, relayed.message_id).await;
    Box::pin(finish(&f, rerun, threads.clone())).await;
    Box::pin(retire_producer(&f, rerun)).await;
    let retained: (i64, Option<Uuid>) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND id=$3), \
                (SELECT origin_id FROM workflow_message_notification_origins WHERE company_id=$1 AND message_id=$2)",
    )
    .bind(company)
    .bind(relayed.message_id)
    .bind(rerun.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        retained,
        (0, Some(origin)),
        "the producer retired; the hop keeps its origin"
    );
    let proof: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND id=$2), \
                (SELECT count(*) FROM message_internal_relays WHERE company_id=$1 AND delivery_id=$2)",
    )
    .bind(company)
    .bind(delivery.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        proof.0, proof.1,
        "a relay proof lives exactly as long as its delivery"
    );
    release(&f, relayed.message_id).await;

    // The department's answering binding admits the relayed hop. The run inherits the exact
    // origin, and its failure starts no case: it is the notice's own work coming back.
    for claim in claims(&f).await {
        process(&f, claim).await;
    }
    let hop_run = admitted_from(&f, relayed.message_id, answering).await;
    assert_eq!(run_origin(&f, hop_run).await, Some(origin));
    fail_on_guard(&f, hop_run).await;
    let after = counts(&f).await;
    assert_eq!(
        after["cases"], before["cases"],
        "a relayed hop's failure starts no new case"
    );
    assert_eq!(after["roots"], before["roots"]);

    // Ordinary work on the same channel keeps its own ladder.
    let ordinary = ordinary_department_message(&f).await;
    for claim in claims(&f).await {
        process(&f, claim).await;
    }
    let ordinary_run = admitted_from(&f, ordinary, answering).await;
    assert_eq!(run_origin(&f, ordinary_run).await, None);
    fail_on_guard(&f, ordinary_run).await;
    assert_eq!(
        counts(&f).await["cases"],
        before["cases"].as_i64().unwrap() + 1,
        "ordinary work on the department channel still escalates"
    );

    Box::pin(denied_destinations(&f, &threads, &email, real)).await;
    Box::pin(database_negatives(
        &f,
        delivery,
        relayed.message_id,
        ordinary,
        origin,
    ))
    .await;
}

/// A hop to another tenant's channel, or back to its own, is refused before canonical acceptance.
async fn denied_destinations(
    f: &AgentCase,
    threads: &ThreadUseCases,
    email: &OutboundEmailV1,
    real: InternalRelayProof,
) {
    use crate::application::ports::channel::{ChannelPersistence, ChannelWrite};
    use crate::entities::transport::ChannelSelector;
    use crate::entities::value_objects::{ChannelSlug, CompanySlug};
    let p = f.people.role.p();
    let foreign = Uuid::new_v4();
    let foreign_slug = format!("foreign-{}", foreign.simple());
    crate::adapters::persistence::test_support::insert_company_with_owner(
        p.pool(),
        foreign,
        "Foreign",
        &foreign_slug,
        f.people.role.admission.binding.target.actor.user_id(),
    )
    .await;
    ChannelPersistence::create(
        p,
        foreign,
        ChannelWrite {
            name: "Support".into(),
            slug: "support".into(),
            enabled: true,
            ..ChannelWrite::default()
        },
    )
    .await
    .unwrap();
    let desk = ChannelPersistence::get_by_id(p, f.created.desk_id)
        .await
        .unwrap()
        .unwrap();
    let company_messages = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM messages WHERE company_id=ANY($1)")
            .bind(vec![foreign, f.people.role.company().as_uuid()])
            .fetch_one(p.pool())
            .await
            .unwrap()
    };
    let before = company_messages().await;
    let company: CompanySlug =
        sqlx::query_scalar::<_, String>("SELECT slug FROM companies WHERE id=$1")
            .bind(f.people.role.company().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap()
            .into();
    for (case, target, expected) in [
        (
            "another company's channel",
            ChannelSelector::Qualified {
                company: CompanySlug::from(foreign_slug.clone()),
                channel: ChannelSlug::from("support".to_string()),
            },
            "Cross-company internal channel delivery is not allowed",
        ),
        (
            "the sending channel itself",
            ChannelSelector::Qualified {
                company: company.clone(),
                channel: desk.slug.clone(),
            },
            "cannot deliver an internal message to itself",
        ),
    ] {
        let mut mail = hop(email, real);
        mail.target = &target;
        let refused = threads.relay_internal(&mail).await.unwrap_err();
        assert!(refused.to_string().contains(expected), "{case}: {refused}");
        assert_eq!(company_messages().await, before, "{case} stored nothing");
    }
}

/// The schema refuses every fabricated relay fact on its own, whoever writes it.
async fn database_negatives(
    f: &AgentCase,
    delivery: DeliveryId,
    relayed: Uuid,
    ordinary: Uuid,
    origin: Uuid,
) {
    let p = f.people.role.p();
    let company = f.people.role.company().as_uuid();
    let rejected = |error: sqlx::Error, expected: &str| {
        let database = error.as_database_error().expect("a database refusal");
        assert_eq!(database.code().as_deref(), Some("23514"), "{error}");
        assert!(error.to_string().contains(expected), "{error}");
    };
    // An ordinary message cannot claim a real part it is not.
    rejected(
        sqlx::query("INSERT INTO message_internal_relays(company_id,message_id,delivery_id,part_index,source_channel_id) \
            VALUES($1,$2,$3,0,$4)")
            .bind(company).bind(ordinary).bind(delivery.as_uuid()).bind(f.created.desk_id)
            .execute(p.pool()).await.unwrap_err(),
        "must prove the exact delivery part",
    );
    // Nor acquire the hop's origin without a relay proof.
    rejected(
        sqlx::query("INSERT INTO workflow_message_notification_origins(company_id,message_id,origin_id) VALUES($1,$2,$3)")
            .bind(company).bind(ordinary).bind(origin).execute(p.pool()).await.unwrap_err(),
        "requires its exact canonical producer",
    );
    // An internal intent must belong to a proved hop: the rerun's own reply, which has an
    // association on the desk and no intent of its own, cannot be claimed as one.
    let (reply, channel, thread): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT delivery.message_id, membership.channel_id, membership.thread_id \
         FROM message_deliveries AS delivery JOIN thread_messages AS membership \
           ON membership.company_id=delivery.company_id AND membership.message_id=delivery.message_id \
         WHERE delivery.company_id=$1 AND delivery.id=$2",
    )
    .bind(company)
    .bind(delivery.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *tx)
        .await
        .unwrap();
    rejected(
        sqlx::query("INSERT INTO workflow_message_intents (company_id, id, message_id, channel_id, thread_id, role, \
                mentioned, admissible, origin, hop_count, trace_channels, delivery_policy, correlation_id) \
            VALUES ($1, $2, $3, $4, $5, 'to', false, true, 'internal', 1, '{}', 'deliver', $2)")
            .bind(company).bind(Uuid::new_v4()).bind(reply).bind(channel).bind(thread)
            .execute(&mut *tx).await.unwrap_err(),
        "must retain its exact delivery proof",
    );
    drop(tx);
    // The proof itself is immutable while its message, delivery and part exist.
    rejected(
        sqlx::query("UPDATE message_internal_relays SET part_index=part_index WHERE company_id=$1 AND message_id=$2")
            .bind(company).bind(relayed).execute(p.pool()).await.unwrap_err(),
        "is immutable",
    );
    rejected(
        sqlx::query("DELETE FROM message_internal_relays WHERE company_id=$1 AND message_id=$2")
            .bind(company)
            .bind(relayed)
            .execute(p.pool())
            .await
            .unwrap_err(),
        "is immutable",
    );
}

async fn admitted_from(f: &AgentCase, message: Uuid, binding: WorkflowBindingId) -> RunId {
    RunId::new(
        sqlx::query_scalar(
            "SELECT run_id FROM workflow_admissions WHERE company_id=$1 AND binding_id=$2 \
             AND workflow_escalation_source_message(source_key)=$3",
        )
        .bind(f.people.role.company().as_uuid())
        .bind(binding.as_uuid())
        .bind(message)
        .fetch_one(f.people.role.p().pool())
        .await
        .unwrap(),
    )
}

async fn run_origin(f: &AgentCase, run: RunId) -> Option<Uuid> {
    sqlx::query_scalar(
        "SELECT origin_id FROM workflow_run_notification_origins WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(f.people.role.p().pool())
    .await
    .unwrap()
}

async fn fail_on_guard(f: &AgentCase, run: RunId) {
    use crate::application::app_error::WorkflowGuardViolation;
    use crate::application::workflow::activation::{ActivationRequest, WorkflowJobId};
    use crate::application::workflow::guard::{
        GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
    };
    let p = f.people.role.p();
    let (execution, job): (Uuid, Uuid) = sqlx::query_as(
        "SELECT execution.id, job.id FROM workflow_executions AS execution \
         JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id \
         WHERE execution.company_id=$1 AND execution.run_id=$2 AND execution.completed_at IS NULL",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(run.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let scope = ActivationRequest {
        company: f.people.role.company(),
        run,
        execution: crate::domain::workflow::ExecutionId::new(execution),
        job: WorkflowJobId(job),
    };
    assert_eq!(
        p.fail_run_on_guard(GuardFailure {
            scope,
            violation: WorkflowGuardViolation::from_constraint(
                "workflow_guard:test_escalation_guard",
                "forced guard"
            )
            .unwrap(),
            site: GuardSite::Step,
            fence: None,
        })
        .await
        .unwrap(),
        GuardFailureOutcome::Failed
    );
}

/// A person's ordinary message to the department channel, through the canonical commit.
async fn ordinary_department_message(f: &AgentCase) -> Uuid {
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
            channel_id: channel,
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
            channel_id: channel,
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

async fn hold(f: &AgentCase, message: Uuid) {
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

async fn release(f: &AgentCase, message: Uuid) {
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
async fn finish(f: &AgentCase, rerun: RunId, threads: Arc<ThreadUseCases>) {
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
async fn retire_producer(f: &AgentCase, run: RunId) {
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
async fn department_agent(f: &AgentCase) -> Uuid {
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
               WHERE run.company_id=$1 AND owner.channel_id IN ($3,$5)), \
           'internal_intents',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND origin='internal'), \
           'deliveries',(SELECT count(*) FROM message_deliveries WHERE company_id=$1), \
           'pending',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1 AND outcome IS NULL AND available_at<=clock_timestamp()))",
    )
    .bind(company)
    .bind(vec![first.reply, relayed, second_leg.reply])
    .bind(f.created.desk_id)
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
