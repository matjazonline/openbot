use super::*;

fn message(n: u128, body: &str) -> HistoryMessage {
    HistoryMessage {
        message_id: Uuid::from_u128(n),
        at: DateTime::from_timestamp(1_700_000_000 + n as i64, 0).unwrap(),
        role: MessageRole::Human,
        audience: MessageAudience::ExternalConversation,
        entry_kind: ThreadEntryKind::Conversation,
        author: "Ada".into(),
        subject: "Order".into(),
        body: body.into(),
    }
}

/// `count` messages, newest first (ids count..1), each body `body`.
fn history(count: u128, body: &str, max_tokens: u64) -> FrozenHistory {
    FrozenHistory {
        max_tokens,
        newest: (1..=count).rev().map(|n| message(n, body)).collect(),
        eligible: count as u64,
    }
}

fn ids(output: &Value) -> Vec<String> {
    output["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["message_id"].as_str().unwrap().to_owned())
        .collect()
}

fn one_item_tokens() -> u64 {
    serde_json::to_vec(&item(&message(1, "hello")))
        .unwrap()
        .len() as u64
}

#[test]
fn workflow_history_returns_the_newest_fitting_suffix_oldest_first() {
    let per = one_item_tokens();
    let output = select(&history(5, "hello", per * 3), 1 << 20).unwrap();
    assert_eq!(
        ids(&output),
        [3, 4, 5].map(|n| Uuid::from_u128(n).to_string())
    );
    assert_eq!(output["token_count"], json!(per * 3));
    assert_eq!(output["omitted"], json!(2));
    assert_eq!(
        output["items"][0]["audience"],
        json!("external_conversation")
    );
    assert_eq!(output["items"][0]["entry_kind"], json!("conversation"));
    assert_eq!(output["items"][0]["role"], json!("human"));
}

#[test]
fn workflow_history_stops_at_the_first_message_that_does_not_fit() {
    let mut frozen = history(3, "short", 0);
    // The middle (second newest) message is long: it ends the selection even though the
    // oldest would fit, so the result never has a hole.
    frozen.newest[1].body = "x".repeat(4000);
    frozen.max_tokens = one_item_tokens() * 2 + 100;
    let output = select(&frozen, 1 << 20).unwrap();
    assert_eq!(ids(&output), [Uuid::from_u128(3).to_string()]);
    assert_eq!(output["omitted"], json!(2));
}

#[test]
fn workflow_history_output_never_exceeds_the_result_byte_cap() {
    let frozen = history(40, &"y".repeat(200), 131_072);
    let full = select(&frozen, 1 << 20).unwrap();
    let cap = serde_json::to_vec(&full).unwrap().len();
    assert_eq!(full["omitted"], json!(0));
    // Exactly at the cap everything fits; one byte under drops the oldest message.
    assert_eq!(select(&frozen, cap).unwrap(), full);
    let tight = select(&frozen, cap - 1).unwrap();
    assert!(serde_json::to_vec(&tight).unwrap().len() < cap);
    assert_eq!(tight["omitted"], json!(1));
    assert_eq!(ids(&tight).len(), 39);
    assert_eq!(ids(&tight)[0], Uuid::from_u128(2).to_string());
}

#[test]
fn workflow_history_newest_too_large_returns_no_items_and_counts_all() {
    let output = select(&history(3, &"z".repeat(500), 10), 1 << 20).unwrap();
    assert_eq!(output, json!({"items": [], "token_count": 0, "omitted": 3}));
    let empty = FrozenHistory {
        max_tokens: 1000,
        newest: vec![],
        eligible: 0,
    };
    assert_eq!(
        select(&empty, 1 << 20).unwrap(),
        json!({"items": [], "token_count": 0, "omitted": 0})
    );
}

#[test]
fn workflow_history_cuts_long_bodies_at_the_character_bound() {
    let long = "é".repeat(MAX_HISTORY_TEXT + 5);
    let output = select(&history(1, &long, 131_072), 1 << 20).unwrap();
    let item = &output["items"][0];
    assert_eq!(item["truncated"], json!(true));
    assert_eq!(
        item["body"].as_str().unwrap().chars().count(),
        MAX_HISTORY_TEXT
    );
    let exact = select(&history(1, &"é".repeat(MAX_HISTORY_TEXT), 131_072), 1 << 20).unwrap();
    assert_eq!(exact["items"][0]["truncated"], json!(false));
}

#[test]
fn workflow_history_failures_classify_storage_as_retryable_and_bad_rows_as_terminal() {
    assert_eq!(
        failure(&AppError::BadRequest("x".into())).failure.class(),
        FailureClass::Terminal
    );
    assert_eq!(
        failure(&AppError::Database("x".into()))
            .failure
            .code()
            .as_str(),
        "context.unavailable"
    );
    let refused = failure(&AppError::NotFound("run".into())).failure;
    assert_eq!(
        (refused.class(), refused.code().as_str()),
        (FailureClass::Terminal, "context.rejected")
    );
}
