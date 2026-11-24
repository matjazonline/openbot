//! The call section draws each reference only with its access (gate AWC-P05-D2 r2, D1 and F5).
use super::*;
use crate::application::workflow::RelatedAssociation;

fn item(phase: CallPhase) -> ChannelCallItem {
    let now = chrono::Utc::now();
    ChannelCallItem {
        call: Uuid::from_u128(1),
        execution: Uuid::from_u128(2),
        agent: Uuid::from_u128(3),
        agent_revision: Uuid::from_u128(4),
        desk: Uuid::from_u128(5),
        desk_name: "<b>finance</b>".into(),
        responder: Uuid::from_u128(0xabcdef01 << 96),
        responder_revision: 4,
        child: crate::domain::workflow::RunId::new(Uuid::from_u128(7)),
        child_association: RelatedAssociation::Company,
        child_readable: true,
        scope_manifest: Uuid::from_u128(8),
        deadline: now,
        past_deadline: true,
        created_at: now,
        phase,
    }
}

#[test]
fn channel_call_views_section_draws_only_authorized_references() {
    let decision = Uuid::from_u128(9);
    let page = ChannelCallPage {
        items: vec![item(CallPhase::AwaitingFinalReview { decision })],
        truncated: true,
    };
    let company = Uuid::from_u128(10);
    let run = Uuid::from_u128(11);
    let none = channel_calls(company, run, &page, &CallAccess::default());
    assert!(none.contains("to a desk"));
    assert!(!none.contains("finance"));
    assert!(!none.contains("responder"));
    assert!(!none.contains(&format!("run_id={}", Uuid::from_u128(7))));
    assert!(!none.contains("/ui/workflow-decisions/"));
    assert!(none.contains("Awaiting final review"));
    assert!(none.contains("past deadline"));
    assert!(none.contains("Only the newest 50 calls are listed."));
    let all = CallAccess {
        runs: [Uuid::from_u128(7)].into(),
        desks: [Uuid::from_u128(5)].into(),
        responders: [Uuid::from_u128(5)].into(),
        decisions: [decision].into(),
    };
    let html = channel_calls(company, run, &page, &all);
    assert!(html.contains("&lt;b&gt;finance&lt;/b&gt;"), "{html}");
    assert!(html.contains("responder <span class=\"font-mono\">abcdef01</span> revision 4"));
    assert!(html.contains(&format!("run_id={}", Uuid::from_u128(7))));
    assert!(html.contains(&format!("/ui/workflow-decisions/{decision}")));
    // The desk visible but its settings not readable: the desk, no responder.
    let desk_only = CallAccess {
        desks: [Uuid::from_u128(5)].into(),
        ..CallAccess::default()
    };
    let html = channel_calls(company, run, &page, &desk_only);
    assert!(html.contains("finance") && !html.contains("responder"));
    // The child readable but the decision not openable: no review link (AWC-P05-B2-F1).
    let child_only = CallAccess {
        runs: [Uuid::from_u128(7)].into(),
        ..CallAccess::default()
    };
    let html = channel_calls(company, run, &page, &child_only);
    assert!(html.contains(&format!("run_id={}", Uuid::from_u128(7))));
    assert!(!html.contains("/ui/workflow-decisions/"));
    assert!(channel_calls(company, run, &ChannelCallPage::default(), &all).is_empty());
    // scripts/tests/workflow-run-calls.mjs reads these exact renderings.
    if let Some(dir) = std::env::var_os("WORKFLOW_CALLS_BROWSER_FIXTURES") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("authorized.html"),
            channel_calls(company, run, &page, &all),
        )
        .unwrap();
        std::fs::write(dir.join("restricted.html"), none).unwrap();
    }
}
