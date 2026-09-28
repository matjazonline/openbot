use super::*;
use crate::adapters::persistence::test_support::wait_until_backends_are_blocked;
use admission_history_tests::Conversation;

#[derive(Clone, Copy, Debug)]
enum Delivery {
    New,
    Replay,
    Alias,
}

async fn delivery(
    f: &AdmissionFixture,
    mode: Delivery,
    request: AdmitWorkflowRequest,
) -> PreparedAdmission {
    let mut alias = f.request("alias", request.trigger.clone());
    alias.association = request.association;
    alias.input = request.input.clone();
    let first = f.prepare(request).await;
    if matches!(mode, Delivery::New) {
        return first;
    }
    f.persistence().admit(&first).await.unwrap();
    if matches!(mode, Delivery::Replay) {
        return first;
    }
    f.prepare(alias).await
}

pub(super) async fn rejected_after_commit(
    f: &AdmissionFixture,
    command: &PreparedAdmission,
    revocation: Transaction<'_, Postgres>,
) {
    let before = f.counts().await;
    let (result, ()) = tokio::join!(f.persistence().admit(command), async {
        wait_until_backends_are_blocked(f.persistence().pool(), 1).await;
        revocation.commit().await.unwrap();
    });
    assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    assert_eq!(
        f.counts().await,
        before,
        "denied admission must write nothing"
    );
}

#[tokio::test]
async fn workflow_admit_sql_actor_revocation_blocks_new_replay_and_alias() {
    for mode in [Delivery::New, Delivery::Replay, Delivery::Alias] {
        let f = AdmissionFixture::new().await;
        let command = delivery(&f, mode, f.request("original", f.manual())).await;
        let mut revoked = f.persistence().pool().begin().await.unwrap();
        sqlx::query("DELETE FROM company_members WHERE company_id=$1 AND user_id=$2")
            .bind(f.binding.target.company.as_uuid())
            .bind(f.binding.target.actor.user_id())
            .execute(&mut *revoked)
            .await
            .unwrap();
        rejected_after_commit(&f, &command, revoked).await;
    }
}

