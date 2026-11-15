//! Native direct ingress with explicit pure bindings. Production default agent workflows are
//! exercised through the real company-agent Deployment runtime fixture.
use super::*;
use crate::adapters::persistence::workflow::association::{
    AssociationColumns, association_columns, association_from_columns,
};
use crate::application::ports::channel::ChannelPersistence;
use crate::application::use_cases::agent::{AgentPersistence, AgentWrite};
use crate::application::workflow::{LifecycleAuthorizer, reads::WorkflowReads};
use crate::entities::transport::PrincipalId;

const DOMAIN: &str = "direct-workflow.test";

#[path = "direct_delivery_tests.rs"]
mod delivery;

struct DirectIntents {
    s: Intents,
    principal: PrincipalId,
}

impl DirectIntents {
    async fn new() -> Self {
        let fixture = Fixture::new().await;
        let mut draft = fixture.save(None);
        let mut source: Value =
            serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
        source["input_schema"] =
            crate::application::workflow::triggers::MessageTriggerInput::schema();
        source["output_schema"] = json!(true);
        source["steps"]["start"]["with"] =
            json!({"value":{"literal":{"result":42}},"output_schema":{"literal":true}});
        draft.content.source = source.to_string();
        let s = Intents::from_draft(fixture, draft).await;
        sqlx::query("ALTER TABLE workflow_runs ENABLE TRIGGER workflow_run_snapshot_immutable")
            .execute(s.pool())
            .await
            .unwrap();
        let company = s.fixture.target.company.as_uuid();
        let agent = AgentPersistence::create(
            s.p(),
            company,
            AgentWrite {
                name: "Direct workflow agent".into(),
                slug: "direct-workflow-agent".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let principal = PrincipalId::new(
            sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND agent_id=$2")
                .bind(company)
                .bind(agent.id)
                .fetch_one(s.pool())
                .await
                .unwrap(),
        );
        let mut tx = s.pool().begin().await.unwrap();
        sqlx::query("INSERT INTO addresses(company_id,slug,target_kind,principal_id,is_primary) VALUES($1,'direct-workflow-agent','principal',$2,true)")
            .bind(company).bind(principal.as_uuid()).execute(&mut *tx).await.unwrap();
        crate::adapters::persistence::integration::email_binding::sync_principal_email_binding_on(
            &mut tx, company, principal,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        Self { s, principal }
    }

    fn target(&self) -> AddressTarget {
        AddressTarget::Principal(self.principal)
    }

    async fn binding(&self) -> WorkflowBindingId {
        let p = self.s.p();
        let service = BindingService::new(
            LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
            p.clone(),
            p.clone(),
            p.clone(),
        );
        let target = self
            .s
            .target(self.s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
        let mut request = self
            .s
            .configuration(target, None, None, Some(MessageTrigger::Any));
        request.association = RelatedAssociation::Principal(self.principal);
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

    async fn ingest(&self, target: AddressTarget) -> crate::use_cases::thread::InboundIngestResult {
        let sender: String = sqlx::query_scalar("SELECT email::text FROM users WHERE id=$1")
            .bind(self.s.owner().user_id())
            .fetch_one(self.s.pool())
            .await
            .unwrap();
        self.ingest_from(target, sender).await
    }

    async fn ingest_from(
        &self,
        target: AddressTarget,
        sender: String,
    ) -> crate::use_cases::thread::InboundIngestResult {
        use crate::adapters::protocols::email::{EmailIngressAdapter, EmailIngressTrust};
        let company = self.s.fixture.target.company.as_uuid();
        let company_slug: String =
            sqlx::query_scalar("SELECT slug::text FROM companies WHERE id=$1")
                .bind(company)
                .fetch_one(self.s.pool())
                .await
                .unwrap();
        let address = self
            .s
            .p()
            .address_for_target(company, target)
            .await
            .unwrap()
            .unwrap();
        let mail = EmailIngressAdapter::new(DOMAIN)
            .accept(
                crate::adapters::protocols::email::parser::RawInboundPayload {
                    from: sender,
                    to: format!("{}@{company_slug}.{DOMAIN}", address.slug),
                    subject: Some("Direct workflow question".into()),
                    text: Some("Please answer this direct message".into()),
                    headers: Some(format!("Message-ID: <{}@sender.test>\n", Uuid::new_v4())),
                    ..Default::default()
                },
                EmailIngressTrust::Application,
            )
            .unwrap()
            .into_inbound(
                crate::use_cases::thread::IngressOrigin::TrustedApplication,
                crate::transport::ReplyDelivery::Send,
            );
        let result =
            crate::adapters::persistence::test_support::thread_use_cases(self.s.p(), DOMAIN)
                .ingest(mail)
                .await
                .unwrap();
        assert!(result.accepted, "{:?}", result.reason());
        assert_eq!(result.intent_ids.len(), 1);
        result
    }
}

#[path = "direct_workflow_policy_tests.rs"]
mod policy;

#[test]
fn direct_binding_association_codec_preserves_every_scope_and_refuses_invalid_null_shapes() {
    let desk = Uuid::new_v4();
    let principal = PrincipalId::random();
    let thread = RelatedThreadId::new(Uuid::new_v4());
    for association in [
        RelatedAssociation::Company,
        RelatedAssociation::Channel(RelatedChannelId::new(desk)),
        RelatedAssociation::Principal(principal),
        RelatedAssociation::Thread {
            target: AddressTarget::Desk(desk),
            thread_id: thread,
        },
        RelatedAssociation::Thread {
            target: AddressTarget::Principal(principal),
            thread_id: thread,
        },
    ] {
        assert_eq!(
            association_from_columns(association_columns(association)).unwrap(),
            association
        );
    }
    for columns in [
        AssociationColumns {
            channel_id: None,
            principal_id: None,
            thread_id: Some(thread.as_uuid()),
        },
        AssociationColumns {
            channel_id: Some(desk),
            principal_id: Some(principal.as_uuid()),
            thread_id: None,
        },
        AssociationColumns {
            channel_id: Some(desk),
            principal_id: Some(principal.as_uuid()),
            thread_id: Some(thread.as_uuid()),
        },
    ] {
        assert!(association_from_columns(columns).is_err());
    }
}

#[tokio::test]
async fn direct_binding_real_ingress_admits_one_agent_run_with_exact_target_and_history_pins() {
    let f = DirectIntents::new().await;
    let binding = f.binding().await;
    let stored = f.s.state(binding).await;
    assert_eq!(
        stored.association,
        RelatedAssociation::Principal(f.principal)
    );
    let company_wide: bool = sqlx::query_scalar(
        "SELECT company_wide FROM workflow_bindings WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.fixture.target.company.as_uuid())
    .bind(binding.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert!(!company_wide);
    let accepted = f.ingest(f.target()).await;
    let thread = accepted.thread.unwrap();
    assert_eq!(thread.target, f.target());
    let admissions = f.s.admissions(admission(f.s.p()));
    assert_eq!(f.s.drain(&admissions).await, 1);
    assert_eq!(f.s.admitted(accepted.intent_ids[0]).await, ids(&[binding]));
    let runs = f.s.runs(binding).await;
    assert_eq!(runs.len(), 1);
    let summary =
        f.s.p()
            .run_summary(f.s.fixture.target.company, RunId::new(runs[0]))
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        summary.association,
        RelatedAssociation::Thread {
            target: f.target(),
            thread_id: RelatedThreadId::new(thread.id)
        }
    );
    let pinned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workflow_runs AS run JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.run_id=run.id JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id JOIN agent_history_sessions AS session ON session.company_id=run.company_id AND session.id=run.id WHERE run.id=$1 AND run.channel_id IS NULL AND run.principal_id=$2 AND job.channel_id IS NULL AND job.principal_id=run.principal_id AND job.thread_id=run.thread_id AND session.channel_id IS NULL AND session.principal_id=run.principal_id AND session.thread_id=run.thread_id)")
        .bind(runs[0]).bind(f.principal.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert!(
        pinned,
        "admission must not degrade a NULL desk into company scope"
    );
}

#[tokio::test]
async fn direct_binding_person_is_refused_by_application_and_native_guard_and_receives_no_run() {
    let f = DirectIntents::new().await;
    let p = f.s.p();
    let company = f.s.fixture.target.company;
    let person = PrincipalId::new(f.s.owner_principal());
    let service = BindingService::new(
        LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone()),
        p.clone(),
        p.clone(),
        p.clone(),
    );
    let target =
        f.s.target(f.s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
    let mut request =
        f.s.configuration(target, None, None, Some(MessageTrigger::Any));
    request.association = RelatedAssociation::Principal(person);
    let authorizer = LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone());
    assert!(matches!(
        authorizer
            .authorize(
                company,
                f.s.owner(),
                RelatedAssociation::Principal(person),
                WorkflowOperation::ManageBinding
            )
            .await,
        Err(AppError::BadRequest(_))
    ));
    assert!(service.configure(request).await.is_err());
    let error = sqlx::query("INSERT INTO workflow_bindings(company_id,id,state_revision,configuration_revision,active,principal_id) VALUES($1,$2,1,1,false,$3)")
        .bind(company.as_uuid()).bind(Uuid::new_v4()).bind(person.as_uuid()).execute(f.s.pool()).await.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23514")
    );
    let _agent_binding = f.binding().await;
    let accepted = f.ingest(AddressTarget::Principal(person)).await;
    let admissions = f.s.admissions(admission(p));
    assert_eq!(f.s.drain(&admissions).await, 1);
    assert!(f.s.admitted(accepted.intent_ids[0]).await.is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE company_id=$1")
        .bind(company.as_uuid())
        .fetch_one(f.s.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn direct_reply_real_ingress_pure_completion_and_competing_publishers_keep_the_principal_binding()
 {
    use crate::application::workflow::{batch::*, channel_reply::*, message::MessagePlanner};
    use crate::transport::{DeliveryComposer, ports::TransportRenderers};
    let DirectReplyRoot { f, scope, thread } = prepare_direct_reply().await;
    let advanced =
        f.s.p()
            .advance_pure(
                scope,
                BatchBudget::new(4, Duration::from_millis(100)).unwrap(),
            )
            .await
            .unwrap();
    assert_eq!(advanced.disposition, BatchDisposition::Completed);
    let page = ReplyPublicationPage::try_from(1).unwrap();
    let (a, b) = tokio::join!(
        f.s.p()
            .claim_replies(ReplyPublicationWorker(Uuid::new_v4()), page),
        f.s.p()
            .claim_replies(ReplyPublicationWorker(Uuid::new_v4()), page),
    );
    let claims: Vec<_> = a.unwrap().into_iter().chain(b.unwrap()).collect();
    assert_eq!(
        claims.len(),
        1,
        "only one real worker may claim the final reply"
    );
    let renderers = Arc::new(
        TransportRenderers::new()
            .register(Arc::new(
                crate::adapters::protocols::email::EmailRenderer::new(DOMAIN),
            ))
            .unwrap(),
    );
    let composer = DeliveryComposer::new(renderers, Arc::new(f.s.p().clone()));
    let publisher = ChannelReplyPublisher::new(
        f.s.p().clone(),
        MessagePlanner::new(composer, DOMAIN.into()),
    );
    let (a, b) = tokio::join!(
        publisher.publish_claim(claims[0]),
        publisher.publish_claim(claims[0])
    );
    let a = a.unwrap();
    assert_eq!(b.unwrap(), a);
    let ReplyPublicationOutcome::Published(receipt) = a else {
        panic!("{a:?}");
    };
    assert!(receipt.delivery.is_some());
    assert_direct_publication(&f, scope, thread).await;
    assert_eq!(
        publisher.publish_claim(claims[0]).await.unwrap(),
        ReplyPublicationOutcome::Published(receipt)
    );
    // Keep the real delivery worker/provider boundary out of the publication future's frame.
    Box::pin(delivery::assert_delivered(&f, receipt.delivery.unwrap())).await;
}

struct DirectReplyRoot {
    f: DirectIntents,
    scope: crate::application::workflow::activation::ActivationRequest,
    thread: Uuid,
}

async fn prepare_direct_reply() -> DirectReplyRoot {
    use crate::application::workflow::{activation::*, channel_reply::*};
    let f = DirectIntents::new().await;
    let binding = f.binding().await;
    let state = f.s.state(binding).await;
    f.s.p()
        .configure_channel_reply(ConfigureChannelReply {
            company: f.s.fixture.target.company,
            target: f.target(),
            actor: f.s.owner(),
            expected: None,
            responder: Some(ConfigureResponder {
                binding,
                expected_binding_revision: state.revision,
                contract: ChannelReplyContract {
                    format_version: 1,
                    subject: ReplyText::Literal {
                        literal: "Direct answer".into(),
                    },
                    body: ReplyText::Literal {
                        literal: "The direct reply".into(),
                    },
                },
            }),
            approval_policy: None,
        })
        .await
        .unwrap();
    let accepted = f.ingest(f.target()).await;
    let thread = accepted.thread.unwrap();
    f.s.drain(&f.s.admissions(admission(f.s.p()))).await;
    let runs = f.s.runs(binding).await;
    assert_eq!(runs.len(), 1);
    #[derive(sqlx::FromRow)]
    struct Start {
        execution_id: Uuid,
        job_id: Uuid,
    }
    let start: Start = sqlx::query_as("SELECT execution.id AS execution_id,job.id AS job_id FROM workflow_executions AS execution JOIN background_tasks AS job ON job.company_id=execution.company_id AND job.workflow_execution_id=execution.id WHERE execution.company_id=$1 AND execution.run_id=$2")
        .bind(f.s.fixture.target.company.as_uuid()).bind(runs[0]).fetch_one(f.s.pool()).await.unwrap();
    let scope = ActivationRequest {
        company: f.s.fixture.target.company,
        run: RunId::new(runs[0]),
        execution: ExecutionId::new(start.execution_id),
        job: WorkflowJobId(start.job_id),
    };
    DirectReplyRoot {
        f,
        scope,
        thread: thread.id,
    }
}

async fn assert_direct_publication(
    f: &DirectIntents,
    scope: crate::application::workflow::activation::ActivationRequest,
    thread: Uuid,
) {
    #[derive(sqlx::FromRow)]
    struct Published {
        channel_id: Option<Uuid>,
        principal_id: Option<Uuid>,
        thread_id: Uuid,
        delivery_principal: Option<Uuid>,
        binding_principal: Option<Uuid>,
        payload: Value,
        clean_text_body: String,
        target: Value,
    }
    let published: Published = sqlx::query_as("SELECT filed.channel_id,filed.principal_id,filed.thread_id,delivery.principal_id AS delivery_principal,binding.principal_id AS binding_principal,part.payload,message.clean_text_body,proposal.integrity_document->'target' AS target FROM channel_reply_decisions AS final_reply JOIN channel_reply_proposals AS proposal ON proposal.company_id=final_reply.company_id AND proposal.id=final_reply.proposal_id JOIN messages AS message ON message.company_id=final_reply.company_id AND message.id=final_reply.message_id JOIN thread_messages AS filed ON filed.company_id=message.company_id AND filed.message_id=message.id JOIN message_deliveries AS delivery ON delivery.company_id=message.company_id AND delivery.id=final_reply.delivery_id JOIN message_delivery_parts AS part ON part.company_id=delivery.company_id AND part.delivery_id=delivery.id JOIN channel_bindings AS binding ON binding.company_id=delivery.company_id AND binding.id=delivery.source_binding_id WHERE final_reply.company_id=$1 AND final_reply.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(f.s.pool()).await.unwrap();
    assert_eq!(published.channel_id, None);
    assert_eq!(published.principal_id, Some(f.principal.as_uuid()));
    assert_eq!(published.delivery_principal, published.principal_id);
    assert_eq!(published.binding_principal, published.principal_id);
    assert_eq!(published.thread_id, thread);
    assert_eq!(published.clean_text_body, "The direct reply");
    assert_eq!(published.target, json!(f.target()));
    assert_eq!(
        published.payload["body"]["relay"]["source_target"],
        json!(f.target())
    );
    let company_slug: String = sqlx::query_scalar("SELECT slug::text FROM companies WHERE id=$1")
        .bind(scope.company.as_uuid())
        .fetch_one(f.s.pool())
        .await
        .unwrap();
    assert_eq!(
        published.payload["body"]["from"],
        json!(format!("direct-workflow-agent@{company_slug}.{DOMAIN}"))
    );
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages WHERE company_id=$1 AND direction='outbound'",
    )
    .bind(scope.company.as_uuid())
    .fetch_one(f.s.pool())
    .await
    .unwrap();
    assert_eq!(
        total, 1,
        "competing publishers and replay cannot duplicate the final effect"
    );
}
