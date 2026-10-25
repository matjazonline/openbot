use super::*;
use crate::application::workflow::decisions::*;
use crate::domain::workflow::{CompanyId, ExecutionId, RunId, StepId, VersionId};
use serde_json::json;

fn view() -> DecisionView {
    DecisionView {
        subject: DecisionSubject::Business,
        context: DecisionContext {
            company: CompanyId::new(Uuid::new_v4()),
            workflow_title: "Review <draft>".into(),
            version: VersionId::new(Uuid::new_v4()),
            channel: None,
            thread: None,
            trigger_message: None,
            inspect_run: true,
        },
        data_schema: Some(
            json!({"type":"object","properties":{"body":{"type":"string","maxLength":32}},"required":["body"],"additionalProperties":false}),
        ),
        feedback_required: vec!["revise".into()],
        can_submit: true,
        history: vec![],
        history_truncated: false,
        decision: DecisionId(Uuid::new_v4()),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        step: StepId::parse("review").unwrap(),
        revision: 1,
        artifact_digest: "digest".into(),
        proposal: json!({"body":"Initial draft"}),
        deadline: chrono::Utc::now() + chrono::Duration::hours(1),
        state: "waiting".into(),
        reviewers: vec![],
        choices: vec!["accept".into(), "revise".into()],
        submission: None,
        comments: vec![],
    }
}

#[test]
fn decision_data_forms_preserve_raw_errors_and_protected_requests_are_immutable() {
    let mut view = view();
    view.context.channel = Some(Uuid::new_v4());
    view.context.thread = Some(Uuid::new_v4());
    let message = Uuid::new_v4();
    view.context.trigger_message = Some(message);
    let linked = workflow_decision_section(&view, None, None);
    assert!(linked.contains("href=\"/ui/message?"));
    assert!(linked.contains(&format!("message_id={message}")));
    let inputs = DecisionFormInputs::Submission(DecisionInputs {
        values: [
            ("data_mode".into(), "json".into()),
            ("data".into(), "{ invalid <script>".into()),
            ("feedback".into(), "Keep\nthis".into()),
        ]
        .into_iter()
        .collect(),
    });
    let page = workflow_decision_section(
        &view,
        Some(DecisionOutcome::Refused("invalid")),
        Some(&inputs),
    );
    assert!(page.contains("{ invalid &lt;script&gt;"));
    assert!(page.contains("Keep\nthis"));
    assert!(!page.contains("<script>"));
    assert!(page.contains("Review &lt;draft&gt;"));
    for subject in [
        DecisionSubject::ProtectedAction,
        DecisionSubject::TokenAllowance,
    ] {
        view.subject = subject;
        view.proposal = json!({"secret_effect":"immutable <request>"});
        let page = workflow_decision_section(&view, None, None);
        assert!(page.contains(subject.label()));
        assert!(page.contains("immutable &lt;request&gt;"));
        assert!(page.contains("name=\"data\" value=\"{}\""));
        assert!(!page.contains("name=\"data:body\""));
    }
    view.can_submit = false;
    let page = workflow_decision_section(&view, None, None);
    assert!(!page.contains("/submit\""));
    assert!(page.contains("assigned reviewers"));
}

/// Optional rendered fixtures let the browser exercise the actual server markup with shipped HTMX.
#[test]
fn decision_browser_markup_and_closed_draft_recovery() {
    let view = view();
    let waiting = workflow_decision_page(&view);
    assert!(waiting.contains("id=\"decision-shell\""));
    assert!(waiting.contains("hx-sync=\"#decision-shell:queue all\""));
    let mut closed = view.clone();
    closed.state = "completed".into();
    closed.can_submit = false;
    closed.revision = 2;
    closed.submission = Some(DecisionSubmissionView {
        submitted_by: Uuid::new_v4(),
        submitted_at: chrono::Utc::now(),
        choice: "accept".into(),
        feedback: "Accepted".into(),
        data: json!({"body":"Final"}),
    });
    let inputs = DecisionFormInputs::Comment(DecisionInputs {
        values: [("body".into(), "Unposted <comment>".into())]
            .into_iter()
            .collect(),
    });
    let settled = workflow_decision_section(
        &closed,
        Some(DecisionOutcome::Done("Decision submitted")),
        Some(&inputs),
    );
    assert!(settled.contains("Unposted &lt;comment&gt;"));
    assert!(!settled.contains("<form"));
    if let Some(dir) = std::env::var_os("WORKFLOW_DECISION_BROWSER_FIXTURES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("waiting.html"), waiting).unwrap();
        std::fs::write(
            dir.join("comment.html"),
            workflow_decision_section(&view, Some(DecisionOutcome::Done("Comment posted")), None),
        )
        .unwrap();
        std::fs::write(dir.join("closed.html"), settled).unwrap();
    }
}
