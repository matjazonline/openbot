//! Actual binding configuration, canonical ingress, candidate readers and admission.
//! These cases establish targeted zero/one admission and ordinary ingress fan-out;
//! origin inheritance and real handler terminal/ack outcomes remain separate checks.
use super::*;
#[path = "escalation_agent_expiry_tests.rs"]
mod expiry_tests;
#[path = "escalation_agent_guard_tests.rs"]
mod guard_tests;
#[path = "escalation_origin_retention_tests.rs"]
mod origin_retention_tests;
#[path = "escalation_agent_role_lock_tests.rs"]
mod role_lock_tests;
#[path = "escalation_agent_terminal_tests.rs"]
mod terminal_tests;
use crate::adapters::persistence::workflow::admission_binding::SourceKey;
use crate::application::ports::thread::{ProtocolExtension, qualified_email_identity};
use crate::application::workflow::message_admission::{
    MessageAdmissions, MessageIntentQueue, MessageIntentWorkerId,
};
use crate::entities::email_message::EmailMessageMetadata;
use crate::entities::transport::{
    ChannelBindingId, ExternalMessageKey, ExternalThreadKey, InboundSource, RecipientRole,
};
use crate::entities::value_objects::MessageId;
use crate::transport::{
    AddressedIdentity, BoundedVec, CanonicalContent, InboundAdmissionIntent, InboundCommitRequest,
    InboundEnvelope, InboundMessageCommitter, InboundOrigin, IngressDirectives, IngressPolicyFacts,
    PipelineStep, ReplyDelivery, ThreadAssociation, ThreadTarget,
};

#[derive(sqlx::FromRow)]
struct Configuration {
    version_id: Uuid,
    params: Value,
    resources: Value,
    run_as_user_id: Uuid,
}

async fn own_default_desk(f: &AgentCase) {
    use crate::application::ports::channel::{
        ChannelOwnershipWrite, ChannelPersistence, ChannelUpdate, ChannelWrite,
    };
    let p = f.people.role.p();
    let channel = ChannelPersistence::get_by_id(p, f.created.desk_id)
        .await
        .unwrap()
        .unwrap();
    ChannelPersistence::update(
        p,
        ChannelUpdate {
            company_id: f.people.role.company().as_uuid(),
            channel_id: channel.id,
            write: ChannelWrite {
                name: channel.name,
                slug: channel.slug.to_string(),
                enabled: true,
                ownership: Some(ChannelOwnershipWrite {
                    department_id: Some(Some(f.people.role.department)),
                    members_only: Some(true),
                }),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
}

async fn other_bindings(f: &AgentCase, count: usize) -> Vec<WorkflowBindingId> {
    let configuration:Configuration=sqlx::query_as("SELECT configuration.version_id,configuration.params,configuration.resources,configuration.run_as_user_id \
        FROM workflow_provisioning_commands AS provision JOIN workflow_bindings AS binding \
          ON binding.company_id=provision.company_id AND binding.id::text=provision.result->>'binding_id' \
        JOIN workflow_binding_revisions AS configuration ON configuration.company_id=binding.company_id AND configuration.binding_id=binding.id \
          AND configuration.revision=binding.configuration_revision WHERE provision.company_id=$1 AND provision.command_key=$2")
        .bind(f.people.role.company().as_uuid()).bind(crate::adapters::persistence::company_agent::default_desk_key(f.created.agent.id))
        .fetch_one(f.people.role.p().pool()).await.unwrap();
    let service = f.people.role.admission.binding.service();
    let mut ids = Vec::new();
    for _ in 0..count {
        let mut request = f.people.role.admission.binding.request(None);
        request.target.binding = WorkflowBindingId::new(Uuid::new_v4());
        request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.created.desk_id));
        request.version = VersionId::new(configuration.version_id);
        request.params = configuration.params.clone();
        request.resources = serde_json::from_value(configuration.resources.clone()).unwrap();
        request.run_as = Some(WorkflowActor::authenticated(configuration.run_as_user_id).unwrap());
        request.message_trigger = Some(MessageTrigger::Any);
        let target = request.target;
        let configured = service.configure(request).await.unwrap();
        service
            .activate(SetBindingActivity {
                target,
                expected: configured.revision,
            })
            .await
            .unwrap();
        ids.push(target.binding);
    }
    ids
}

async fn claims(
    f: &AgentCase,
) -> Vec<crate::application::workflow::message_admission::ClaimedIntent> {
    f.people
        .role
        .p()
        .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), 32)
        .await
        .unwrap()
}

