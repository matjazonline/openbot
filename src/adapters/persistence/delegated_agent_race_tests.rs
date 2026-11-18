use super::*;
use crate::adapters::persistence::test_support::{WriteBarrier, competing_writes};

impl ManagedAgents {
    async fn revoke(&self) {
        self.d
            .f
            .use_cases
            .clear_role(
                self.d.f.owner,
                self.d.f.company,
                self.d.unit.id,
                DepartmentRole::Lead,
                self.d.f.member_principal,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn delegated_admin_competing_role_removal_precedes_agent_draft_write() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let agent = m.create("draft-race", m.d.unit.id).await;
    let request = m.changes(m.actor(), agent.agent.id).await;
    let before = request.expected_revision;
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::DeskGrants,
        m.revoke(),
        m.p().save_company_agent(request),
    )
    .await;
    hidden(result.second);
    let stored = AgentPersistence::get_by_id(m.p(), agent.agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.draft_revision, before);
    assert_eq!(stored.system_prompt, Some("Answer carefully.".into()));
}

#[tokio::test]
async fn delegated_admin_competing_agent_draft_write_precedes_role_removal() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let agent = m.create("draft-first", m.d.unit.id).await;
    let request = m.changes(m.actor(), agent.agent.id).await;
    let before = request.expected_revision;
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::Agents,
        m.p().save_company_agent(request),
        m.revoke(),
    )
    .await;
    assert_eq!(result.first.unwrap().draft_revision.get(), before.get() + 1);
    let stored = AgentPersistence::get_by_id(m.p(), agent.agent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.system_prompt,
        Some("Updated within the unit.".into())
    );
    hidden(
        m.p()
            .preview_agent(m.scope(m.actor(), agent.agent.id))
            .await
            .map(drop),
    );
}

#[tokio::test]
async fn delegated_admin_competing_role_removal_precedes_agent_deletion() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let agent = m.create("delete-race", m.d.unit.id).await;
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::DeskGrants,
        m.revoke(),
        m.p()
            .delete_company_agent(m.scope(m.actor(), agent.agent.id)),
    )
    .await;
    hidden(result.second);
    assert!(
        AgentPersistence::get_by_id(m.p(), agent.agent.id)
            .await
            .unwrap()
            .is_some()
    );
    let kind: String =
        sqlx::query_scalar("SELECT kind FROM principals WHERE company_id=$1 AND id=$2")
            .bind(m.d.f.company)
            .bind(agent.principal_id.as_uuid())
            .fetch_one(m.p().pool())
            .await
            .unwrap();
    assert_eq!(kind, "agent");
}

#[tokio::test]
async fn delegated_admin_competing_agent_deletion_precedes_role_removal() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let agent = m.create("delete-first", m.d.unit.id).await;
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::Agents,
        m.p()
            .delete_company_agent(m.scope(m.actor(), agent.agent.id)),
        m.revoke(),
    )
    .await;
    result.first.unwrap();
    assert!(
        AgentPersistence::get_by_id(m.p(), agent.agent.id)
            .await
            .unwrap()
            .is_none()
    );
    let kind: String =
        sqlx::query_scalar("SELECT kind FROM principals WHERE company_id=$1 AND id=$2")
            .bind(m.d.f.company)
            .bind(agent.principal_id.as_uuid())
            .fetch_one(m.p().pool())
            .await
            .unwrap();
    assert_eq!(kind, "external");
}

#[tokio::test]
async fn delegated_admin_competing_agent_construction_finishes_before_role_removal() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::DeskGrants,
        m.service.create(
            m.actor(),
            m.d.company(),
            write("construction-first"),
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        ),
        m.revoke(),
    )
    .await;
    let created = result.first.unwrap();
    assert!(
        AgentPersistence::get_by_id(m.p(), created.agent.id)
            .await
            .unwrap()
            .is_some()
    );
    let complete: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM principals AS principal JOIN department_members AS member \
         ON member.company_id=principal.company_id AND member.principal_id=principal.id \
         JOIN agent_revisions AS revision ON revision.company_id=principal.company_id AND revision.agent_id=principal.agent_id \
         JOIN workflow_definitions AS definition ON definition.company_id=principal.company_id AND definition.owner_agent_id=principal.agent_id \
         JOIN workflow_bindings AS binding ON binding.company_id=principal.company_id AND binding.principal_id=principal.id AND binding.active \
         JOIN channel_principal_grants AS grant_row ON grant_row.company_id=principal.company_id AND grant_row.principal_id=principal.id \
         WHERE principal.company_id=$1 AND principal.agent_id=$2 AND member.department_id=$3)",
    ).bind(m.d.f.company).bind(created.agent.id).bind(m.d.unit.id.as_uuid())
        .fetch_one(m.p().pool()).await.unwrap();
    assert!(
        complete,
        "membership, publication, native binding and desk grants committed together"
    );
    hidden(
        m.p()
            .preview_agent(m.scope(m.actor(), created.agent.id))
            .await
            .map(drop),
    );
}

#[tokio::test]
async fn delegated_admin_competing_role_removal_precedes_agent_construction() {
    let Some(m) = ManagedAgents::new().await else {
        return;
    };
    let before = construction::counts(&m).await;
    let result = competing_writes(
        m.p().pool(),
        WriteBarrier::DeskGrants,
        m.revoke(),
        m.service.create(
            m.actor(),
            m.d.company(),
            write("construction-refused"),
            Some(AgentPlacement {
                department: m.d.unit.id,
                role: None,
            }),
        ),
    )
    .await;
    hidden(result.second);
    let after = construction::counts(&m).await;
    assert_eq!(after.agents, before.agents);
    assert_eq!(after.principals, before.principals);
    assert_eq!(after.revisions, before.revisions);
    assert_eq!(after.workflows, before.workflows);
    assert_eq!(after.bindings, before.bindings);
    assert_eq!(after.members, before.members);
}