async fn restrict_channel(f: &AdmissionFixture, c: &Conversation) -> Uuid {
    let p = f.persistence();
    p.create_user("replacement_owner", "replacement@example.test", "hash")
        .await
        .unwrap();
    let owner = p
        .get_by_email("replacement@example.test")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE companies SET user_id=$2 WHERE id=$1")
        .bind(f.binding.target.company.as_uuid())
        .bind(owner.id)
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE company_members SET role='admin' WHERE company_id=$1 AND user_id=$2")
        .bind(f.binding.target.company.as_uuid())
        .bind(f.binding.target.actor.user_id())
        .execute(p.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(c.channel)
        .execute(p.pool())
        .await
        .unwrap();
    let principal: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(f.binding.target.company.as_uuid())
            .bind(f.binding.target.actor.user_id())
            .fetch_one(p.pool())
            .await
            .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants (company_id,channel_id,principal_id,capability,provenance) VALUES ($1,$2,$3,'view','configured_allowlist')")
        .bind(f.binding.target.company.as_uuid()).bind(c.channel).bind(principal).execute(p.pool()).await.unwrap();
    principal
}

#[tokio::test]
async fn workflow_admit_sql_channel_revocation_blocks_new_replay_and_alias() {
    for mode in [Delivery::New, Delivery::Replay, Delivery::Alias] {
        let f = AdmissionFixture::new().await;
        let c = Conversation::new(&f).await;
        let principal = restrict_channel(&f, &c).await;
        let command = delivery(&f, mode, c.request(&f, "original", c.message)).await;
        let mut revoked = f.persistence().pool().begin().await.unwrap();
        sqlx::query("DELETE FROM channel_principal_grants WHERE channel_id=$1 AND principal_id=$2")
            .bind(c.channel)
            .bind(principal)
            .execute(&mut *revoked)
            .await
            .unwrap();
        rejected_after_commit(&f, &command, revoked).await;
    }
}

#[tokio::test]
async fn workflow_admit_sql_message_membership_revocation_is_observed() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let command = f.prepare(c.request(&f, "original", c.message)).await;
    let mut revoked = f.persistence().pool().begin().await.unwrap();
    sqlx::query(
        "DELETE FROM thread_messages WHERE company_id=$1 AND thread_id=$2 AND message_id=$3",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(c.thread)
    .bind(c.message)
    .execute(&mut *revoked)
    .await
    .unwrap();
    rejected_after_commit(&f, &command, revoked).await;
}

async fn resource_fixture() -> (AdmissionFixture, Uuid) {
    let mut binding = BindingFixture::new(json!([{"slot":"service","kind":"mcp"}])).await;
    let resource = binding.mcp().await;
    (AdmissionFixture::with_binding(binding).await, resource)
}

#[tokio::test]
async fn workflow_admit_sql_saved_resource_revocation_blocks_new_replay_and_alias() {
    for mode in [Delivery::New, Delivery::Replay, Delivery::Alias] {
        let (f, resource) = resource_fixture().await;
        let command = delivery(&f, mode, f.request("original", f.manual())).await;
        let mut revoked = f.persistence().pool().begin().await.unwrap();
        // MCP mutations and admission share the company-owner lock protocol.
        sqlx::query("SELECT id FROM companies WHERE id=$1 FOR UPDATE")
            .bind(f.binding.target.company.as_uuid())
            .execute(&mut *revoked)
            .await
            .unwrap();
        sqlx::query("DELETE FROM company_mcp_tool_grants WHERE company_id=$1 AND connection_id=$2")
            .bind(f.binding.target.company.as_uuid())
            .bind(resource)
            .execute(&mut *revoked)
            .await
            .unwrap();
        rejected_after_commit(&f, &command, revoked).await;
    }
}

#[tokio::test]
async fn workflow_admit_sql_deactivated_replay_uses_saved_resources_not_current_head() {
    use crate::application::use_cases::mcp::McpPersistence;
    let (f, original_resource) = resource_fixture().await;
    let original = f.prepare(f.request("original", f.manual())).await;
    f.persistence().admit(&original).await.unwrap();
    let current = f.binding.state().await;
    f.binding
        .service()
        .deactivate(SetBindingActivity {
            target: f.binding.target,
            expected: current.revision,
        })
        .await
        .unwrap();
    assert_eq!(
        f.persistence().admit(&original).await.unwrap(),
        AdmissionResult::Replayed(original.proposed_run_id())
    );
    let p = f.persistence();
    let old = p
        .list_mcp_connections(f.binding.target.company.as_uuid())
        .await
        .unwrap()
        .remove(0);
    let replacement = p
        .create_mcp_connection(
            f.binding.target.company.as_uuid(),
            crate::application::use_cases::mcp::McpConnectionWrite {
                slug: "replacement".into(),
                endpoint: old.endpoint,
                enabled: true,
                auth: old.auth,
                discovered_tools: old.discovered_tools,
                tool_grants: old.tool_grants,
            },
        )
        .await
        .unwrap();
    let mut change = f.binding.request(Some(f.binding.state().await.revision));
    change.resources.insert(
        ResourceName::parse("service").unwrap(),
        RuntimeResourceId::new(replacement.id),
    );
    f.binding.service().configure(change).await.unwrap();
    sqlx::query("UPDATE company_mcp_connections SET enabled=false WHERE id=$1")
        .bind(replacement.id)
        .execute(p.pool())
        .await
        .unwrap();
    let saved = f
        .prepare(f.request("alias", original.trigger().clone()))
        .await;
    assert_eq!(
        saved.binding().resources()[&ResourceName::parse("service").unwrap()],
        RuntimeResourceId::new(original_resource)
    );
    assert_eq!(
        p.admit(&saved).await.unwrap(),
        AdmissionResult::Replayed(original.proposed_run_id())
    );
    sqlx::query("UPDATE company_mcp_connections SET enabled=false WHERE id=$1")
        .bind(original_resource)
        .execute(p.pool())
        .await
        .unwrap();
    // Preparation performs readiness checks too; reuse a prepared alias to prove
    // the committing owner independently rechecks the saved resource after selection.
    let before = f.counts().await;
    assert!(matches!(p.admit(&saved).await, Err(AppError::NotFound(_))));
    assert_eq!(f.counts().await, before);
}
