//! Drive the real terminal owner after actual private and ordinary admissions.
use super::*;
use crate::application::app_error::WorkflowGuardViolation;
use crate::application::workflow::guard::{
    GuardFailure, GuardFailureOutcome, GuardSite, WorkflowGuardFailure,
};

async fn enable_handler_escalation(f: &AgentCase) {
    own_default_desk(f).await;
    let p = f.people.role.p();
    let binding: Uuid = sqlx::query_scalar(
        "SELECT (result->>'binding_id')::uuid FROM workflow_provisioning_commands \
         WHERE company_id=$1 AND command_key=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(crate::adapters::persistence::company_agent::default_desk_key(f.created.agent.id))
    .fetch_one(p.pool())
    .await
    .unwrap();
    let state = p
        .binding(f.people.role.company(), WorkflowBindingId::new(binding))
        .await
        .unwrap()
        .unwrap();
    let row: Configuration = sqlx::query_as(
        "SELECT configuration.version_id,configuration.params,configuration.resources,configuration.run_as_user_id \
         FROM workflow_bindings AS binding JOIN workflow_binding_revisions AS configuration \
           ON configuration.company_id=binding.company_id AND configuration.binding_id=binding.id \
           AND configuration.revision=binding.configuration_revision \
         WHERE binding.company_id=$1 AND binding.id=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(binding)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let mut request = f
        .people
        .role
        .admission
        .binding
        .request(Some(state.revision));
    request.target.binding = WorkflowBindingId::new(binding);
    request.association = RelatedAssociation::Channel(RelatedChannelId::new(f.created.desk_id));
    request.version = VersionId::new(row.version_id);
    request.params = row.params;
    request.resources = serde_json::from_value(row.resources).unwrap();
    request.run_as = Some(WorkflowActor::authenticated(row.run_as_user_id).unwrap());
    request.message_trigger = Some(MessageTrigger::Any);
    request.escalation = Some(EscalationPolicy {
        notify: DepartmentRole::Reviewer,
        hops: EscalationHops::try_from(1).unwrap(),
    });
    let configured = f
        .people
        .role
        .admission
        .binding
        .service()
        .configure(request)
        .await
        .unwrap();
    select_handler_revision(f, configured.revision).await;
}

pub(super) async fn select_handler_revision(
    f: &AgentCase,
    revision: crate::domain::workflow::BindingStateRevision,
) {
    use crate::application::workflow::channel_reply::{
        ChannelReplies, ConfigureChannelReply, ConfigureResponder,
    };
    let p = f.people.role.p();
    let company = f.people.role.company();
    let channel = RelatedChannelId::new(f.created.desk_id);
    let actor = f.people.role.admission.binding.target.actor;
    let settings = p
        .channel_reply_settings(company, channel, actor)
        .await
        .unwrap()
        .unwrap();
    let responder = settings.responder.unwrap();
    p.configure_channel_reply(ConfigureChannelReply {
        company,
        channel,
        actor,
        expected: Some(settings.revision),
        responder: Some(ConfigureResponder {
            binding: responder.binding,
            expected_binding_revision: revision,
            contract: responder.contract,
        }),
        approval_policy: settings.approval_policy,
    })
    .await
    .unwrap();
}

async fn admitted_scope(f: &AgentCase, notice: bool) -> ActivationRequest {
    let run: Uuid = sqlx::query_scalar(
        "SELECT run.id FROM workflow_runs AS run WHERE run.company_id=$1 \
         AND EXISTS(SELECT 1 FROM workflow_escalation_handling_roots AS ownership \
           WHERE ownership.company_id=run.company_id AND ownership.run_id=run.id)=$2",
    )
    .bind(f.people.role.company().as_uuid())
    .bind(notice)
    .fetch_one(f.people.role.p().pool())
    .await
    .unwrap_or_else(|error| panic!("missing admitted run, notice={notice}: {error}"));
    f.people.scope(RunId::new(run)).await
}

fn guard_failure(scope: ActivationRequest) -> GuardFailure {
    GuardFailure {
        scope,
        violation: WorkflowGuardViolation::from_constraint(
            "workflow_guard:test_notification_handling",
            "forced handler guard",
        )
        .unwrap(),
        site: GuardSite::Step,
        fence: None,
    }
}

#[tokio::test]
async fn workflow_escalation_agent_competing_terminal_owners_fail_original_recipient_without_recursion()
 {
    let f = Box::pin(pending_agent()).await;
    enable_handler_escalation(&f).await;
    let p = f.people.role.p();
    let worker = notification_worker(p, Default::default());
    assert_eq!(worker.drain_once().await.unwrap(), 1);
    process(&f, claims(&f).await.pop().unwrap()).await;
    let scope = admitted_scope(&f, true).await;
    let (a, b) = tokio::join!(
        p.fail_run_on_guard(guard_failure(scope)),
        p.fail_run_on_guard(guard_failure(scope)),
    );
    let outcomes = [a.unwrap(), b.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| **r == GuardFailureOutcome::Failed)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| **r == GuardFailureOutcome::Inactive)
            .count(),
        1
    );
    let failed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_escalation_recipient_failures WHERE company_id=$1 \
         AND failure_kind='handling' AND reason_code='escalation.notification_handling_failed'",
    )
    .bind(f.people.role.company().as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(failed, 1);
    assert_eq!(facts(&f).await["cases"], 1);
    assert_eq!(facts(&f).await["runs"], 1);
    assert_eq!(worker.drain_once().await.unwrap(), 1);
    let state: String =
        sqlx::query_scalar("SELECT state FROM workflow_escalation_progress WHERE company_id=$1")
            .bind(f.people.role.company().as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    assert_eq!(state, "owner_waiting");
    assert_eq!(worker.drain_once().await.unwrap(), 0);
    ordinary(&f).await;
    process(&f, claims(&f).await.pop().unwrap()).await;
    assert_eq!(
        p.fail_run_on_guard(guard_failure(admitted_scope(&f, false).await))
            .await
            .unwrap(),
        GuardFailureOutcome::Failed
    );
    assert_eq!(
        facts(&f).await["cases"],
        2,
        "new authenticated user work retains its own escalation"
    );
    assert_eq!(facts(&f).await["runs"], 2);
}
