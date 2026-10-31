//! The run pages rendered from records a test run cannot produce without a provider: usage,
//! receipts, attempts, the annotation offer, and how an ended run shows a step it never finished.
use super::*;

/// What the run pages show for records a test run cannot produce without a provider or a
/// remote action: a run waiting on reconciliation offers the annotation, labelled as one, and a
/// step lists its model usage and receipts, escaped.
#[test]
fn reconciliation_is_offered_as_an_annotation_and_a_step_shows_usage_and_receipts() {
    use crate::application::workflow::reads::ActionSummary;
    use crate::application::workflow::run_views::{ModelCall, Receipt, StepView};
    let now = chrono::Utc::now();
    let action = ActionSummary {
        marker: Uuid::new_v4(),
        execution: Uuid::new_v4(),
        invocation: Uuid::new_v4(),
        argument_digest: "a".repeat(64),
        effect_kind: "remote".into(),
        attempt: 1,
        created_at: now,
        has_receipt: false,
        evidence_conflict: false,
        destinations: Vec::new(),
    };
    let view = sample_view("waiting", Some("reconciliation"), Vec::new(), vec![action]);
    let html = pages::run_pane(&pages::RunPaneView {
        company_id: Uuid::new_v4(),
        viewer: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
        run: &view,
        graph_html: "",
        people: &HashMap::new(),
        banner: None,
    });
    assert!(
        html.contains("Record what happened (an annotation, not a resume)"),
        "{html}"
    );
    assert!(html.contains("waiting on reconciliation"));
    assert!(html.contains("To recover, cancel it and rerun."));
    assert!(html.contains(r#"name="expected_revision" value="7""#));
    assert!(html.contains("Cancel run") && html.contains("press it again"));

    let hostile = format!("<{}>alert(1)</script>", "script");
    let step_view = StepView {
        execution: Uuid::new_v4(),
        step: "lookup".into(),
        activation: 1,
        created_at: now,
        activated_at: Some(now),
        completed_at: Some(now),
        frozen_inputs: Some(json!({"q": hostile})),
        committed_output: Some(json!({"answer": hostile})),
        frozen_choice: None,
        committed_route: Some("success".into()),
        route_target: Some("$end".into()),
        model_calls: vec![ModelCall {
            provider: "openai".into(),
            model: "gpt-test".into(),
            requested_at: now,
            settled_at: Some(now),
            input_tokens: Some(12),
            output_tokens: Some(34),
            usage_source: Some("reported".into()),
        }],
        receipts: vec![Receipt {
            dispatch: Uuid::new_v4(),
            invocation: Uuid::new_v4(),
            effect_kind: "remote".into(),
            result: json!({"status": hostile}),
            created_at: now,
        }],
        attempts: vec![crate::application::workflow::run_views::Attempt {
            number: 2,
            status: "failed".into(),
            stop_reason: Some("lease_expired".into()),
            started_at: now,
            finished_at: Some(now),
            failure_class: Some("transient".into()),
            failure_code: Some("workflow.timeout".into()),
            retry_safety: Some("safe".into()),
        }],
        children: Vec::new(),
        truncated: false,
    };
    let body = pages::run_step_body(&pages::StepBodyView {
        company_id: Uuid::new_v4(),
        step: &step_view,
        agent: Some(pages::FrozenAgentView {
            handle: "assistant",
            model: "openai / gpt-test".into(),
            tools: vec!["lookup"],
            skills: Vec::new(),
        }),
    });
    assert!(
        body.contains("Model usage") && body.contains(">12<") && body.contains(">34<"),
        "{body}"
    );
    assert!(body.contains("Action receipts") && body.contains("accepted"));
    assert!(body.contains("uses the agent's tools and skills as frozen"));
    assert!(body.contains("tools lookup; skills none"));
    assert!(body.contains("Diagnostics: worker attempts"));
    assert!(body.contains("transient: workflow.timeout") && body.contains("lease_expired"));
    assert!(!body.contains(&hostile), "{body}");
}

/// A run view with no history, for the page tests: `state`, its steps newest first, its actions.
fn sample_view(
    state: &str,
    waiting_reason: Option<&str>,
    executions: Vec<crate::application::workflow::reads::ExecutionSummary>,
    actions: Vec<crate::application::workflow::reads::ActionSummary>,
) -> RunView {
    use crate::application::workflow::reads::{RunDetail, RunSummary};
    use crate::domain::workflow::{VersionId, WorkflowId};
    let now = chrono::Utc::now();
    RunView {
        detail: RunDetail {
            summary: RunSummary {
                run: RunId::new(Uuid::new_v4()),
                binding: WorkflowBindingId::new(Uuid::new_v4()),
                workflow: WorkflowId::new(Uuid::new_v4()),
                version: VersionId::new(Uuid::new_v4()),
                association: RelatedAssociation::Company,
                state: state.into(),
                waiting_reason: waiting_reason.map(Into::into),
                revision: 7,
                actor: Uuid::new_v4(),
                parent: None,
                rerun_of: None,
                created_at: now,
                deadline: now,
                finished_at: crate::application::workflow::reads::RunStateName::parse(state)
                    .filter(|state| !state.is_open())
                    .map(|_| now),
            },
            executions,
            executions_truncated: false,
            waits: Vec::new(),
            waits_truncated: false,
            actions,
            actions_truncated: false,
        },
        workflow_title: "Lookup".into(),
        draft_revision: 1,
        channel_name: None,
        trigger_message: None,
        input: json!({}),
        params: json!({}),
        lineage: None,
        reruns: Vec::new(),
        reruns_truncated: false,
        children: Vec::new(),
        children_truncated: false,
        controls: Vec::new(),
        controls_truncated: false,
        reconciliations: Vec::new(),
        reconciliations_truncated: false,
        events: Vec::new(),
        events_truncated: false,
    }
}

/// A step that started but never finished reads as current while its run goes on, and as stopped
/// once the run has ended, on the graph and in the step list alike.
#[test]
fn an_unfinished_step_of_an_ended_run_reads_as_stopped() {
    use crate::application::workflow::reads::ExecutionSummary;
    let now = chrono::Utc::now();
    let started = ExecutionSummary {
        execution: Uuid::new_v4(),
        step: "start".into(),
        activation: 1,
        activated_at: Some(now),
        completed_at: Some(now),
        route: Some("success".into()),
    };
    let parked = ExecutionSummary {
        execution: Uuid::new_v4(),
        step: "hold".into(),
        activation: 2,
        activated_at: Some(now),
        completed_at: None,
        route: None,
    };
    let steps = vec![parked, started];
    let waiting = sample_view("waiting", Some("timer"), steps.clone(), Vec::new());
    assert_eq!(marks(&waiting)["hold"], pages::NodeMark::Current);
    let cancelled = sample_view("cancelled", None, steps, Vec::new());
    let marked = marks(&cancelled);
    assert_eq!(
        (marked["hold"], marked["start"]),
        (pages::NodeMark::Stopped, pages::NodeMark::Done)
    );
    let html = pages::run_pane(&pages::RunPaneView {
        company_id: Uuid::new_v4(),
        viewer: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
        run: &cancelled,
        graph_html: "",
        people: &HashMap::new(),
        banner: None,
    });
    assert!(
        html.contains("hold</span> · activation 2 · stopped, run cancelled"),
        "{html}"
    );
    assert!(!html.contains("running or waiting"));
}
