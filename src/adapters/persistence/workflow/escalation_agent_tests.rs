//! Real company-agent/default-desk creation and the existing notification worker.
use super::*;
use crate::adapters::persistence::{
    company_agent::CompanyAgentService,
    test_support::{notification_worker, provisioning_templates},
};
use crate::application::use_cases::{
    agent::{AgentPlacement, AgentWrite, CompanyAgents, CreatedAgent},
    company::{CompanyModelConnectionWrite, CompanyPersistence},
};
use crate::entities::agent_capability::{AgentBuiltinTool, AgentToolBinding};
use crate::services::test_support::{SCRIPTED_MODEL, SCRIPTED_PROVIDER};
#[path = "escalation_agent_tool_tests.rs"]
mod tool_tests;

#[path = "escalation_agent_binding_tests.rs"]
mod binding_tests;
#[path = "escalation_agent_lock_tests.rs"]
mod lock_tests;

struct AgentCase {
    people: PeopleFixture,
    created: CreatedAgent,
}

fn pending_agent() -> impl std::future::Future<Output = AgentCase> {
    pending_agent_with_tools(&[])
}

async fn pending_agent_with_tools(tools: &[AgentBuiltinTool]) -> AgentCase {
    let people = Box::pin(PeopleFixture::new(1)).await;
    let binding = &people.role.admission.binding;
    let state = binding.state().await;
    let mut request = binding.request(Some(state.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(people.role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    let prepared = people.role.prepared("canonical-agent-escalation").await;
    clear_reviewers(&people).await;
    assert!(matches!(
        people.role.p().admit(&prepared).await,
        Err(AppError::WorkflowRole(_))
    ));
    Box::pin(provision_handler(people, tools)).await
}

async fn provision_handler(people: PeopleFixture, tools: &[AgentBuiltinTool]) -> AgentCase {
    let binding = &people.role.admission.binding;
    let p = people.role.p();
    let actor = binding.target.actor;
    let company = people.role.company();
    CompanyPersistence::replace_model_connections_for_user(
        p,
        actor.user_id(),
        company.as_uuid(),
        vec![CompanyModelConnectionWrite {
            provider: SCRIPTED_PROVIDER.into(),
            api_key: Some("scripted-key".into()),
            models: vec![SCRIPTED_MODEL.into()],
            is_default: true,
        }],
    )
    .await
    .unwrap();
    let service = CompanyAgentService::new(p.clone(), provisioning_templates());
    // The actual publisher descends into agent and workflow compilation; box that
    // substantive fixture phase to retain the stock test-thread stack.
    let created = Box::pin(
        service.create(
            actor,
            company,
            AgentWrite {
                name: "Escalation handler".into(),
                tool_bindings: Some(
                    tools
                        .iter()
                        .copied()
                        .map(AgentToolBinding::builtin)
                        .collect(),
                ),
                slug: "escalation-handler".into(),
                provider: Some(SCRIPTED_PROVIDER.into()),
                model: Some(SCRIPTED_MODEL.into()),
                ..Default::default()
            },
            Some(AgentPlacement {
                department: people.role.department,
                role: Some(DepartmentRole::Reviewer),
            }),
        ),
    )
    .await
    .unwrap();
    let worker = notification_worker(p, Default::default());
    assert_eq!(
        worker.drain_once().await.unwrap(),
        1,
        "materialize the real agent hop"
    );
    let kind: String = sqlx::query_scalar(
        "SELECT principal_kind FROM workflow_escalation_notices WHERE company_id=$1",
    )
    .bind(company.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(
        kind, "agent",
        "the neutral role must preserve the actual agent holder"
    );
    AgentCase { people, created }
}

fn pending_agent_from_run() -> impl std::future::Future<Output = (AgentCase, RunId)> {
    pending_agent_from_run_with_tools(&[])
}

async fn pending_agent_from_run_with_tools(tools: &[AgentBuiltinTool]) -> (AgentCase, RunId) {
    use crate::application::app_error::WorkflowGuardViolation;
    use crate::application::workflow::guard::{
        GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
    };
    let people = Box::pin(PeopleFixture::new(1)).await;
    let binding = &people.role.admission.binding;
    let mut request = binding.request(Some(binding.state().await.revision));
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(people.role.channel));
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    binding.service().configure(request).await.unwrap();
    let run = people.role.admitted("retained-notice-source").await;
    let scope = people.scope(run).await;
    clear_reviewers(&people).await;
    assert_eq!(
        people
            .role
            .p()
            .fail_run_on_guard(GuardFailure {
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
    (Box::pin(provision_handler(people, tools)).await, run)
}

async fn facts(f: &AgentCase) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object( \
         'messages',(SELECT count(*) FROM messages WHERE company_id=$1), \
         'intents',(SELECT count(*) FROM workflow_message_intents WHERE company_id=$1), \
         'deliveries',(SELECT count(*) FROM message_deliveries WHERE company_id=$1), \
         'person_notifications',(SELECT count(*) FROM notifications WHERE company_id=$1), \
         'runs',(SELECT count(*) FROM workflow_runs WHERE company_id=$1), \
         'cases',(SELECT count(*) FROM workflow_escalation_cases WHERE company_id=$1), \
         'hops',(SELECT count(*) FROM workflow_run_escalations WHERE company_id=$1))",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap()
}

async fn notice_queue(f: &AgentCase) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('status',status,'attempts',attempt_count, \
        'class',last_error_class,'backoff',available_at>clock_timestamp()) \
        FROM notification_events WHERE company_id=$1 AND source_kind='escalation_notice'",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap()
}

async fn retry_notice(f: &AgentCase) {
    sqlx::query(
        "UPDATE notification_events SET available_at=clock_timestamp() \
        WHERE company_id=$1 AND source_kind='escalation_notice' AND status='pending'",
    )
    .bind(f.people.role.company().as_uuid())
    .execute(f.people.role.p().pool())
    .await
    .unwrap();
    assert_eq!(
        notification_worker(f.people.role.p(), Default::default())
            .drain_once()
            .await
            .unwrap(),
        1
    );
    assert_eq!(notice_queue(f).await["status"], "projected");
    assert_eq!(facts(f).await["messages"], 1);
    assert_eq!(facts(f).await["intents"], 1);
}

#[tokio::test]
async fn workflow_escalation_agent_actual_worker_commits_one_private_message_and_intent_on_default_desk()
 {
    let f = Box::pin(pending_agent()).await;
    let p = f.people.role.p();
    let worker = notification_worker(p, Default::default());
    assert_eq!(
        worker.drain_once().await.unwrap(),
        1,
        "project the actual agent recipient"
    );
    let expected = json!({"messages":1,"intents":1,"deliveries":0,"person_notifications":0,"runs":0,"cases":1,"hops":1});
    assert_eq!(
        facts(&f).await,
        expected,
        "agent routing must create a private canonical notice, with no SMTP or direct run"
    );
    let channel: Uuid =
        sqlx::query_scalar("SELECT channel_id FROM workflow_message_intents WHERE company_id=$1")
            .bind(f.people.role.company().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(channel, f.created.desk_id);
    let private_only: bool = sqlx::query_scalar(
        "SELECT message.audience='internal_only' AND intent.delivery_policy='in_app_only' \
         FROM workflow_message_intents AS intent \
         JOIN messages AS message ON message.company_id=intent.company_id AND message.id=intent.message_id \
         WHERE intent.company_id=$1",
    )
    .bind(f.people.role.company().as_uuid()).fetch_one(p.pool()).await.unwrap();
    assert!(
        private_only,
        "the notice and any reply must stay inside the application"
    );
    let origin: bool = sqlx::query_scalar(
        "SELECT receipt.agent_id=$2 AND receipt.desk_id=$3 AND receipt.binding_id=binding.id \
         AND receipt.binding_revision=binding.configuration_revision AND author.kind='system' \
         AND message.role='system' AND intent.responder_principal_id=author.id \
         FROM workflow_escalation_agent_receipts AS receipt \
         JOIN workflow_bindings AS binding ON binding.company_id=receipt.company_id AND binding.id=receipt.binding_id \
         JOIN messages AS message ON message.company_id=receipt.company_id AND message.id=receipt.message_id \
         JOIN principals AS author ON author.company_id=message.company_id AND author.id=message.author_principal_id \
         JOIN workflow_message_intents AS intent ON intent.company_id=receipt.company_id AND intent.id=receipt.intent_id \
         WHERE receipt.company_id=$1",
    ).bind(f.people.role.company().as_uuid()).bind(f.created.agent.id).bind(f.created.desk_id)
        .fetch_one(p.pool()).await.unwrap();
    assert!(
        origin,
        "the private origin proves the actual agent/default binding without an agent account"
    );
    assert_eq!(
        worker.drain_once().await.unwrap(),
        0,
        "a successful notice is not immediately reclaimed"
    );
    assert_eq!(
        facts(&f).await,
        expected,
        "replay cannot send a second canonical notice"
    );
}
