//! The internal-delegation exemption: the one path on which an agent's action skips the human.
//!
//! Every test here asserts the same property from a different angle -- that anything short of
//! "every recipient is a callable same-company agent channel, and an operator turned this on"
//! falls back to human approval.

use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::entities::channel::Channel;
use crate::services::test_support::{ChannelDirectoryStub, agent_channel};

use super::*;

fn policy(
    channels: Vec<Channel>,
    company_id: Uuid,
    requires_approval: bool,
) -> InternalDelegationPolicy {
    InternalDelegationPolicy {
        channel_persistence: Arc::new(ChannelDirectoryStub::new(channels)),
        company_id,
        source_channel_id: Uuid::new_v4(),
        requires_approval,
    }
}

fn outreach_args(channels: &[&str], emails: &[&str]) -> serde_json::Value {
    json!({ "target_channels": channels, "target_emails": emails })
}

fn outreach(args: &serde_json::Value) -> ApprovalTrigger<'_> {
    ApprovalTrigger::Tool {
        name: OUTREACH_TOOL_ID,
        args,
    }
}

#[tokio::test]
async fn an_all_internal_call_skips_the_human_when_policy_allows() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );
    let args = outreach_args(&["billing"], &[]);

    assert!(policy.approves_without_human(&outreach(&args)).await);
}

#[tokio::test]
async fn an_external_recipient_still_requires_the_human() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );
    let args = outreach_args(&[], &["stranger@supplier.example"]);

    assert!(!policy.approves_without_human(&outreach(&args)).await);
}

/// The case that justifies deciding per call instead of per tool: one stranger in the list must
/// pull the whole call back under approval.
#[tokio::test]
async fn a_mixed_call_requires_the_human() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );
    let args = outreach_args(&["billing"], &["stranger@supplier.example"]);

    assert!(!policy.approves_without_human(&outreach(&args)).await);
}

#[tokio::test]
async fn a_platform_address_with_no_such_channel_requires_the_human() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );
    let args = outreach_args(&["ghost"], &[]);

    assert!(!policy.approves_without_human(&outreach(&args)).await);
}

#[tokio::test]
async fn the_default_policy_never_skips_the_human() {
    let company_id = Uuid::new_v4();
    let policy = policy(vec![agent_channel(company_id, "billing")], company_id, true);
    let args = outreach_args(&["billing"], &[]);

    assert!(!policy.approves_without_human(&outreach(&args)).await);
}

#[tokio::test]
async fn an_empty_or_malformed_target_list_requires_the_human() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );

    let empty = outreach_args(&[], &[]);
    assert!(!policy.approves_without_human(&outreach(&empty)).await);

    let no_targets_at_all = json!({});
    assert!(
        !policy
            .approves_without_human(&outreach(&no_targets_at_all))
            .await
    );
}

/// A trigger for anything but outreach never takes the exemption, whatever it carries.
#[tokio::test]
async fn only_the_outreach_tool_can_be_exempted() {
    let company_id = Uuid::new_v4();
    let policy = policy(
        vec![agent_channel(company_id, "billing")],
        company_id,
        false,
    );
    let args = outreach_args(&["billing"], &[]);

    assert!(
        !policy
            .approves_without_human(&ApprovalTrigger::Tool {
                name: "some_other_tool",
                args: &args,
            })
            .await
    );
    assert!(
        !policy
            .approves_without_human(&ApprovalTrigger::State {
                from: Some("draft"),
                to: "sending",
            })
            .await
    );
}

#[test]
fn the_approval_flag_fails_closed_unless_explicitly_false() {
    let explicit = json!({
        "tool_security": { "tools": { OUTREACH_TOOL_ID: {
            "config": { "internal_requires_approval": false } } } }
    });
    assert!(!internal_requires_approval(&explicit));

    assert!(internal_requires_approval(&json!({})));
    let wrong_type = json!({
        "tool_security": { "tools": { OUTREACH_TOOL_ID: {
            "config": { "internal_requires_approval": "false" } } } }
    });
    assert!(internal_requires_approval(&wrong_type));
}

/// The step key is what a restarted process uses to recognise a decision it did not ask for, so
/// its inputs are fixed: the same action in the same thread hashes alike, and any change to the
/// action or the thread does not.
#[test]
fn the_step_key_identifies_the_action_and_not_the_call() {
    let args = json!({ "target_channels": ["billing"] });
    let other = json!({ "target_channels": ["legal"] });

    assert_eq!(step_text(&outreach(&args)), step_text(&outreach(&args)));
    assert_ne!(step_text(&outreach(&args)), step_text(&outreach(&other)));

    // The stored shapes, spelled out: these strings are hashed into keys that are already in the
    // database, so a change here re-asks for every approval a human has already granted.
    assert_eq!(
        step_text(&ApprovalTrigger::Condition {
            name: "over_budget",
            matched: "true",
        }),
        "condition:over_budget:true"
    );
    assert_eq!(
        step_text(&ApprovalTrigger::State {
            from: Some("draft"),
            to: "sending",
        }),
        r#"state:Some("draft"):sending"#
    );
    assert_eq!(
        step_text(&ApprovalTrigger::State {
            from: None,
            to: "sending",
        }),
        "state:None:sending"
    );
}

#[test]
fn the_action_title_names_what_a_human_is_being_asked_about() {
    let args = json!({});
    assert_eq!(
        action_title(&outreach(&args)),
        format!("Tool Execution: {OUTREACH_TOOL_ID}")
    );
    assert_eq!(
        action_title(&ApprovalTrigger::Condition {
            name: "over_budget",
            matched: "true",
        }),
        "Condition Approval: over_budget"
    );
    assert_eq!(
        action_title(&ApprovalTrigger::State {
            from: Some("draft"),
            to: "sending",
        }),
        "State Transition: sending"
    );
}
