//! Port and registry tests. Pure: no database, no runtime, no mocks beyond one stub harness.

use std::{fs, path::Path, sync::Arc};

use serde_json::json;

use crate::services::test_support::StubHarness;

use super::*;

fn registry_with(kind: HarnessKind) -> HarnessRegistry {
    HarnessRegistry::new()
        .register(kind, Arc::new(StubHarness::new(kind)))
        .expect("the first registration of a kind is accepted")
}

#[test]
fn registering_two_harnesses_for_one_kind_is_refused() {
    let kind = HarnessKind::AiAgents;
    let registry = registry_with(kind);

    let error = registry
        .register(kind, Arc::new(StubHarness::new(kind)))
        .err()
        .expect("a second registration for the same kind is refused");

    assert_eq!(error, HarnessRegistrationError::Duplicate(kind));
    assert!(
        error.to_string().contains(kind.as_str()),
        "the message names the kind: {error}"
    );
}

/// The mismatch arm cannot fire while [`HarnessKind`] has one variant: the declared slot and the
/// registered kind are always equal. What is pinned here is that the check exists and that its
/// error names both sides -- and the count assertion is what makes this test demand a real case
/// the day a second harness arrives, rather than quietly staying vacuous.
#[test]
fn a_harness_whose_kind_disagrees_with_its_slot_is_refused() {
    assert_eq!(
        HarnessKind::ALL.len(),
        1,
        "a second harness kind exists: register one into the other's slot and assert Mismatched"
    );

    let kind = HarnessKind::AiAgents;
    assert!(
        registry_with(kind).get(kind).is_some(),
        "a harness that agrees with its slot installs"
    );

    let mismatch = HarnessRegistrationError::Mismatched {
        declared: kind,
        registered: kind,
    };
    assert!(
        mismatch.to_string().contains(kind.as_str()),
        "the message names what was registered and what was expected: {mismatch}"
    );
}

#[test]
fn require_reports_the_missing_kind_rather_than_falling_back() {
    let empty = HarnessRegistry::new();

    let error = empty
        .require(HarnessKind::AiAgents)
        .err()
        .expect("an empty registry cannot serve any kind");

    assert_eq!(error.kind(), HarnessKind::AiAgents);
    assert!(
        error.to_string().contains(HarnessKind::AiAgents.as_str()),
        "the message names the kind that is missing: {error}"
    );
    assert!(empty.get(HarnessKind::AiAgents).is_none());
    assert!(
        empty.registered().is_empty(),
        "an unregistered kind is not quietly present"
    );
}

#[test]
fn a_registered_harness_is_the_one_that_comes_back() {
    let registry = registry_with(HarnessKind::AiAgents);

    let harness = registry
        .require(HarnessKind::AiAgents)
        .expect("the registered kind resolves");

    assert_eq!(harness.kind(), HarnessKind::AiAgents);
    assert_eq!(registry.registered(), vec![HarnessKind::AiAgents]);
}

#[test]
fn an_approval_ask_round_trips_its_variants() {
    // The serialized shape is stored on the approval row, so it is a contract with rows already
    // written: an externally-tagged `type` plus the runtime's own field names.
    let args = json!({ "target_channels": ["billing"] });
    let cases = [
        (
            ApprovalTrigger::Tool {
                name: "outreach_and_await_quorum",
                args: &args,
            },
            "tool",
            json!({
                "type": "tool",
                "name": "outreach_and_await_quorum",
                "args": { "target_channels": ["billing"] },
            }),
        ),
        (
            ApprovalTrigger::Condition {
                name: "over_budget",
                matched: "true",
            },
            "condition",
            json!({ "type": "condition", "name": "over_budget", "matched": "true" }),
        ),
        (
            ApprovalTrigger::State {
                from: Some("draft"),
                to: "sending",
            },
            "state",
            json!({ "type": "state", "from": "draft", "to": "sending" }),
        ),
        (
            ApprovalTrigger::State {
                from: None,
                to: "sending",
            },
            "state",
            json!({ "type": "state", "from": null, "to": "sending" }),
        ),
    ];

    for (trigger, kind, expected) in cases {
        assert_eq!(trigger.kind(), kind);
        assert_eq!(
            serde_json::to_value(trigger).expect("a trigger serializes"),
            expected
        );
    }
}

#[test]
fn a_rejection_carries_the_reason_it_was_built_with() {
    assert_eq!(
        ApprovalVerdict::rejected("no approver is configured"),
        ApprovalVerdict::Rejected {
            reason: "no approver is configured".to_string(),
        }
    );
    assert_ne!(ApprovalVerdict::rejected("any"), ApprovalVerdict::Approved);
}

#[test]
fn every_trace_label_is_distinct_and_bounded() {
    let sources = [
        ToolTraceSource::Model,
        ToolTraceSource::Skill,
        ToolTraceSource::StateAction,
        ToolTraceSource::Plan,
        ToolTraceSource::Orchestration,
        ToolTraceSource::Spawner,
        ToolTraceSource::Other,
    ];
    let outcomes = [
        ToolTraceOutcome::Success,
        ToolTraceOutcome::Failed,
        ToolTraceOutcome::NotExecuted,
        ToolTraceOutcome::TimedOut,
        ToolTraceOutcome::Cancelled,
    ];

    // These become metric label values, so a duplicate would silently merge two different things
    // into one time series.
    let mut labels: Vec<&str> = sources.iter().map(|source| source.label()).collect();
    labels.extend(outcomes.iter().map(|outcome| outcome.label()));
    let mut seen: Vec<&str> = Vec::new();
    for label in labels {
        assert!(!label.is_empty());
        assert!(
            !seen.contains(&label),
            "{label} labels two different things"
        );
        seen.push(label);
    }

    assert!(ToolTraceOutcome::Success.is_success());
    assert!(
        outcomes
            .iter()
            .filter(|outcome| outcome.is_success())
            .count()
            == 1,
        "exactly one outcome is the routine one"
    );
}

/// The phase's own boundary, asserted rather than remembered.
///
/// This module exists to keep the agent runtime out of everything above it; a single `use
/// ai_agents::...` here would make the port an alias for one runtime's types and nothing would
/// fail. `src/application/transport/dependency_tests.rs` now does the same job over the whole
/// application layer; this stays because this module is the one that must never regress first.
#[test]
fn the_harness_ports_name_no_agent_runtime() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/application/services/harness");
    let mut scanned = 0;
    let mut offenders: Vec<String> = Vec::new();

    for entry in fs::read_dir(&directory).expect("the harness module is readable") {
        let path = entry.expect("a readable directory entry").path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        // The test files are where the forbidden name is legitimately written down.
        if !name.ends_with(".rs") || name == "tests.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        scanned += 1;
        let source = fs::read_to_string(&path).expect("a readable source file");
        for (number, line) in source.lines().enumerate() {
            let code = line.trim();
            // Prose may name the runtime -- saying which types this module exists to keep out is
            // the documentation, and a comment compiles to nothing. Only code counts.
            if code.starts_with("//") || !code.contains("ai_agents") {
                continue;
            }
            offenders.push(format!("{name}:{}: {code}", number + 1));
        }
    }

    assert!(scanned >= 3, "the scan found only {scanned} files");
    assert!(
        offenders.is_empty(),
        "the harness ports must not name a runtime; translate in the adapter instead:\n  {}",
        offenders.join("\n  ")
    );
}
