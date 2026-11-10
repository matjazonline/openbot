use super::*;
use crate::application::memory::{MemorySaveReceipt, SavedMemory};

fn detail(level: u8, body: &str) -> MemoryDetail {
    MemoryDetail {
        observation_id: Uuid::new_v4(),
        kind: "fact".into(),
        title: "Title".into(),
        body: body.into(),
        occurred_at: chrono::Utc::now(),
        access_level: level,
        revision: 1,
        content_sha256: "0".repeat(64),
    }
}

/// E4: whole items until the step-output budget, the rest counted; the exposed level is the
/// fetch's and still covers the omitted items.
#[test]
fn memory_integration_load_output_omits_whole_items_and_keeps_the_exposed_level() {
    let items = vec![detail(2, "short"), detail(5, &"x".repeat(4_000))];
    let all = load_output(MemoryStepScope::Company, &items, AccessLevel::L5, 1 << 20);
    assert_eq!(all["items"].as_array().unwrap().len(), 2);
    assert_eq!(all["omitted"], 0);
    let budget = all["items"][0].to_string().len() + 200;
    let cut = load_output(
        MemoryStepScope::Agent(Uuid::nil()),
        &items,
        AccessLevel::L5,
        budget,
    );
    assert_eq!(cut["items"].as_array().unwrap().len(), 1);
    assert_eq!(cut["omitted"], 1);
    assert_eq!(cut["exposed_level"], 5);
    assert_eq!(cut["items"][0]["scope"], "agent");
    assert!(cut.to_string().len() <= budget);
}

/// F11: the carried level is the higher of the answer's label and every item's, and an item
/// without a valid level refuses the answer.
#[test]
fn memory_access_level_load_carries_the_highest_of_the_label_and_its_items() {
    let items = vec![detail(2, "a"), detail(6, "b")];
    assert_eq!(
        carried_level(AccessLevel::L0, &items),
        Some(AccessLevel::L6)
    );
    assert_eq!(
        carried_level(AccessLevel::L8, &items),
        Some(AccessLevel::L8)
    );
    assert_eq!(carried_level(AccessLevel::L3, &[]), Some(AccessLevel::L3));
    assert_eq!(carried_level(AccessLevel::L0, &[detail(10, "c")]), None);
    let output = load_output(MemoryStepScope::Company, &items, AccessLevel::L6, 1 << 20);
    assert_eq!(output["exposed_level"], 6);
}

#[test]
fn memory_integration_save_output_and_terminal_outcomes() {
    let receipt = MemorySaveReceipt {
        observations: vec![
            SavedMemory {
                observation_id: Uuid::nil(),
                revision: 1,
                access_level: 4,
            },
            SavedMemory {
                observation_id: Uuid::max(),
                revision: 2,
                access_level: 6,
            },
        ],
        replayed: true,
    };
    let output = save_output(&receipt);
    assert_eq!(output["access_level"], 6);
    assert_eq!(output["replayed"], true);
    assert_eq!(output["saved"][1]["revision"], 2);
    for (outcome, code) in [
        ("disabled", "memory.not_enabled"),
        ("expired", "memory.save_expired"),
        ("owner_removed", "memory.save_cancelled"),
        ("run_cancelled", "memory.save_cancelled"),
        ("conflict", "memory.save_failed"),
        ("refused", "memory.save_failed"),
    ] {
        let failure = save_failed(outcome).failure;
        assert_eq!(failure.code().as_str(), code, "{outcome}");
        assert_eq!(failure.class(), FailureClass::Terminal);
    }
}
