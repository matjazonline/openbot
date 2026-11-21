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

#[path = "escalation_origin_relay_runtime_tests.rs"]
mod runtime;
use runtime::*;
#[path = "relay_identity_tests.rs"]
mod identity;

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
        FROM principals AS principal JOIN channel_reply_settings AS response \
          ON response.company_id=principal.company_id AND response.principal_id=principal.id \
        JOIN workflow_bindings AS binding ON binding.company_id=response.company_id AND binding.id=response.binding_id \
        JOIN workflow_binding_revisions AS configuration ON configuration.company_id=binding.company_id AND configuration.binding_id=binding.id \
          AND configuration.revision=binding.configuration_revision WHERE principal.company_id=$1 AND principal.agent_id=$2")
        .bind(f.people.role.company().as_uuid()).bind(agent)
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
    // This case tests forged origin on ordinary external traffic. Open only this direct
    // endpoint explicitly; an observed From never grants private-agent participation.
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(f.created.principal_id.as_uuid())
        .execute(p.pool())
        .await
        .unwrap();
    let endpoint: Uuid = sqlx::query_scalar(
        "SELECT id FROM channel_bindings WHERE company_id=$1 AND principal_id=$2 \
         AND transport='email' AND status='active'",
    )
    .bind(company)
    .bind(f.created.principal_id.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    let settings: i64 = sqlx::query_scalar(
        "SELECT settings_revision FROM principals WHERE company_id=$1 AND id=$2",
    )
    .bind(company)
    .bind(f.created.principal_id.as_uuid())
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
        target: crate::entities::address::AddressTarget::Principal(f.created.principal_id),
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
    let mut trace = relay.trace_targets.clone();
    if !trace.contains(&relay.source_target) {
        trace.push(relay.source_target);
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
        source_target: relay.source_target,
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
                if reason == "Internal source proof does not match its delivery"),
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
    let failure: Option<String> =
        sqlx::query_scalar("SELECT last_error_detail FROM message_deliveries WHERE id=$1")
            .bind(delivery.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(delivered(&f, delivery).await, "delivered", "{failure:?}");
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
        WHERE relay.company_id=$1 AND relay.delivery_id=$2 AND relay.part_index=0 AND relay.source_principal_id=$3 \
          AND intent.channel_id=$4")
        .bind(company).bind(delivery.as_uuid()).bind(f.created.principal_id.as_uuid()).bind(f.people.role.channel)
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
    use crate::application::ports::channel::ChannelWrite;
    use crate::entities::transport::AddressSelector;
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
    crate::adapters::persistence::test_support::create_channel(
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
    let source_slug = ChannelSlug::from(f.created.agent.slug.as_str());
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
            AddressSelector::Qualified {
                company: CompanySlug::from(foreign_slug.clone()),
                channel: ChannelSlug::from("support".to_string()),
            },
            "Cross-company internal delivery is not allowed",
        ),
        (
            "the sending channel itself",
            AddressSelector::Qualified {
                company: company.clone(),
                channel: source_slug.clone(),
            },
            "An inbox cannot relay a message to itself",
        ),
    ] {
        let mut mail = hop(email, real);
        mail.target = &target;
        let refused = threads.relay_internal(&mail).await.unwrap();
        assert!(
            matches!(&refused, RelayDisposition::Refused(reason) if reason == expected),
            "{case}: {refused:?}"
        );
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
        sqlx::query("INSERT INTO message_internal_relays(company_id,message_id,delivery_id,part_index,source_principal_id) \
            VALUES($1,$2,$3,0,$4)")
            .bind(company).bind(ordinary).bind(delivery.as_uuid()).bind(f.created.principal_id.as_uuid())
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
    let (reply, principal, thread): (Uuid, Uuid, Uuid) = sqlx::query_as(
        "SELECT delivery.message_id, membership.principal_id, membership.thread_id \
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
        sqlx::query("INSERT INTO workflow_message_intents (company_id, id, message_id, principal_id, thread_id, role, \
                mentioned, admissible, origin, hop_count, trace_targets, delivery_policy, correlation_id) \
            VALUES ($1, $2, $3, $4, $5, 'to', false, true, 'internal', 1, '[]', 'deliver', $2)")
            .bind(company).bind(Uuid::new_v4()).bind(reply).bind(principal).bind(thread)
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
