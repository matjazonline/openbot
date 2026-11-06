use super::*;
use crate::application::workflow::run_views::{RunReadScope, RunViews};

async fn populated_manifest(f: &AdmissionFixture, run: RunId) -> DependencyManifest {
    let first = new_agent(f, Vec::new()).await;
    let revision = save_revision(f, &definition(f, first.id, Vec::new())).await;
    let mut manifest = manifest_for(f, run, first.id, revision).await;
    for index in 1..51 {
        let agent = new_agent(f, Vec::new()).await;
        let mut value = definition(f, agent.id, Vec::new());
        value.key = AgentKey::parse(format!("captured-{index}")).unwrap();
        let revision = save_revision(f, &value).await;
        manifest.agents.push(AgentRevisionSelection {
            agent_id: AgentId::new(agent.id),
            revision_id: AgentRevisionId::new(revision),
        });
        let workflow = WorkflowId::new(Uuid::new_v4());
        let version = f
            .binding
            .fixture
            .publish_workflow(
                workflow,
                serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap(),
            )
            .await;
        manifest.workflows.push(WorkflowVersionSelection {
            workflow_id: workflow,
            version_id: version,
        });
    }
    manifest
}

#[tokio::test]
async fn autonomous_agent_views_bound_agent_and_workflow_dependencies_with_explicit_truncation() {
    let f = AdmissionFixture::new().await;
    let root = native_runs::unsealed_root(&f, "bounded-view").await;
    let manifest = Box::pin(populated_manifest(&f, root.run)).await;
    assert_eq!(manifest.agents.len(), 51);
    assert_eq!(manifest.workflows.len(), 51);
    save_manifest(&f, &manifest).await;
    let view = f
        .persistence()
        .run_view(RunReadScope {
            company: root.company,
            run: root.run,
            viewer: f.binding.target.actor,
        })
        .await
        .unwrap()
        .unwrap();
    let selected = view.dependencies.as_ref().unwrap();
    assert_eq!(selected.agents.len(), 50);
    assert_eq!(selected.workflows.len(), 50);
    assert!(selected.agents_truncated && selected.workflows_truncated);
    let mut expected = manifest
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect::<Vec<_>>();
    expected.sort();
    expected.truncate(50);
    assert_eq!(
        selected
            .agents
            .iter()
            .map(|agent| agent.agent_id)
            .collect::<Vec<_>>(),
        expected
    );
    let html = crate::adapters::http::pages::run_body(&crate::adapters::http::pages::RunPaneView {
        company_id: root.company.as_uuid(),
        viewer: f.binding.target.actor.user_id(),
        epoch: Uuid::new_v4(),
        run: &view,
        graph_html: "",
        people: &std::collections::HashMap::new(),
        banner: None,
    });
    assert!(html.contains("additional selections are omitted"));
    assert!(!html.contains("Retain this immutable instruction"));
    assert!(!html.contains("Retain the configured facts"));
}
