use super::*;
use crate::application::workflow::{
    IdempotencyKey, WorkflowInspection,
    lifecycle::{
        BindingLifecycle, DefinitionTarget, DraftContent, PublishDraft, SaveDraft,
        WorkflowDefinitions,
    },
    reads::{AuthoringReads, PageRequest, WorkflowReads},
};
use crate::domain::workflow::{DraftRevision, VersionId, WorkflowBindingId, WorkflowId};

#[tokio::test]
async fn native_mcp_company_cascade_removes_real_broker_call_and_run_with_deferred_provenance() {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    p.native_mcp(&f.request(json!({"query":"company cascade"})))
        .await
        .unwrap();
    sqlx::query("DELETE FROM companies WHERE id=$1")
        .bind(f.s.f.company)
        .execute(p.pool())
        .await
        .unwrap();
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM native_mcp_brokers WHERE company_id=$1),(SELECT count(*) FROM native_mcp_calls WHERE company_id=$1),(SELECT count(*) FROM workflow_runs WHERE company_id=$1)")
        .bind(f.s.f.company).fetch_one(p.pool()).await.unwrap();
    assert_eq!(counts, (0, 0, 0));
}

#[tokio::test]
async fn native_mcp_private_broker_is_absent_from_all_public_authoring_reads_but_run_history_is_visible()
 {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    let company = CompanyId::new(f.s.f.company);
    let response = p
        .native_mcp(&f.request(json!({"query":"private source"})))
        .await
        .unwrap();
    let (workflow,version,binding,key):(Uuid,Uuid,Uuid,String)=sqlx::query_as(
        "SELECT broker.workflow_id,broker.version_id,broker.workflow_binding_id,version.command_key FROM native_mcp_brokers AS broker JOIN workflow_versions AS version ON version.company_id=broker.company_id AND version.id=broker.version_id WHERE broker.company_id=$1",
    ).bind(company.as_uuid()).fetch_one(p.pool()).await.unwrap();
    let workflow = WorkflowId::new(workflow);
    let version = VersionId::new(version);
    let binding = WorkflowBindingId::new(binding);
    assert!(p.draft(company, workflow).await.unwrap().is_none());
    assert!(p.definition(company, workflow).await.unwrap().is_none());
    assert!(
        p.publication(company, &IdempotencyKey::parse(key).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        p.selectable_version(company, version)
            .await
            .unwrap()
            .is_none()
    );
    assert!(p.binding(company, binding).await.unwrap().is_none());
    assert!(p.binding_summary(company, binding).await.unwrap().is_none());
    assert!(
        p.list_definitions(company, PageRequest::new(None, None).unwrap())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        p.list_bindings(company, PageRequest::new(None, None).unwrap())
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        p.version_source(company, workflow, version)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        p.workflow_usage(company, workflow)
            .await
            .unwrap()
            .bindings
            .is_empty()
    );
    assert!(
        p.binding_use(company, binding.as_uuid())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        p.tool_targets(company, &[version])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(p.head(company, response.run_id).await.unwrap().is_some());
    assert!(
        p.run_summary(company, response.run_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        p.run_detail(company, response.run_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn native_mcp_private_broker_refuses_public_save_archive_and_publication_without_mutation() {
    let f = Box::pin(McpSetup::new()).await;
    let p = f.s.f.persistence.as_ref();
    p.native_mcp(&f.request(json!({"query":"private lifecycle"})))
        .await
        .unwrap();
    let workflow: Uuid =
        sqlx::query_scalar("SELECT workflow_id FROM native_mcp_brokers WHERE company_id=$1")
            .bind(f.s.f.company)
            .fetch_one(p.pool())
            .await
            .unwrap();
    let before: (i64, bool, String) = sqlx::query_as(
        "SELECT revision,archived,source FROM workflow_definitions WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(workflow)
    .fetch_one(p.pool())
    .await
    .unwrap();
    let engine = crate::infra::workflow::WorkflowEngine::for_tests(p.clone());
    let service = engine.services().definitions(vec![], vec![]);
    let target = DefinitionTarget {
        company: CompanyId::new(f.s.f.company),
        actor: WorkflowActor::authenticated(f.s.f.owner).unwrap(),
        workflow: WorkflowId::new(workflow),
    };
    assert!(
        Box::pin(service.save(SaveDraft {
            target,
            expected: Some(DraftRevision::new(1).unwrap()),
            content: DraftContent {
                title: "changed private broker".into(),
                description: String::new(),
                source: before.2.clone()
            },
        }))
        .await
        .is_err()
    );
    assert!(
        Box::pin(service.archive(target, DraftRevision::new(1).unwrap()))
            .await
            .is_err()
    );
    assert!(
        Box::pin(service.publish(PublishDraft {
            target,
            expected: DraftRevision::new(1).unwrap(),
            version: VersionId::new(Uuid::new_v4()),
            key: IdempotencyKey::parse("public-native-broker-publication").unwrap(),
        }))
        .await
        .is_err()
    );
    let after: (i64, bool, String) = sqlx::query_as(
        "SELECT revision,archived,source FROM workflow_definitions WHERE company_id=$1 AND id=$2",
    )
    .bind(f.s.f.company)
    .bind(workflow)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(before, after);
    let versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_versions WHERE company_id=$1 AND workflow_id=$2",
    )
    .bind(f.s.f.company)
    .bind(workflow)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(versions, 1);
}
