//! Direct downstream writers must check their actual unit/agent, even without a use case.
use super::*;
use crate::application::agent::{
    AgentPublicationKey, AgentPublicationScope, AgentPublications, PublishAgent,
};
use crate::application::workflow::binding::ResourceDirectory;
use crate::application::workflow::channel_reply::{ChannelReplies, ConfigureChannelReply};
use crate::application::workflow::lifecycle::{
    BindingLifecycle, BindingService, BindingState, BindingTarget, PreparedBinding,
    SetBindingActivity,
};
use crate::application::workflow::{RelatedAssociation, WorkflowAuthorization, WorkflowOperation};
use crate::domain::workflow::{RuntimeResourceId, WorkflowBindingId};
use crate::entities::address::AddressTarget;
use crate::entities::agent_capability::AgentId;
use std::sync::Mutex;

/// Deliberately bypass the application preflight while building an otherwise valid
/// pause command. Only the real adapter's authorization may satisfy these assertions.
#[derive(Clone)]
struct CapturePause {
    p: PostgresPersistence,
    command: Arc<Mutex<Option<PreparedBinding>>>,
}

#[async_trait::async_trait]
impl WorkflowAuthorization for CapturePause {
    async fn authorize(
        &self,
        _: CompanyId,
        _: WorkflowActor,
        _: RelatedAssociation,
        _: WorkflowOperation,
    ) -> AppResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl BindingLifecycle for CapturePause {
    async fn binding(
        &self,
        company: CompanyId,
        binding: WorkflowBindingId,
    ) -> AppResult<Option<BindingState>> {
        self.p.binding(company, binding).await
    }

    async fn save_binding(&self, command: PreparedBinding) -> AppResult<()> {
        *self.command.lock().unwrap() = Some(command);
        Ok(())
    }
}

async fn pause_command(
    p: &PostgresPersistence,
    company: CompanyId,
    binding: WorkflowBindingId,
    actor: WorkflowActor,
) -> PreparedBinding {
    let capture = CapturePause {
        p: p.clone(),
        command: Arc::new(Mutex::new(None)),
    };
    let state = p.binding(company, binding).await.unwrap().unwrap();
    BindingService::new(capture.clone(), capture.clone(), p.clone(), p.clone())
        .deactivate(SetBindingActivity {
            target: BindingTarget {
                company,
                actor,
                binding,
            },
            expected: state.revision,
        })
        .await
        .unwrap();
    capture.command.lock().unwrap().take().unwrap()
}

async fn assert_desk_writers_refused(
    p: &PostgresPersistence,
    company: CompanyId,
    unit: DepartmentId,
    owner: Uuid,
    refuser: Uuid,
) {
    let owner = WorkflowActor::authenticated(owner).unwrap();
    let actor = WorkflowActor::authenticated(refuser).unwrap();
    let binding: Uuid = sqlx::query_scalar(
        "SELECT (result->>'binding_id')::uuid FROM workflow_provisioning_commands \
         WHERE company_id=$1 AND command_key=$2",
    )
    .bind(company.as_uuid())
    .bind(format!("department:{unit}"))
    .fetch_one(p.pool())
    .await
    .unwrap();
    let binding = WorkflowBindingId::new(binding);
    let before = p.binding(company, binding).await.unwrap().unwrap();
    hidden(
        p.save_binding(pause_command(p, company, binding, actor).await)
            .await,
    );
    let after = p.binding(company, binding).await.unwrap().unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.active, before.active);
    let desk: Uuid =
        sqlx::query_scalar("SELECT id FROM channels WHERE company_id=$1 AND department_id=$2")
            .bind(company.as_uuid())
            .bind(unit.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let target = AddressTarget::Desk(desk);
    let settings = p
        .channel_reply_settings(company, target, owner)
        .await
        .unwrap()
        .unwrap();
    hidden(
        p.configure_channel_reply(ConfigureChannelReply {
            company,
            target,
            actor,
            expected: Some(settings.revision),
            responder: None,
            approval_policy: None,
        })
        .await,
    );
    assert_eq!(
        p.channel_reply_settings(company, target, owner)
            .await
            .unwrap(),
        Some(settings.clone())
    );
    // The same revision and payload succeed under actual target authority.
    p.configure_channel_reply(ConfigureChannelReply {
        company,
        target,
        actor: owner,
        expected: Some(settings.revision),
        responder: None,
        approval_policy: None,
    })
    .await
    .unwrap();
    p.save_binding(pause_command(p, company, binding, owner).await)
        .await
        .unwrap();
}

async fn assert_publication_refused(
    p: &PostgresPersistence,
    company: CompanyId,
    principal: PrincipalId,
    owner: Uuid,
    refuser: Uuid,
) {
    let agent = AgentId::new(
        sqlx::query_scalar("SELECT agent_id FROM principals WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(principal.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap(),
    );
    let scope = AgentPublicationScope {
        company,
        actor: WorkflowActor::authenticated(owner).unwrap(),
        agent,
    };
    let preview = p.preview_agent(scope).await.unwrap();
    let request = PublishAgent {
        scope: AgentPublicationScope {
            actor: WorkflowActor::authenticated(refuser).unwrap(),
            ..scope
        },
        expected_draft_revision: preview.draft_revision,
        expected_active_revision: preview.active_revision,
        key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
    };
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(company.as_uuid())
    .bind(agent.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(matches!(
        p.publish_agent(request.clone()).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(
        p.active_agent_revision(scope).await.unwrap().unwrap().id,
        preview.active_revision.unwrap()
    );
    let after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_revisions WHERE company_id=$1 AND agent_id=$2",
    )
    .bind(company.as_uuid())
    .bind(agent.as_uuid())
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(after, before);
    p.publish_agent(PublishAgent { scope, ..request })
        .await
        .unwrap();
}

#[tokio::test]
async fn delegated_admin_direct_nested_writers_hide_siblings_and_foreign_targets() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool.clone()).await;
    let f = &d.f;
    let unit = f
        .use_cases
        .create_department(
            f.member,
            f.company,
            f.write("Remote", Some(d.child.id)),
            Some(f.agent_principal),
        )
        .await
        .unwrap();
    assert_desk_writers_refused(
        &f.persistence,
        d.company(),
        unit.id,
        f.owner,
        d.sibling_lead,
    )
    .await;
    assert_publication_refused(
        &f.persistence,
        d.company(),
        f.agent_principal,
        f.owner,
        d.sibling_lead,
    )
    .await;
    let foreign = Departments::on(pool).await;
    let unit = foreign
        .use_cases
        .create_department(
            foreign.owner,
            foreign.company,
            foreign.write("Foreign remote", None),
            Some(foreign.agent_principal),
        )
        .await
        .unwrap();
    assert_desk_writers_refused(
        &foreign.persistence,
        CompanyId::new(foreign.company),
        unit.id,
        foreign.owner,
        f.member,
    )
    .await;
    assert_publication_refused(
        &foreign.persistence,
        CompanyId::new(foreign.company),
        foreign.agent_principal,
        foreign.owner,
        f.member,
    )
    .await;
}

#[tokio::test]
async fn delegated_admin_unit_lead_cannot_inspect_root_owned_resources() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let d = Delegated::on(pool).await;
    let p = &d.f.persistence;
    assert!(
        manages_unit(p, d.f.member, d.company(), d.child.id)
            .await
            .unwrap()
    );
    let resource = RuntimeResourceId::new(d.company().as_uuid());
    assert!(matches!(
        p.inspect(
            d.company(),
            WorkflowActor::authenticated(d.f.member).unwrap(),
            resource
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        p.inspect(
            d.company(),
            WorkflowActor::authenticated(d.f.owner).unwrap(),
            resource
        )
        .await
        .unwrap()
        .is_some()
    );
}