async fn process(
    f: &AgentCase,
    claim: crate::application::workflow::message_admission::ClaimedIntent,
) {
    let p = f.people.role.p();
    let admission = WorkflowService::new(p.clone(), p.clone(), p.clone(), p.clone(), Preflight);
    MessageAdmissions::new(p.clone(), admission, p.clone())
        .process(claim)
        .await
        .unwrap();
}

async fn ordinary(f: &AgentCase) {
    let endpoint:Uuid=sqlx::query_scalar("SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 AND transport='email' AND status='active'")
        .bind(f.people.role.company().as_uuid()).bind(f.created.desk_id).fetch_one(f.people.role.p().pool()).await.unwrap();
    let settings: i64 =
        sqlx::query_scalar("SELECT settings_revision FROM channels WHERE company_id=$1 AND id=$2")
            .bind(f.people.role.company().as_uuid())
            .bind(f.created.desk_id)
            .fetch_one(f.people.role.p().pool())
            .await
            .unwrap();
    let request = ordinary_request(f, ChannelBindingId::new(endpoint), settings);
    f.people.role.p().commit_inbound(request).await.unwrap();
}

fn ordinary_request(
    f: &AgentCase,
    binding: ChannelBindingId,
    settings: i64,
) -> InboundCommitRequest {
    let key = format!("<ordinary-{}@example.test>", Uuid::new_v4());
    let recipient = qualified_email_identity("desk@example.test").unwrap();
    InboundCommitRequest {
        company_id: f.people.role.company().as_uuid(),
        envelope: InboundEnvelope {
            source: InboundSource {
                binding_id: binding,
                event_key: None,
                message_key: ExternalMessageKey::parse(&key).unwrap(),
                thread_key: ExternalThreadKey::parse(&key).unwrap(),
            },
            author: qualified_email_identity("person@example.test").unwrap(),
            addressed: BoundedVec::parse(
                "recipient",
                vec![AddressedIdentity::new(RecipientRole::To, recipient)],
            )
            .unwrap(),
            content: CanonicalContent::parse("Ordinary work", "Please help").unwrap(),
            attachments: BoundedVec::empty(),
            reply_candidates: Default::default(),
            directives: IngressDirectives::default(),
            policy: IngressPolicyFacts::TrustedApplication,
            correlation_id: CorrelationId::new(),
            extension: ProtocolExtension::email(EmailMessageMetadata::new(MessageId::from(key))),
        },
        claimed_event: None,
        associations: BoundedVec::parse(
            "desk",
            vec![ThreadAssociation {
                channel_id: f.created.desk_id,
                binding_id: binding,
                target: ThreadTarget::Create {
                    subject: "Ordinary work".into(),
                },
                role: RecipientRole::To,
                step: PipelineStep::only(),
                principals: BoundedVec::empty(),
            }],
        )
        .unwrap(),
        intents: BoundedVec::parse(
            "intent",
            vec![InboundAdmissionIntent {
                channel_id: f.created.desk_id,
                role: RecipientRole::To,
                mentioned: true,
                reply_to: None,
                settings_revision: settings,
            }],
        )
        .unwrap(),
        admissible: true,
        origin: InboundOrigin::Application,
        deliveries: vec![],
        reply_delivery: ReplyDelivery::InAppOnly,
    }
}

