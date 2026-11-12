use super::*;
use crate::application::workflow::sample::{SampleRequest, SampleStatus};
use crate::application::workflow::sample_capture::SampleDefinitionCapture;

fn capture(d: &Document) -> crate::adapters::persistence::workflow::SampleCapture {
    crate::adapters::persistence::workflow::SampleCapture {
        persistence: d.f.persistence.clone(),
        http: vec![],
        mcp: ["get_profile", "create_canvas", "get_canvas"]
            .map(|tool| McpPublicationSelection {
                connection: d.connection,
                slot: ResourceName::parse("service").unwrap(),
                tool: TypeName::parse(tool).unwrap(),
            })
            .into(),
        refused_kinds: &[],
    }
}

async fn request(d: &Document) -> SampleRequest {
    let mut request: SampleRequest = serde_json::from_str(include_str!(
        "../../../../integrations/doop/create-document.sample.json"
    ))
    .unwrap();
    request.params["action_approval"] = params(&d.f).await["action_approval"].clone();
    request.params["result_destinations"] = params(&d.f).await["result_destinations"].clone();
    for script in &mut request.scripts {
        script.workflow_id = d.f.target.workflow;
    }
    request
}

#[tokio::test]
async fn doop_sample_shipped_document_runs_without_model_or_mcp_transport_and_requires_approval() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let capture = capture(&d);
    let request = request(&d).await;
    for approval in [false, true] {
        let mut request = request.clone();
        request
            .scripts
            .iter_mut()
            .find(|s| s.step.as_str() == "create")
            .unwrap()
            .responses[0]
            .approval = Some(approval);
        let prepared = capture
            .capture(d.f.target, DraftRevision::new(1).unwrap(), request)
            .await
            .unwrap();
        let report = crate::application::workflow::sample::execute(prepared).unwrap();
        capture.authorize_release(d.f.target).await.unwrap();
        if approval {
            assert_eq!(
                report.status,
                SampleStatus::Completed,
                "{:?}",
                report.diagnostic
            );
            // The complete guide is injected as Skills; the editor calls three granted tools.
            assert_eq!(report.usage.direct_tool_calls, 3);
            assert_eq!(
                report
                    .steps
                    .iter()
                    .filter(|step| step.kind.as_str() == "mcp.call")
                    .count(),
                4
            );
            assert_eq!(
                report
                    .steps
                    .iter()
                    .filter(|step| step.step.as_str() == "notify")
                    .count(),
                1
            );
            assert_eq!(report.agent_selections.len(), 1);
            assert_eq!(
                report.output.as_ref().unwrap()["visual_review"],
                "not_performed"
            );
            assert_eq!(
                report.output.as_ref().unwrap()["canvas_id"],
                "synthetic-canvas"
            );
        } else {
            assert_eq!(report.status, SampleStatus::Stopped);
            assert!(report.diagnostic.unwrap().contains("approval:true"));
        }
        assert!(
            d.server.log().is_empty(),
            "sample cannot open the actual transport"
        );
    }
    let mut failure = request;
    let edit = failure
        .scripts
        .iter_mut()
        .find(|s| s.step.as_str() == "edit")
        .unwrap();
    edit.responses[0]
        .tools
        .iter_mut()
        .find(|t| t.name.as_str() == "create_frame")
        .unwrap()
        .output = Some(json!({"content":[{"type":"text","text":"conflict"}],"isError":true}));
    let prepared = capture
        .capture(d.f.target, DraftRevision::new(1).unwrap(), failure)
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(prepared).unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(
        report.output.is_none(),
        "scripted draft cannot hide the MCP error"
    );
    assert_eq!(
        report.usage.direct_tool_calls, 2,
        "no tool after the error executes"
    );
    assert!(d.server.log().is_empty());
    d.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn doop_result_sample_one_under_step_budget_stops_before_final_success() {
    let d = Box::pin(Document::new(McpErrorPolicy::FailStep)).await;
    let capture = capture(&d);
    let mut draft = d.f.save(Some(DraftRevision::new(1).unwrap()));
    let mut source = document_source(&d.f);
    source["limits"]["max_steps"] = json!(16);
    draft.content.source = source.to_string();
    d.engine
        .services()
        .definitions(vec![], capture.mcp.clone())
        .save(draft)
        .await
        .unwrap();
    let prepared = capture
        .capture(
            d.f.target,
            DraftRevision::new(2).unwrap(),
            request(&d).await,
        )
        .await
        .unwrap();
    let report = crate::application::workflow::sample::execute(prepared).unwrap();
    assert_eq!(report.status, SampleStatus::Stopped);
    assert!(report.diagnostic.unwrap().contains("step limit"));
    assert!(report.output.is_none());
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|s| s.step.as_str() == "create")
            .count(),
        1
    );
    assert_eq!(
        report
            .steps
            .iter()
            .filter(|s| s.step.as_str() == "notify")
            .count(),
        1
    );
    assert!(
        d.server.log().is_empty(),
        "sample must not use an actual transport"
    );
    d.client.shutdown().await.unwrap();
}