#[tokio::test]
async fn workflow_escalation_agent_actual_admission_selects_one_of_eight_and_ordinary_ingress_keeps_fanout()
 {
    let f = Box::pin(pending_agent()).await;
    let others = other_bindings(&f, 7).await;
    assert_eq!(
        notification_worker(f.people.role.p(), Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let notice = claims(&f).await.pop().unwrap();
    let selected = f.people.role.p().candidate_bindings(&notice).await.unwrap();
    assert_eq!(selected.len(), 1);
    for other in &others {
        assert!(
            f.people
                .role
                .p()
                .candidate_binding(&notice, *other)
                .await
                .unwrap()
                .is_none()
        );
    }
    guard_tests::reject_requested_binding(&f, &notice, others[0]).await;
    process(&f, notice).await;
    assert_eq!(
        facts(&f).await["runs"],
        1,
        "only the captured default binding admits the notice"
    );
    let roots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_handling_roots WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(
        roots, 1,
        "the admitted root has one immutable intended-origin ownership link"
    );
    let open: bool = sqlx::query_scalar(
        "SELECT state='waiting' FROM workflow_escalation_progress WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap();
    assert!(
        open,
        "admission never acknowledges the administrative decision"
    );
    ordinary(&f).await;
    let ordinary = claims(&f).await.pop().unwrap();
    assert_eq!(
        f.people
            .role
            .p()
            .candidate_bindings(&ordinary)
            .await
            .unwrap()
            .len(),
        8
    );
    process(&f, ordinary).await;
    assert_eq!(
        facts(&f).await["runs"],
        9,
        "ordinary ingress keeps all eight valid matching bindings"
    );
    assert_private_origin_guards(&f).await;
}

#[tokio::test]
async fn workflow_escalation_agent_changed_captured_revision_exposes_no_fallback_candidate() {
    let f = Box::pin(pending_agent()).await;
    other_bindings(&f, 1).await;
    assert_eq!(
        notification_worker(f.people.role.p(), Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let notice = claims(&f).await.pop().unwrap();
    let selected = f.people.role.p().candidate_bindings(&notice).await.unwrap();
    assert_eq!(selected.len(), 1);
    let service = f.people.role.admission.binding.service();
    let mut request = f.people.role.admission.binding.request(None);
    request.target.binding = selected[0].id;
    let state = f
        .people
        .role
        .p()
        .binding(request.target.company, request.target.binding)
        .await
        .unwrap()
        .unwrap();
    request.expected = Some(state.revision);
    let row:Configuration=sqlx::query_as("SELECT version_id,params,resources,run_as_user_id FROM workflow_binding_revisions WHERE company_id=$1 AND binding_id=$2 AND revision=$3")
        .bind(f.people.role.company().as_uuid()).bind(selected[0].id.as_uuid()).bind(i64::try_from(selected[0].revision.get()).unwrap())
        .fetch_one(f.people.role.p().pool()).await.unwrap();
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.created.desk_id));
    request.version = VersionId::new(row.version_id);
    request.params = row.params;
    request.resources = serde_json::from_value(row.resources).unwrap();
    request.run_as = Some(WorkflowActor::authenticated(row.run_as_user_id).unwrap());
    request.message_trigger = Some(MessageTrigger::Any);
    service.configure(request).await.unwrap();
    assert!(
        f.people
            .role
            .p()
            .candidate_bindings(&notice)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        f.people
            .role
            .p()
            .candidate_binding(&notice, selected[0].id)
            .await
            .unwrap()
            .is_none()
    );
    process(&f, notice).await;
    assert_eq!(facts(&f).await["runs"], 0);
    let failed:i64=sqlx::query_scalar("SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1 AND reason_code='escalation.notification_route_changed'")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert_eq!(
        failed, 1,
        "a changed captured route must fail on the original recipient without a fallback root"
    );
    assert_eq!(
        notification_worker(f.people.role.p(), Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_escalation_progress WHERE company_id=$1")
            .bind(f.people.role.company().as_uuid())
            .fetch_one(f.people.role.p().pool())
            .await
            .unwrap();
    assert_eq!(state, "owner_waiting");
    assert_eq!(facts(&f).await["cases"], 1);
    assert_eq!(facts(&f).await["hops"], 1);
}

#[tokio::test]
async fn workflow_escalation_agent_source_message_sql_matches_existing_typed_decoder() {
    let database = crate::adapters::persistence::test_support::own_database()
        .await
        .unwrap();
    let id = Uuid::new_v4();
    let mut cases = vec![
        "bad".to_string(),
        "v1:bad".into(),
        "v1:null".into(),
        "v1:42".into(),
        "v1:{}".into(),
        "v1:[]".into(),
        "v1:[\"message\"]".into(),
        "v1:[\"message\",42]".into(),
        format!("v1:[\"manual\",\"{id}\"]"),
        format!("v1:[\"message\",\"{id}\",\"extra\"]"),
    ];
    for value in [
        id.to_string(),
        id.simple().to_string(),
        id.to_string().to_uppercase(),
        format!("{{{id}}}"),
        format!("urn:uuid:{id}"),
    ] {
        cases.push(format!("v1:[\"message\",\"{value}\"]"));
    }
    for source in cases {
        let expected = SourceKey::message(&source);
        let actual: Option<Uuid> =
            sqlx::query_scalar("SELECT workflow_escalation_source_message($1)")
                .bind(&source)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(actual, expected, "source {source}");
    }
    let oversized = format!("v1:{}[\"message\",\"{id}\"]", " ".repeat(512));
    let actual: Option<Uuid> = sqlx::query_scalar("SELECT workflow_escalation_source_message($1)")
        .bind(oversized)
        .fetch_one(&database.pool)
        .await
        .unwrap();
    assert!(
        actual.is_none(),
        "the SQL boundary rejects input beyond the stored source-key contract"
    );
}

#[tokio::test]
async fn workflow_escalation_agent_company_teardown_removes_private_roots_and_origin_receipts() {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    assert_eq!(
        notification_worker(p, Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    process(&f, claims(&f).await.pop().unwrap()).await;
    let roots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_handling_roots WHERE company_id=$1",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(roots, 1);
    CompanyPersistence::delete(p, f.people.role.company().as_uuid())
        .await
        .unwrap();
    for table in [
        "workflow_escalation_handling_roots",
        "workflow_escalation_agent_receipts",
        "workflow_escalation_notice_targets",
        "workflow_run_notification_origins",
        "workflow_message_notification_origins",
        "workflow_notification_origins",
        "workflow_message_producer_seals",
        "workflow_escalation_cases",
        "workflow_message_intents",
        "messages",
        "notification_events",
    ] {
        let remaining: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE company_id=$1"))
                .bind(f.people.role.company().as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(remaining, 0, "{table} outlived its company");
    }
}

async fn assert_private_origin_guards(f: &AgentCase) {
    let forged=sqlx::query("INSERT INTO workflow_escalation_handling_roots(company_id,notice_id,run_id,message_id,intent_id,binding_id,binding_revision) \
        SELECT receipt.company_id,receipt.notice_id,run.id,receipt.message_id,receipt.intent_id,receipt.binding_id,receipt.binding_revision \
        FROM workflow_escalation_agent_receipts AS receipt JOIN workflow_runs AS run ON run.company_id=receipt.company_id AND run.binding_id=receipt.binding_id \
        WHERE receipt.company_id=$1 AND NOT EXISTS(SELECT 1 FROM workflow_escalation_handling_roots AS ownership WHERE ownership.company_id=run.company_id AND ownership.run_id=run.id)")
        .bind(f.people.role.company().as_uuid()).execute(f.people.role.p().pool()).await.unwrap_err();
    assert_eq!(
        forged.as_database_error().unwrap().constraint(),
        Some("workflow_escalation_handling_root_guard"),
        "an unrelated real root of the SAME binding cannot claim the notice"
    );
    let public=sqlx::query("UPDATE workflow_message_intents SET delivery_policy='deliver' WHERE company_id=$1 AND id=(SELECT intent_id FROM workflow_escalation_agent_receipts WHERE company_id=$1)")
        .bind(f.people.role.company().as_uuid()).execute(f.people.role.p().pool()).await.unwrap_err();
    assert_eq!(
        public.as_database_error().unwrap().code().as_deref(),
        Some("23514"),
        "{public}"
    );
    assert_eq!(
        public.as_database_error().unwrap().message(),
        "channel response ingress facts are immutable"
    );
    let private:bool=sqlx::query_scalar("SELECT delivery_policy='in_app_only' FROM workflow_message_intents WHERE company_id=$1 AND id=(SELECT intent_id FROM workflow_escalation_agent_receipts WHERE company_id=$1)")
        .bind(f.people.role.company().as_uuid()).fetch_one(f.people.role.p().pool()).await.unwrap();
    assert!(private);
}
